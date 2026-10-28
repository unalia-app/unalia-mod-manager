//! Installation en un clic : enchaîne resolver → téléchargement →
//! extraction → placement → déploiement → état, pour tout le plan.
//!
//! Le plan entier est transactionnel (SPEC §2.2) : si une étape échoue, tout
//! ce qui a été posé pendant cette installation est retiré, les versions
//! remplacées sont remises en place, et rien n'est enregistré.

mod nexus;

pub use nexus::{NexusFile, NexusInstall, available_updates_nexus};

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::HttpClient;
use crate::archive::{self, ArchiveError, Limits};
use crate::deploy::{self, DeployError, Deployment, Source};
use crate::download::{self, Cancel, DownloadError};
use crate::foreign::{self, Foreign};
use crate::games::GameCatalog;
use crate::loaders::bepinex::BepInEx;
use crate::loaders::rules::RulesLoader;
use crate::loaders::{Loader, PackageKind, Placement};
use crate::resolver::{self, Installed, PlanStep, ResolveError};
use crate::sources::{ModSource, SourceError};
use crate::state::{InstalledMod, State, StateError};
use crate::store::{Freed, Manifest, Store};

/// Dossiers de Modalia.
#[derive(Debug, Clone)]
pub struct Paths {
    /// Stockage des mods (`store::Store`) : chaque fichier une seule fois,
    /// et un paquet par version extraite.
    pub store: PathBuf,
    /// Archives téléchargées, retirées une fois extraites (débuts de
    /// téléchargement compris, pour reprendre).
    pub downloads: PathBuf,
}

impl Paths {
    pub fn store(&self) -> Store {
        Store::new(&self.store)
    }
}

/// Le jeu dans lequel on installe.
#[derive(Debug, Clone)]
pub struct Target<'a> {
    /// Clé du jeu dans l'état, ex. `steam:1030300`.
    pub game: &'a str,
    pub game_dir: &'a Path,
    /// Identifiant du jeu pour la source (communauté Thunderstore).
    pub community: &'a str,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "stage")]
pub enum Progress {
    Resolving,
    Downloading {
        index: usize,
        total: usize,
        name: String,
        received: u64,
        size: Option<u64>,
    },
    /// Extraction : `done` / `size` en octets décompressés.
    Extracting {
        index: usize,
        total: usize,
        name: String,
        done: u64,
        size: u64,
    },
    Installing {
        index: usize,
        total: usize,
        name: String,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error(transparent)]
    Resolve(#[from] ResolveError),
    #[error("couldn't download {name}")]
    Download {
        name: String,
        #[source]
        source: DownloadError,
    },
    #[error("couldn't unpack {name}")]
    Archive {
        name: String,
        #[source]
        source: ArchiveError,
    },
    #[error("couldn't install {name}")]
    Deploy {
        name: String,
        #[source]
        source: DeployError,
    },
    #[error("{path} is already provided by {other}")]
    Conflict { path: String, other: String },
    #[error("Unalia can't tell where the files of {0} go")]
    NothingToInstall(String),
    /// Installateur FOMOD : des options à choisir, pas encore proposées.
    #[error("{0} comes with an installer that asks you to pick options, which Unalia doesn't support yet")]
    Fomod(String),
    #[error("{0} has no file to install")]
    NoFile(String),
    #[error("Unalia can't find the game's files: is it fully installed?")]
    NoGameFiles,
    #[error("{name} {version} on GitHub doesn't have the file Unalia expects")]
    NoReleaseFile { name: String, version: String },
    #[error(transparent)]
    Source(#[from] SourceError),
    #[error(transparent)]
    State(#[from] StateError),
    #[error("BepInEx is already installed in this game outside Unalia")]
    LoaderPresent,
    #[error("other mods still need the loader: remove them first")]
    LoaderInUse,
    #[error("the loader can't be turned off: turn off the mods instead")]
    LoaderToggle,
    #[error("this mod isn't installed")]
    NotInstalled,
    #[error("this version is no longer kept: install it again from the mod's page")]
    NoPreviousVersion,
    #[error("couldn't access Unalia's storage")]
    Io(#[from] std::io::Error),
    /// Mod Nexus à plusieurs fichiers (variantes, options) : à l'utilisateur
    /// de choisir, le fichier conseillé en tête.
    #[error("choose which files of this mod to install")]
    ChooseFiles(Vec<crate::sources::nexus::ModFile>),
    /// Compte gratuit : ce fichier se télécharge depuis le site de Nexus.
    #[error("this file downloads from the Nexus Mods website")]
    NexusWebsite { file_id: u32 },
    #[error("the install was cancelled")]
    Cancelled,
}

/// Un paquet prêt à télécharger, quelle que soit sa source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    /// Connexions simultanées permises par la source (Nexus gratuit : une
    /// seule, comme Vortex). `None` : la valeur par défaut.
    pub connections: Option<usize>,
    pub id: String,
    pub name: String,
    pub author: String,
    pub version: String,
    pub icon_url: Option<String>,
    pub download_url: String,
    pub sha256: Option<String>,
    /// Fichier précis d'un mod Nexus : un mod en a plusieurs (variantes)
    /// de même version, chacun a sa place dans le cache.
    pub file_id: Option<u32>,
}

impl Package {
    fn of_step(step: &PlanStep) -> Self {
        let version = &step.version.number;
        Self {
            connections: None,
            id: step.id.clone(),
            name: step.name.clone(),
            author: step
                .id
                .split_once('-')
                .map(|(a, _)| a.to_owned())
                .unwrap_or_default(),
            version: version.clone(),
            icon_url: Some(format!(
                "https://gcdn.thunderstore.io/live/repository/icons/{}-{version}.png",
                step.id
            )),
            download_url: step.version.download_url.clone(),
            sha256: step.version.sha256.clone(),
            file_id: None,
        }
    }
}

/// Paquet téléchargé et rangé, avec la destination de ses fichiers.
struct Fetched {
    package: Package,
    /// Clé du paquet dans le stockage.
    key: String,
    manifest: Manifest,
    placements: Vec<Placement>,
    loader_root: Option<String>,
}

pub struct Installer<'a, S> {
    pub source: &'a S,
    pub catalog: Option<&'a GameCatalog>,
    pub client: &'a HttpClient,
    pub paths: &'a Paths,
    /// Annulation demandée par l'utilisateur : respectée pendant les
    /// téléchargements et extractions, jamais au milieu d'un déploiement.
    pub cancel: Option<&'a Cancel>,
}

impl<S: ModSource> Installer<'_, S> {
    /// Installe `mod_id` et ses dépendances. Renvoie les mods du jeu après
    /// l'installation.
    pub async fn install(
        &self,
        state: &mut State,
        target: &Target<'_>,
        mod_id: &str,
        mut on_progress: impl FnMut(Progress),
    ) -> Result<Vec<InstalledMod>, InstallError> {
        on_progress(Progress::Resolving);
        let existing = state.list(target.game)?;
        let plan = self.plan(&installed_of(&existing), target, mod_id).await?;

        let total = plan.steps.len();
        let mut done: Vec<Done> = Vec::new();
        for (index, step) in plan.steps.iter().enumerate() {
            if let Err(error) = self.check_cancel() {
                rollback(&self.paths.store(), target.game_dir, &done);
                return Err(error);
            }
            let mut previous = existing.iter().find(|m| m.id == step.id).cloned();
            match self
                .install_step(
                    target,
                    step,
                    index,
                    total,
                    previous.as_mut(),
                    &existing,
                    &mut on_progress,
                )
                .await
            {
                Ok(record) => done.push(Done { record, previous }),
                Err(error) => {
                    rollback(&self.paths.store(), target.game_dir, &done);
                    return Err(error);
                }
            }
        }

        let replaced = replaced(&done);
        let records: Vec<InstalledMod> = done
            .into_iter()
            .map(|d| {
                let mut record = d.record;
                // Un mod déjà là garde son statut (une dépendance mise à jour
                // reste une dépendance) ; un nouveau est volontaire s'il est
                // celui que l'utilisateur a demandé.
                record.explicit = match &d.previous {
                    Some(previous) => previous.explicit,
                    None => record.id == mod_id,
                };
                record
            })
            .collect();
        state.save_all(&records)?;
        keep_replaced(state, self.paths, &replaced)?;
        Ok(state.list(target.game)?)
    }

    /// Prépare l'installation de `mod_id` sans toucher au jeu ni à l'état :
    /// tout le plan est téléchargé et extrait. Sert pendant une partie : à la
    /// fermeture du jeu, l'installation n'a plus qu'à poser les liens.
    /// Renvoie le nombre de paquets préparés.
    pub async fn prefetch(
        &self,
        existing: &[InstalledMod],
        target: &Target<'_>,
        mod_id: &str,
    ) -> Result<usize, InstallError> {
        let plan = self.plan(&installed_of(existing), target, mod_id).await?;
        let total = plan.steps.len();
        for (index, step) in plan.steps.iter().enumerate() {
            self.fetch_step(target, step, index, total, &mut |_| {})
                .await?;
        }
        Ok(total)
    }

    async fn plan(
        &self,
        installed: &Installed,
        target: &Target<'_>,
        mod_id: &str,
    ) -> Result<resolver::InstallPlan, InstallError> {
        let plan = resolver::resolve(
            mod_id,
            installed,
            |id| async move { self.source.get(target.community, &id).await },
            |id| self.loader_root(id).is_some(),
        )
        .await?;
        if plan.steps.iter().any(|s| s.loader)
            && installed.loader.is_none()
            && self
                .loader_for(target.community)
                .installed_manually(target.game_dir)
        {
            return Err(InstallError::LoaderPresent);
        }
        Ok(plan)
    }

    #[allow(clippy::too_many_arguments)]
    async fn install_step(
        &self,
        target: &Target<'_>,
        step: &PlanStep,
        index: usize,
        total: usize,
        previous: Option<&mut InstalledMod>,
        existing: &[InstalledMod],
        on_progress: &mut impl FnMut(Progress),
    ) -> Result<InstalledMod, InstallError> {
        let package = Package::of_step(step);
        let (key, manifest) = self
            .fetch_package(target, &package, index, total, on_progress)
            .await?;
        let loader_root = step.loader.then(|| self.loader_root(&step.id)).flatten();
        let placements = self.loader_for(target.community).place(
            &step.id,
            &package_kind(loader_root.as_deref()),
            &files_of(&manifest),
        );
        let fetched = Fetched {
            package,
            key,
            manifest,
            placements,
            loader_root,
        };
        // Dernière occasion d'annuler : ensuite, le jeu est modifié.
        self.check_cancel()?;
        deploy_package(&self.paths.store(), target, fetched, previous, existing)
    }

    /// Pour un paquet déjà en cache, aucun accès réseau.
    async fn fetch_step(
        &self,
        target: &Target<'_>,
        step: &PlanStep,
        index: usize,
        total: usize,
        on_progress: &mut impl FnMut(Progress),
    ) -> Result<(String, Manifest), InstallError> {
        self.fetch_package(target, &Package::of_step(step), index, total, on_progress)
            .await
    }

    /// Clé d'une version dans le stockage.
    fn package_key(&self, game: &str, package: &Package) -> String {
        package_key(game, &package.id, &package.version, package.file_id)
    }

    /// Cette version est-elle déjà rangée, ou son archive téléchargée ?
    fn cached(&self, game: &str, id: &str, version: &str, file_id: Option<u32>) -> bool {
        self.paths
            .store()
            .has(&package_key(game, id, version, file_id))
            || self.archive_path(id, version, file_id).is_file()
    }

    /// Télécharge un paquet et range ses fichiers dans le stockage, sans
    /// toucher au jeu. Renvoie sa clé et son contenu. Déjà fait pour cette
    /// version : instantané.
    async fn fetch_package(
        &self,
        target: &Target<'_>,
        package: &Package,
        index: usize,
        total: usize,
        on_progress: &mut impl FnMut(Progress),
    ) -> Result<(String, Manifest), InstallError> {
        let name = package.name.clone();
        let store = self.paths.store();
        let key = self.package_key(target.game, package);
        if store.has(&key)
            && let Ok(manifest) = store.manifest(&key)
        {
            return Ok((key, manifest));
        }

        // Télécharger (ou reprendre l'archive déjà en cache). L'extension
        // ne compte pas : le format se lit dans le fichier.
        let archive = self.archive_path(&package.id, &package.version, package.file_id);
        let sha256 = if archive.is_file() {
            download::sha256_file(&archive)?
        } else {
            let defaults = download::Options::default();
            let options = download::Options {
                expected_sha256: package.sha256.clone(),
                connections: package.connections.unwrap_or(defaults.connections),
                cancel: self.cancel.cloned(),
                ..defaults
            };
            download::download(
                self.client,
                &package.download_url,
                &archive,
                &options,
                |received, size| {
                    on_progress(Progress::Downloading {
                        index,
                        total,
                        name: name.clone(),
                        received,
                        size,
                    });
                },
            )
            .await
            .map_err(|source| match source {
                // Annuler, c'est renoncer : le début du fichier part aussi.
                // (Quitter l'appli, elle, garde de quoi reprendre.)
                DownloadError::Cancelled => {
                    discard_partial(&archive);
                    InstallError::Cancelled
                }
                source => InstallError::Download {
                    name: name.clone(),
                    source,
                },
            })?
            .sha256
        };

        on_progress(Progress::Installing {
            index,
            total,
            name: name.clone(),
        });

        // Extraire à côté du stockage, puis y ranger chaque fichier. Sur un
        // fil à part : extraire et hacher 10 Go prend des minutes, le fil
        // asynchrone doit rester libre. La progression revient par canal.
        let scratch = store.scratch()?;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let (from, to) = (archive.clone(), scratch.join("package"));
        let cancel = self.cancel.cloned();
        let (job_store, job_key, job_sha) = (store.clone(), key.clone(), sha256.clone());
        let job = tokio::task::spawn_blocking(move || {
            let mut last = 0;
            archive::extract_cancellable(
                &from,
                &to,
                Limits::default(),
                cancel.as_ref(),
                |done, size| {
                    // Un message par Mo suffit à l'affichage.
                    if done - last >= 1024 * 1024 || done == size {
                        last = done;
                        let _ = tx.send((done, size));
                    }
                },
            )?;
            // Erreur de rangement (disque plein…) : l'archive, elle, est
            // bonne et reste en cache.
            Ok::<_, ArchiveError>(job_store.ingest(&job_key, &to, &job_sha))
        });
        while let Some((done, size)) = rx.recv().await {
            on_progress(Progress::Extracting {
                index,
                total,
                name: name.clone(),
                done,
                size,
            });
        }
        let ingested = job
            .await
            .map_err(|e| InstallError::Io(std::io::Error::other(e)))?;
        match ingested {
            Ok(Ok(manifest)) => {
                // Tout est rangé : l'archive ferait doublon.
                let _ = fs::remove_dir_all(&scratch);
                let _ = fs::remove_file(&archive);
                Ok((key, manifest))
            }
            Ok(Err(error)) => {
                let _ = fs::remove_dir_all(&scratch);
                Err(error.into())
            }
            Err(ArchiveError::Cancelled) => {
                let _ = fs::remove_dir_all(&scratch);
                Err(InstallError::Cancelled)
            }
            Err(source) => {
                // Archive illisible : on la retire du cache pour qu'un nouvel
                // essai la télécharge à nouveau.
                let _ = fs::remove_dir_all(&scratch);
                let _ = fs::remove_file(&archive);
                Err(InstallError::Archive { name, source })
            }
        }
    }

    fn check_cancel(&self) -> Result<(), InstallError> {
        match self.cancel {
            Some(cancel) if cancel.is_cancelled() => Err(InstallError::Cancelled),
            _ => Ok(()),
        }
    }

    /// Archive d'une version dans le cache des téléchargements.
    fn archive_path(&self, id: &str, version: &str, file_id: Option<u32>) -> PathBuf {
        let file = file_id.map(|f| format!("-f{f}")).unwrap_or_default();
        self.paths
            .downloads
            .join(format!("{id}-{}{file}.zip", path_segment(version)))
    }

    /// Adaptateur du jeu : ses règles de placement si le catalogue les
    /// connaît, sinon BepInEx (catalogue indisponible, premier lancement hors
    /// ligne).
    fn loader_for(&self, community: &str) -> Box<dyn Loader + '_> {
        match self
            .catalog
            .and_then(|c| c.by_community(community))
            .filter(|g| g.install_rules.installable())
        {
            Some(game) => Box::new(RulesLoader {
                loader: &game.loader,
                rules: &game.install_rules,
            }),
            None => Box::new(BepInEx),
        }
    }

    fn loader_root(&self, id: &str) -> Option<String> {
        self.catalog?
            .loader_package(id)
            .map(|l| l.root_folder.clone())
    }
}

/// Pose un paquet rangé dans le jeu et renvoie son enregistrement. Une
/// mise à jour retire d'abord l'ancienne version, dont les configs
/// modifiées sont gardées dans `previous` ; en cas d'échec, elle est
/// remise.
fn deploy_package(
    store: &Store,
    target: &Target<'_>,
    fetched: Fetched,
    mut previous: Option<&mut InstalledMod>,
    existing: &[InstalledMod],
) -> Result<InstalledMod, InstallError> {
    let Fetched {
        package,
        key,
        manifest,
        placements,
        loader_root,
    } = fetched;
    if let Some((path, other)) = owned_elsewhere(&placements, existing, &package.id) {
        return Err(InstallError::Conflict { path, other });
    }
    if let Some(old) = previous.as_deref_mut() {
        take_down(store, target.game_dir, old, false);
    }
    // Même version réinstallée : les configs modifiées restent.
    let edits = previous
        .as_deref()
        .filter(|old| old.package == key)
        .map(|old| old.edits.clone())
        .unwrap_or_default();
    let from = Source {
        store,
        manifest: &manifest,
        edits: &edits,
    };
    let deployment = match deploy::deploy(from, target.game_dir, &placements) {
        Ok(d) => d,
        Err(source) => {
            if let Some(old) = previous.as_deref() {
                redeploy(store, target.game_dir, old);
            }
            return Err(InstallError::Deploy {
                name: package.name,
                source,
            });
        }
    };
    Ok(InstalledMod {
        game: target.game.to_owned(),
        id: package.id,
        name: package.name,
        author: package.author,
        version: package.version,
        icon_url: package.icon_url,
        loader_root,
        enabled: true,
        explicit: false,
        sha256: manifest.sha256,
        installed_at: now(),
        package: key,
        legacy_store_dir: String::new(),
        files: package.file_id.into_iter().collect(),
        deployment,
        placements,
        edits,
        previous_versions: Vec::new(),
    })
}

/// Contenu d'un mod enregistré, depuis le stockage.
fn manifest_of(store: &Store, m: &InstalledMod) -> Result<Manifest, InstallError> {
    store.manifest(&m.package).map_err(|_| InstallError::Deploy {
        name: m.name.clone(),
        source: DeployError::MissingSource(m.package.clone()),
    })
}

/// Pose un mod enregistré, avec ses configs modifiées.
fn put_up(store: &Store, game_dir: &Path, m: &InstalledMod) -> Result<Deployment, InstallError> {
    let manifest = manifest_of(store, m)?;
    let from = Source {
        store,
        manifest: &manifest,
        edits: &m.edits,
    };
    deploy::deploy(from, game_dir, &placements_of(m, &manifest)).map_err(|source| {
        InstallError::Deploy {
            name: m.name.clone(),
            source,
        }
    })
}

/// Retire un mod du jeu, en gardant d'abord dans `m.edits` les configs que
/// le joueur y a modifiées. Renvoie ce qui n'a pas pu être retiré.
fn take_down(store: &Store, game_dir: &Path, m: &mut InstalledMod, purge_dirs: bool) -> Vec<String> {
    if let Ok(manifest) = store.manifest(&m.package) {
        let from = Source {
            store,
            manifest: &manifest,
            edits: &m.edits,
        };
        if let Ok(edits) = deploy::capture_edits(from, game_dir, &placements_of(m, &manifest)) {
            m.edits = edits;
        }
    }
    deploy::undeploy(game_dir, &m.deployment, purge_dirs)
}

/// Les fichiers d'un paquet, dans l'ordre.
fn files_of(manifest: &Manifest) -> Vec<String> {
    manifest.files.keys().cloned().collect()
}

fn package_key(game: &str, id: &str, version: &str, file_id: Option<u32>) -> String {
    let version = match file_id {
        Some(file) => format!("{}-f{file}", path_segment(version)),
        None => path_segment(version),
    };
    format!("{}/{version}", mod_prefix(game, id))
}

/// Dossier des paquets d'un mod dans le stockage (`<jeu>/<mod>`).
fn mod_prefix(game: &str, id: &str) -> String {
    format!("{}/{}", game_segment(game), path_segment(id))
}

fn game_segment(game: &str) -> String {
    path_segment(&game.replace(':', "-"))
}

/// Un fichier que ce paquet veut poser appartient-il déjà à un autre mod ?
/// Renvoie le chemin et le nom de ce mod, pour un message qui dit lequel.
fn owned_elsewhere(
    placements: &[Placement],
    existing: &[InstalledMod],
    id: &str,
) -> Option<(String, String)> {
    let wanted: std::collections::HashSet<String> = placements
        .iter()
        .map(|p| p.target.to_ascii_lowercase())
        .collect();
    existing.iter().filter(|m| m.id != id).find_map(|m| {
        let path = m
            .deployment
            .files
            .iter()
            .find(|f| wanted.contains(&f.to_ascii_lowercase()))?;
        Some((path.clone(), m.name.clone()))
    })
}

/// Retire le début d'un téléchargement et son journal de reprise.
fn discard_partial(archive: &Path) {
    let _ = fs::remove_file(archive.with_extension("part"));
    let _ = fs::remove_file(archive.with_extension("part.json"));
}

/// Débuts de téléchargement d'un mod (toutes versions) laissés dans le
/// cache, ex. après avoir quitté l'appli : de quoi reprendre, ou à jeter.
/// Noms : `<id>-<version>.part` et son journal `.part.json`.
pub fn partial_downloads(paths: &Paths, id: &str) -> Vec<PathBuf> {
    let prefix = format!("{id}-");
    let Ok(entries) = fs::read_dir(&paths.downloads) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                n.starts_with(&prefix) && (n.ends_with(".part") || n.ends_with(".part.json"))
            })
        })
        .collect()
}

/// Jette les débuts de téléchargement d'un mod.
pub fn discard_partial_downloads(paths: &Paths, id: &str) {
    for file in partial_downloads(paths, id) {
        let _ = fs::remove_file(file);
    }
}

/// Segment de chemin sûr tiré d'une version (`1.2/beta` → `1.2_beta`).
fn path_segment(version: &str) -> String {
    let cleaned: String = version
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let cleaned = cleaned.trim_matches('.');
    if cleaned.is_empty() {
        "0".into()
    } else {
        cleaned.to_owned()
    }
}

/// Ce que le resolver doit savoir des mods déjà dans le jeu.
fn installed_of(existing: &[InstalledMod]) -> Installed {
    Installed {
        versions: existing
            .iter()
            .map(|m| (m.id.clone(), m.version.clone()))
            .collect(),
        loader: existing
            .iter()
            .find(|m| m.is_loader())
            .map(|m| m.id.clone()),
    }
}

struct Done {
    record: InstalledMod,
    previous: Option<InstalledMod>,
}

fn rollback(store: &Store, game_dir: &Path, done: &[Done]) {
    for d in done.iter().rev() {
        deploy::undeploy(game_dir, &d.record.deployment, d.record.is_loader());
        if let Some(old) = &d.previous {
            redeploy(store, game_dir, old);
        }
    }
}

/// Versions remplacées par une installation (une réinstallation de la même
/// version ne remplace rien).
fn replaced(done: &[Done]) -> Vec<InstalledMod> {
    done.iter()
        .filter_map(|d| d.previous.clone().filter(|p| p.package != d.record.package))
        .collect()
}

/// Garde les versions remplacées pour y revenir, puis fait le ménage du
/// stockage.
fn keep_replaced(
    state: &mut State,
    paths: &Paths,
    replaced: &[InstalledMod],
) -> Result<(), InstallError> {
    let at = now();
    for old in replaced.iter().filter(|old| !old.package.is_empty()) {
        state.push_previous(old, at)?;
    }
    collect_garbage(state, paths)?;
    Ok(())
}

/// Ménage du stockage : retire ce qu'aucun mod installé, aucune version
/// gardée ni aucune config modifiée n'utilise.
pub fn collect_garbage(state: &State, paths: &Paths) -> Result<Freed, InstallError> {
    let (packages, blobs) = state.referenced()?;
    Ok(paths.store().gc(&packages, &blobs)?)
}

/// Remet en place une version déjà enregistrée (annulation d'une mise à
/// jour ratée). Au mieux : l'utilisateur pourra réinstaller sinon.
fn redeploy(store: &Store, game_dir: &Path, old: &InstalledMod) {
    let _ = put_up(store, game_dir, old);
}

fn package_kind(loader_root: Option<&str>) -> PackageKind {
    match loader_root {
        Some(root) => PackageKind::Loader {
            root_folder: root.to_owned(),
        },
        None => PackageKind::Mod,
    }
}

/// Placements enregistrés à l'installation ; pour un mod installé avant
/// leur ajout, recalcul avec les règles BepInEx d'alors.
fn placements_of(m: &InstalledMod, manifest: &Manifest) -> Vec<Placement> {
    if !m.placements.is_empty() {
        return m.placements.clone();
    }
    bepinex_placements(&m.id, m.loader_root.as_deref(), &files_of(manifest))
}

/// Où vont les fichiers d'un paquet, selon les règles BepInEx.
fn bepinex_placements(id: &str, loader_root: Option<&str>, files: &[String]) -> Vec<Placement> {
    BepInEx.place(id, &package_kind(loader_root), files)
}

fn list_files(dir: &Path) -> std::io::Result<Vec<String>> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) -> std::io::Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                walk(root, &path, out)?;
            } else if let Ok(rel) = path.strip_prefix(root) {
                out.push(
                    rel.components()
                        .map(|c| c.as_os_str().to_string_lossy())
                        .collect::<Vec<_>>()
                        .join("/"),
                );
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out)?;
    out.sort();
    Ok(out)
}

/// Mise à jour disponible pour un mod installé.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Update {
    pub mod_id: String,
    pub current: String,
    pub latest: String,
}

/// Mises à jour disponibles parmi `installed`. Rien n'est installé : on
/// prévient, l'utilisateur valide (SPEC §3.5). Un mod que la source ne
/// trouve plus, ou une source injoignable, n'empêche pas de vérifier les
/// autres.
pub async fn available_updates<S: ModSource>(
    source: &S,
    community: &str,
    installed: &[InstalledMod],
) -> Vec<Update> {
    let mut updates = Vec::new();
    for m in installed {
        let Ok(latest) = source.get(community, &m.id).await else {
            continue;
        };
        if let Some(version) = latest.latest()
            && resolver::compare_versions(&version.number, &m.version)
                == std::cmp::Ordering::Greater
        {
            updates.push(Update {
                mod_id: m.id.clone(),
                current: m.version.clone(),
                latest: version.number.clone(),
            });
        }
    }
    updates
}

/// Active ou désactive un mod : ses fichiers quittent le jeu (ou y
/// reviennent), le stockage les garde.
pub fn set_enabled(
    state: &mut State,
    paths: &Paths,
    game: &str,
    game_dir: &Path,
    id: &str,
    enabled: bool,
) -> Result<InstalledMod, InstallError> {
    let mut m = state.get(game, id)?.ok_or(InstallError::NotInstalled)?;
    if m.is_loader() {
        return Err(InstallError::LoaderToggle);
    }
    if m.enabled == enabled {
        return Ok(m);
    }
    let store = paths.store();
    if enabled {
        m.deployment = put_up(&store, game_dir, &m)?;
    } else {
        take_down(&store, game_dir, &mut m, false);
        m.deployment = Deployment::default();
    }
    m.enabled = enabled;
    state.save(&m)?;
    Ok(m)
}

/// Désinstalle un mod. Le loader ne part qu'en dernier, avec tout ce que
/// BepInEx a écrit dans ses dossiers : le jeu retrouve son état d'origine.
/// Le stockage est conservé pour permettre l'annulation (`restore`), puis
/// libéré par `purge`.
pub fn uninstall(
    state: &mut State,
    paths: &Paths,
    game: &str,
    game_dir: &Path,
    id: &str,
) -> Result<InstalledMod, InstallError> {
    let mut m = state.get(game, id)?.ok_or(InstallError::NotInstalled)?;
    if m.is_loader() && state.list(game)?.iter().any(|other| other.id != m.id) {
        return Err(InstallError::LoaderInUse);
    }
    let purge_dirs = m.is_loader();
    take_down(&paths.store(), game_dir, &mut m, purge_dirs);
    state.remove(game, id)?;
    Ok(m)
}

/// Retire tous les mods d'un jeu, loader en dernier : le jeu retrouve son
/// état d'origine. Sert à la désinstallation d'Unalia, quand sa base va
/// disparaître. Renvoie le nombre de mods retirés.
pub fn uninstall_all(state: &mut State, game: &str, game_dir: &Path) -> Result<usize, InstallError> {
    let (loaders, mods): (Vec<_>, Vec<_>) = state.list(game)?.into_iter().partition(InstalledMod::is_loader);
    let count = loaders.len() + mods.len();
    for m in mods.iter().chain(&loaders) {
        deploy::undeploy(game_dir, &m.deployment, m.is_loader());
        state.remove(game, &m.id)?;
    }
    Ok(count)
}

/// Jeux Bethesda : `plugins.txt` (`list`) suit les plugins des mods actifs,
/// après chaque changement. Renvoie `true` s'il a changé.
pub fn sync_plugins(state: &State, game: &str, game_dir: &Path, list: &Path) -> Result<bool, InstallError> {
    let deployed: Vec<String> = state
        .list(game)?
        .into_iter()
        .filter(|m| m.enabled)
        .flat_map(|m| m.deployment.files)
        .collect();
    Ok(crate::loaders::bethesda::sync_plugins(game_dir, list, &deployed)?)
}

/// Mods actifs dont au moins un fichier a quitté le jeu : jeu réinstallé,
/// fichiers vérifiés par Steam, ou effacés à la main. Loader en tête.
pub fn missing(state: &State, game: &str, game_dir: &Path) -> Result<Vec<InstalledMod>, InstallError> {
    Ok(state
        .list(game)?
        .into_iter()
        .filter(|m| {
            m.enabled
                && m.deployment
                    .files
                    .iter()
                    .any(|file| fs::symlink_metadata(game_dir.join(file)).is_err())
        })
        .collect())
}

/// Repose un mod depuis le stockage, sans rien retélécharger. Ce qui
/// restait de lui dans le jeu est retiré d'abord.
pub fn put_back(
    state: &mut State,
    paths: &Paths,
    game: &str,
    game_dir: &Path,
    id: &str,
) -> Result<InstalledMod, InstallError> {
    let mut m = state.get(game, id)?.ok_or(InstallError::NotInstalled)?;
    let store = paths.store();
    take_down(&store, game_dir, &mut m, false);
    m.deployment = put_up(&store, game_dir, &m)?;
    state.save(&m)?;
    Ok(m)
}

/// Revient à une version précédente d'un mod, gardée dans le stockage :
/// rien n'est retéléchargé. La version quittée devient à son tour une
/// version précédente. Un mod désactivé change de version sans revenir
/// dans le jeu.
pub fn revert(
    state: &mut State,
    paths: &Paths,
    game: &str,
    game_dir: &Path,
    id: &str,
    package: &str,
) -> Result<InstalledMod, InstallError> {
    let mut current = state.get(game, id)?.ok_or(InstallError::NotInstalled)?;
    let old = current
        .previous_versions
        .iter()
        .find(|v| v.package == package)
        .cloned()
        .ok_or(InstallError::NoPreviousVersion)?;
    let store = paths.store();
    let manifest = store
        .manifest(&old.package)
        .map_err(|_| InstallError::NoPreviousVersion)?;
    let mut next = InstalledMod {
        version: old.version,
        sha256: old.sha256,
        installed_at: old.installed_at,
        files: old.files,
        placements: if old.placements.is_empty() {
            bepinex_placements(id, current.loader_root.as_deref(), &files_of(&manifest))
        } else {
            old.placements
        },
        edits: old.edits,
        package: old.package,
        legacy_store_dir: String::new(),
        deployment: Deployment::default(),
        previous_versions: Vec::new(),
        ..current.clone()
    };
    if current.enabled {
        let existing = state.list(game)?;
        if let Some((path, other)) = owned_elsewhere(&next.placements, &existing, id) {
            return Err(InstallError::Conflict { path, other });
        }
        take_down(&store, game_dir, &mut current, false);
        let from = Source {
            store: &store,
            manifest: &manifest,
            edits: &next.edits,
        };
        next.deployment = match deploy::deploy(from, game_dir, &next.placements) {
            Ok(d) => d,
            Err(source) => {
                redeploy(&store, game_dir, &current);
                return Err(InstallError::Deploy {
                    name: current.name,
                    source,
                });
            }
        };
    }
    state.remove_previous(game, id, &next.package)?;
    state.save(&next)?;
    state.push_previous(&current, now())?;
    state.get(game, id)?.ok_or(InstallError::NotInstalled)
}

/// Oublie un jeu désinstallé : ses mods quittent la base et leur stockage
/// est libéré. Rien n'est retiré du jeu : son dossier n'existe plus.
pub fn forget(state: &mut State, paths: &Paths, game: &str) -> Result<usize, InstallError> {
    let mods = state.list(game)?;
    for m in &mods {
        state.remove(game, &m.id)?;
        purge(state, paths, game, &m.id)?;
    }
    state.set_pending(game, &[])?;
    state.forget_game_previous(game)?;
    paths.store().remove_packages(&game_segment(game))?;
    collect_garbage(state, paths)?;
    Ok(mods.len())
}

/// Retire un jeu d'Unalia : ses mods quittent son dossier, qui retrouve
/// son état d'origine (`game_dir` absent : le dossier n'existe plus), puis
/// la base et le stockage. Le jeu lui-même n'est pas touché. Renvoie le
/// nombre de mods retirés.
pub fn remove_game(
    state: &mut State,
    paths: &Paths,
    game: &str,
    game_dir: Option<&Path>,
) -> Result<usize, InstallError> {
    let removed = match game_dir {
        Some(dir) => uninstall_all(state, game, dir)?,
        None => 0,
    };
    Ok(removed + forget(state, paths, game)?)
}

/// Fichiers de mods posés dans le jeu hors d'Unalia (voir `foreign`).
/// `loader` : celui du profil du jeu.
pub fn foreign_files(state: &State, game: &str, game_dir: &Path, loader: &str) -> Result<Vec<Foreign>, InstallError> {
    let (mut files, mut dirs) = (HashSet::new(), HashSet::new());
    for m in state.list(game)? {
        files.extend(m.deployment.files);
        dirs.extend(m.deployment.created_dirs);
    }
    Ok(foreign::scan(loader, game_dir, &files, &dirs))
}

/// Bilan d'une remise à zéro.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reset {
    /// Mods d'Unalia retirés.
    pub mods: usize,
    /// Emplacements laissés par d'autres outils, retirés.
    pub foreign: usize,
    /// Ceux qui n'ont pas pu partir.
    pub left: Vec<String>,
}

/// Remet le jeu à zéro côté mods : ceux d'Unalia le quittent et leur
/// stockage est libéré, puis ce que d'autres outils ont laissé passe par
/// `discard` (la corbeille), mais seulement les emplacements de `remove`,
/// cochés par l'utilisateur (voir [`Foreign::sure`]). Le reste et les
/// fichiers du jeu ne sont pas touchés.
pub fn reset(
    state: &mut State,
    paths: &Paths,
    game: &str,
    game_dir: &Path,
    loader: &str,
    remove: &HashSet<String>,
    discard: impl Fn(&Path) -> io::Result<()>,
) -> Result<Reset, InstallError> {
    let before = foreign_files(state, game, game_dir, loader)?;
    // Comme `uninstall_all`, mais les dossiers du loader ne sont pas vidés
    // de force : ce qu'on y avait glissé à la main (un plugin dans
    // `BepInEx/`) part à la corbeille avec le reste, pas pour de bon.
    let (loaders, others): (Vec<_>, Vec<_>) = state.list(game)?.into_iter().partition(InstalledMod::is_loader);
    for m in others.iter().chain(&loaders) {
        deploy::undeploy(game_dir, &m.deployment, false);
        state.remove(game, &m.id)?;
    }
    let mods = loaders.len() + others.len();
    forget(state, paths, game)?;
    // Plus rien d'Unalia dans le jeu : tout ce qui reste vient d'ailleurs.
    let found: Vec<Foreign> = foreign::scan(loader, game_dir, &HashSet::new(), &HashSet::new())
        .into_iter()
        .filter(|f| remove.contains(&f.path))
        .collect();
    let left = foreign::remove(game_dir, &found, discard);
    Ok(Reset {
        mods,
        foreign: before
            .iter()
            .filter(|f| remove.contains(&f.path) && fs::symlink_metadata(game_dir.join(&f.path)).is_err())
            .count(),
        left,
    })
}

/// Place qu'occupe le stockage d'un jeu, en octets : chaque fichier compté
/// une fois, versions précédentes comprises.
pub fn storage_size(paths: &Paths, game: &str) -> u64 {
    paths.store().size_of(&game_segment(game))
}

/// Libère le stockage d'un mod désinstallé, une fois l'annulation passée :
/// ses paquets, ses versions précédentes et, si aucun autre jeu ne l'a, ses
/// archives en cache. Sans effet s'il a été réinstallé entre-temps.
pub fn purge(state: &mut State, paths: &Paths, game: &str, id: &str) -> Result<(), InstallError> {
    // L'identifiant devient un nom de dossier : un seul segment, rien qui
    // remonte.
    let mut parts = Path::new(id).components();
    if !matches!(
        (parts.next(), parts.next()),
        (Some(Component::Normal(_)), None)
    ) {
        return Ok(());
    }
    if state.get(game, id)?.is_some() {
        return Ok(());
    }
    state.forget_previous(game, id)?;
    paths.store().remove_packages(&mod_prefix(game, id))?;
    collect_garbage(state, paths)?;
    if !state.installed_anywhere(id)? {
        let prefix = format!("{id}-");
        if let Ok(entries) = fs::read_dir(&paths.downloads) {
            for path in entries.flatten().map(|e| e.path()) {
                let archive = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(&prefix) && n.ends_with(".zip"));
                if archive {
                    let _ = fs::remove_file(path);
                }
            }
        }
        discard_partial_downloads(paths, id);
    }
    Ok(())
}

/// Annule une désinstallation : remet le mod tel qu'il était.
pub fn restore(
    state: &mut State,
    paths: &Paths,
    game_dir: &Path,
    removed: InstalledMod,
) -> Result<InstalledMod, InstallError> {
    let mut m = removed;
    if m.enabled {
        m.deployment = put_up(&paths.store(), game_dir, &m)?;
    }
    state.save(&m)?;
    Ok(m)
}

/// Dossiers du stockage d'aujourd'hui ; tout autre dossier vient de
/// l'ancien stockage (un dossier par jeu, mod et version).
const STORE_DIRS: &[&str] = &["files", "packages", "tmp"];

/// Réglage : le stockage est au format des paquets.
const STORE_FORMAT: &str = "store_format";

/// Passe les mods de l'ancien stockage (un dossier extrait par version) au
/// stockage par empreinte, sans rien retélécharger, puis retire les restes
/// de l'ancien : versions oubliées, mods qui ne sont plus installés, et
/// archives en cache, devenues des doublons. Renvoie le nombre de mods
/// migrés. Sans effet la deuxième fois.
pub fn migrate_store(state: &mut State, paths: &Paths) -> Result<usize, InstallError> {
    if state.setting(STORE_FORMAT)?.as_deref() == Some("2") {
        return Ok(0);
    }
    let store = paths.store();
    let mut migrated = 0;
    let mut lost = 0;
    for game in state.games()? {
        for mut m in state.list(&game)? {
            if !m.package.is_empty() {
                continue;
            }
            let dir = PathBuf::from(&m.legacy_store_dir);
            if !dir.is_dir() {
                lost += 1;
                continue;
            }
            // Calculés avant de vider le dossier : ils en lisent la liste.
            if m.placements.is_empty() {
                m.placements = bepinex_placements(&m.id, m.loader_root.as_deref(), &list_files(&dir)?);
            }
            let version = dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let key = format!("{}/{}", mod_prefix(&game, &m.id), path_segment(&version));
            store.ingest(&key, &dir, &m.sha256)?;
            m.package = key;
            m.legacy_store_dir = String::new();
            state.save(&m)?;
            migrated += 1;
        }
    }
    // Un mod sans dossier (stockage effacé à la main) garde ses archives :
    // de quoi le réinstaller sans tout retélécharger.
    if lost == 0 {
        if let Ok(entries) = fs::read_dir(&paths.store) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let legacy = !STORE_DIRS.iter().any(|d| name == *d);
                if legacy && entry.file_type().is_ok_and(|t| t.is_dir()) {
                    let _ = fs::remove_dir_all(entry.path());
                }
            }
        }
        if let Ok(entries) = fs::read_dir(&paths.downloads) {
            for path in entries.flatten().map(|e| e.path()) {
                if path.extension().is_some_and(|e| e == "zip") {
                    let _ = fs::remove_file(path);
                }
            }
        }
        state.set_setting(STORE_FORMAT, Some("2"))?;
    }
    collect_garbage(state, paths)?;
    Ok(migrated)
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

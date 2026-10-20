//! Installation en un clic : enchaîne resolver → téléchargement →
//! extraction → placement → déploiement → état, pour tout le plan.
//!
//! Le plan entier est transactionnel (SPEC §2.2) : si une étape échoue, tout
//! ce qui a été posé pendant cette installation est retiré, les versions
//! remplacées sont remises en place, et rien n'est enregistré.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::HttpClient;
use crate::archive::{self, ArchiveError, Limits};
use crate::deploy::{self, DeployError, Deployment};
use crate::download::{self, DownloadError};
use crate::games::GameCatalog;
use crate::loaders::bepinex::BepInEx;
use crate::loaders::{Loader, PackageKind, Placement};
use crate::resolver::{self, Installed, PlanStep, ResolveError};
use crate::sources::ModSource;
use crate::state::{InstalledMod, State, StateError};

/// Dossiers de Modalia.
#[derive(Debug, Clone)]
pub struct Paths {
    /// Mods extraits, un dossier par jeu, mod et version.
    pub store: PathBuf,
    /// Archives téléchargées.
    pub downloads: PathBuf,
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
    #[error(transparent)]
    State(#[from] StateError),
    #[error("BepInEx is already installed in this game outside Modalia")]
    LoaderPresent,
    #[error("other mods still need the loader: remove them first")]
    LoaderInUse,
    #[error("the loader can't be turned off: turn off the mods instead")]
    LoaderToggle,
    #[error("this mod isn't installed")]
    NotInstalled,
    #[error("couldn't access Modalia's storage")]
    Io(#[from] std::io::Error),
}

pub struct Installer<'a, S> {
    pub source: &'a S,
    pub catalog: Option<&'a GameCatalog>,
    pub client: &'a HttpClient,
    pub paths: &'a Paths,
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
            let previous = existing.iter().find(|m| m.id == step.id).cloned();
            match self
                .install_step(
                    target,
                    step,
                    index,
                    total,
                    previous.as_ref(),
                    &mut on_progress,
                )
                .await
            {
                Ok(record) => done.push(Done { record, previous }),
                Err(error) => {
                    rollback(target.game_dir, &done);
                    return Err(error);
                }
            }
        }

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
            && BepInEx.installed_manually(target.game_dir)
        {
            return Err(InstallError::LoaderPresent);
        }
        Ok(plan)
    }

    async fn install_step(
        &self,
        target: &Target<'_>,
        step: &PlanStep,
        index: usize,
        total: usize,
        previous: Option<&InstalledMod>,
        on_progress: &mut impl FnMut(Progress),
    ) -> Result<InstalledMod, InstallError> {
        let name = step.name.clone();
        let version = &step.version.number;
        let (store_dir, sha256) = self
            .fetch_step(target, step, index, total, on_progress)
            .await?;

        // Une mise à jour retire d'abord l'ancienne version du jeu.
        if let Some(old) = previous {
            deploy::undeploy(target.game_dir, &old.deployment, false);
        }
        let loader_root = step.loader.then(|| self.loader_root(&step.id)).flatten();
        let placements = placements(&step.id, loader_root.as_deref(), &store_dir)?;
        let deployment = match deploy::deploy(&store_dir, target.game_dir, &placements) {
            Ok(d) => d,
            Err(source) => {
                if let Some(old) = previous {
                    redeploy(target.game_dir, old);
                }
                return Err(InstallError::Deploy { name, source });
            }
        };

        Ok(InstalledMod {
            game: target.game.to_owned(),
            id: step.id.clone(),
            name,
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
            loader_root,
            enabled: true,
            explicit: false,
            sha256,
            installed_at: now(),
            store_dir: store_dir.to_string_lossy().into_owned(),
            deployment,
        })
    }

    /// Télécharge et extrait un paquet dans le stockage, sans toucher au
    /// jeu. Renvoie le dossier extrait et l'empreinte de l'archive. Déjà
    /// fait pour cette version : instantané.
    async fn fetch_step(
        &self,
        target: &Target<'_>,
        step: &PlanStep,
        index: usize,
        total: usize,
        on_progress: &mut impl FnMut(Progress),
    ) -> Result<(PathBuf, String), InstallError> {
        let name = step.name.clone();
        let version = &step.version.number;

        // Télécharger (ou reprendre l'archive déjà en cache).
        let archive = self
            .paths
            .downloads
            .join(format!("{}-{version}.zip", step.id));
        let sha256 = if archive.is_file() {
            download::sha256_file(&archive)?
        } else {
            let options = download::Options {
                expected_sha256: step.version.sha256.clone(),
                ..download::Options::default()
            };
            download::download(
                self.client,
                &step.version.download_url,
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
            .map_err(|source| InstallError::Download {
                name: name.clone(),
                source,
            })?
            .sha256
        };

        on_progress(Progress::Installing {
            index,
            total,
            name: name.clone(),
        });

        // Extraire dans le stockage (une fois par version).
        let store_dir = self
            .paths
            .store
            .join(target.game.replace(':', "-"))
            .join(&step.id)
            .join(version);
        if !store_dir.is_dir() {
            if let Some(parent) = store_dir.parent() {
                fs::create_dir_all(parent)?;
            }
            // Sur un fil à part : extraire 10 Go prend des minutes, le fil
            // asynchrone doit rester libre. La progression revient par canal.
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let (from, to) = (archive.clone(), store_dir.clone());
            let job = tokio::task::spawn_blocking(move || {
                let mut last = 0;
                archive::extract_with_progress(&from, &to, Limits::default(), |done, size| {
                    // Un message par Mo suffit à l'affichage.
                    if done - last >= 1024 * 1024 || done == size {
                        last = done;
                        let _ = tx.send((done, size));
                    }
                })
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
            job.await
                .map_err(|e| InstallError::Io(std::io::Error::other(e)))?
                .map_err(|source| InstallError::Archive {
                    name: name.clone(),
                    source,
                })?;
        }

        Ok((store_dir, sha256))
    }

    fn loader_root(&self, id: &str) -> Option<String> {
        self.catalog?
            .loader_package(id)
            .map(|l| l.root_folder.clone())
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

fn rollback(game_dir: &Path, done: &[Done]) {
    for d in done.iter().rev() {
        deploy::undeploy(game_dir, &d.record.deployment, d.record.is_loader());
        if let Some(old) = &d.previous {
            redeploy(game_dir, old);
        }
    }
}

/// Remet en place une version déjà enregistrée (annulation d'une mise à
/// jour ratée). Au mieux : l'utilisateur pourra réinstaller sinon.
fn redeploy(game_dir: &Path, old: &InstalledMod) {
    let store = Path::new(&old.store_dir);
    if let Ok(placements) = placements(&old.id, old.loader_root.as_deref(), store) {
        let _ = deploy::deploy(store, game_dir, &placements);
    }
}

/// Où vont les fichiers d'un paquet extrait.
fn placements(
    id: &str,
    loader_root: Option<&str>,
    store_dir: &Path,
) -> std::io::Result<Vec<Placement>> {
    let kind = match loader_root {
        Some(root) => PackageKind::Loader {
            root_folder: root.to_owned(),
        },
        None => PackageKind::Mod,
    };
    Ok(BepInEx.place(id, &kind, &list_files(store_dir)?))
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
    if enabled {
        let store = Path::new(&m.store_dir);
        m.deployment =
            deploy::deploy(store, game_dir, &placements(id, None, store)?).map_err(|source| {
                InstallError::Deploy {
                    name: m.name.clone(),
                    source,
                }
            })?;
    } else {
        deploy::undeploy(game_dir, &m.deployment, false);
        m.deployment = Deployment::default();
    }
    m.enabled = enabled;
    state.save(&m)?;
    Ok(m)
}

/// Désinstalle un mod. Le loader ne part qu'en dernier, avec tout ce que
/// BepInEx a écrit dans ses dossiers : le jeu retrouve son état d'origine.
/// Le stockage est conservé pour permettre l'annulation (`restore`).
pub fn uninstall(
    state: &mut State,
    game: &str,
    game_dir: &Path,
    id: &str,
) -> Result<InstalledMod, InstallError> {
    let m = state.get(game, id)?.ok_or(InstallError::NotInstalled)?;
    if m.is_loader() && state.list(game)?.iter().any(|other| other.id != m.id) {
        return Err(InstallError::LoaderInUse);
    }
    deploy::undeploy(game_dir, &m.deployment, m.is_loader());
    state.remove(game, id)?;
    Ok(m)
}

/// Annule une désinstallation : remet le mod tel qu'il était.
pub fn restore(
    state: &mut State,
    game_dir: &Path,
    removed: InstalledMod,
) -> Result<InstalledMod, InstallError> {
    let mut m = removed;
    if m.enabled {
        let store = Path::new(&m.store_dir);
        m.deployment = deploy::deploy(
            store,
            game_dir,
            &placements(&m.id, m.loader_root.as_deref(), store)?,
        )
        .map_err(|source| InstallError::Deploy {
            name: m.name.clone(),
            source,
        })?;
    }
    state.save(&m)?;
    Ok(m)
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

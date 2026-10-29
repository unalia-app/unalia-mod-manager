//! One-click install: chains resolver, download, extraction, placement,
//! deployment and state, for the whole plan.
//!
//! The whole plan is transactional: if a step fails, everything placed
//! during this install is removed, the replaced versions are put back, and
//! nothing is recorded.

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
    /// Mod storage (`store::Store`): each file only once, and one package
    /// per extracted version.
    pub store: PathBuf,
    /// Downloaded archives, removed once extracted (partial downloads
    /// included, to resume).
    pub downloads: PathBuf,
}

impl Paths {
    pub fn store(&self) -> Store {
        Store::new(&self.store)
    }
}

/// The game we install into.
#[derive(Debug, Clone)]
pub struct Target<'a> {
    /// Game key in the state, e.g. `steam:1030300`.
    pub game: &'a str,
    pub game_dir: &'a Path,
    /// Game id for the source (Thunderstore community).
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
    /// Extraction: `done` / `size` in uncompressed bytes.
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
    /// FOMOD installer: options to pick, not offered yet.
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
    /// Nexus mod with several files (variants, options): the user picks,
    /// the advised file first.
    #[error("choose which files of this mod to install")]
    ChooseFiles(Vec<crate::sources::nexus::ModFile>),
    /// Free account: this file downloads from the Nexus website.
    #[error("this file downloads from the Nexus Mods website")]
    NexusWebsite { file_id: u32 },
    #[error("the install was cancelled")]
    Cancelled,
}

/// A package ready to download, whatever its source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    /// Simultaneous connections allowed by the source (free Nexus: a single
    /// one, like Vortex). `None`: the default value.
    pub connections: Option<usize>,
    pub id: String,
    pub name: String,
    pub author: String,
    pub version: String,
    pub icon_url: Option<String>,
    pub download_url: String,
    pub sha256: Option<String>,
    /// Precise file of a Nexus mod: a mod has several (variants) with the
    /// same version, each one has its own place in the cache.
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
            icon_url: step.icon_url.clone().or_else(|| {
                Some(format!(
                    "https://gcdn.thunderstore.io/live/repository/icons/{}-{version}.png",
                    step.id
                ))
            }),
            download_url: step.version.download_url.clone(),
            sha256: step.version.sha256.clone(),
            file_id: None,
        }
    }
}

/// Downloaded and stored package, with the destination of its files.
struct Fetched {
    package: Package,
    /// Package key in the storage.
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
    /// Cancel requested by the user: honored during downloads and
    /// extractions, never in the middle of a deployment.
    pub cancel: Option<&'a Cancel>,
}

impl<S: ModSource> Installer<'_, S> {
    /// Installs `mod_id` and its dependencies. Returns the game's mods after
    /// the install.
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
                Ok(mut record) => {
                    record.source = step.source.to_owned();
                    done.push(Done { record, previous })
                }
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
                // A mod already there keeps its status (an updated
                // dependency stays a dependency); a new one is explicit if
                // it is the one the user asked for.
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

    /// Prepares the install of `mod_id` without touching the game or the
    /// state: the whole plan is downloaded and extracted. Used during a
    /// session: when the game closes, the install only has to place the
    /// links. Returns the number of prepared packages.
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
        // Last chance to cancel: after this, the game is modified.
        self.check_cancel()?;
        deploy_package(&self.paths.store(), target, fetched, previous, existing)
    }

    /// For a package already cached, no network access.
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

    /// Key of a version in the storage.
    fn package_key(&self, game: &str, package: &Package) -> String {
        package_key(game, &package.id, &package.version, package.file_id)
    }

    /// Is this version already stored, or its archive downloaded?
    fn cached(&self, game: &str, id: &str, version: &str, file_id: Option<u32>) -> bool {
        self.paths
            .store()
            .has(&package_key(game, id, version, file_id))
            || self.archive_path(id, version, file_id).is_file()
    }

    /// Downloads a package and stores its files, without touching the game.
    /// Returns its key and its content. Already done for this version:
    /// instant.
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

        // Download (or reuse the archive already cached). The extension
        // doesn't matter: the format is read from the file.
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
                // Cancel means give up: the partial file goes too.
                // (Quitting the app keeps enough to resume.)
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

        // Extract next to the storage, then store each file. On a separate
        // thread: extracting and hashing 10 GB takes minutes, the async
        // thread must stay free. Progress comes back through a channel.
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
                    // One message per MB is enough for the display.
                    if done - last >= 1024 * 1024 || done == size {
                        last = done;
                        let _ = tx.send((done, size));
                    }
                },
            )?;
            // Storage error (disk full…): the archive itself is fine and
            // stays cached.
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
                // Everything is stored: the archive would be a duplicate.
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
                // Unreadable archive: remove it from the cache so that a new
                // try downloads it again.
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

    /// Archive of a version in the download cache.
    fn archive_path(&self, id: &str, version: &str, file_id: Option<u32>) -> PathBuf {
        let file = file_id.map(|f| format!("-f{f}")).unwrap_or_default();
        self.paths
            .downloads
            .join(format!("{id}-{}{file}.zip", path_segment(version)))
    }

    /// The game's adapter: its placement rules if the catalog knows them,
    /// else BepInEx (catalog unavailable, first launch offline).
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

/// Places a stored package into the game and returns its record. An update
/// first removes the old version, whose edited configs are kept in
/// `previous`; on failure, it is put back.
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
    // Same version reinstalled: the edited configs stay.
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
        source: String::new(),
        deployment,
        placements,
        edits,
        previous_versions: Vec::new(),
    })
}

/// Content of a recorded mod, from the storage.
fn manifest_of(store: &Store, m: &InstalledMod) -> Result<Manifest, InstallError> {
    store.manifest(&m.package).map_err(|_| InstallError::Deploy {
        name: m.name.clone(),
        source: DeployError::MissingSource(m.package.clone()),
    })
}

/// Places a recorded mod, with its edited configs.
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

/// Removes a mod from the game, first keeping in `m.edits` the configs the
/// player edited there. Returns what couldn't be removed.
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

/// The files of a package, in order.
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

/// Folder of a mod's packages in the storage (`<game>/<mod>`).
fn mod_prefix(game: &str, id: &str) -> String {
    format!("{}/{}", game_segment(game), path_segment(id))
}

fn game_segment(game: &str) -> String {
    path_segment(&game.replace(':', "-"))
}

/// Does a file this package wants to place already belong to another mod?
/// Returns the path and that mod's name, for a message that says which.
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

/// Removes a partial download and its resume journal.
fn discard_partial(archive: &Path) {
    let _ = fs::remove_file(archive.with_extension("part"));
    let _ = fs::remove_file(archive.with_extension("part.json"));
}

/// Partial downloads of a mod (all versions) left in the cache, e.g. after
/// quitting the app: enough to resume, or to throw away. Names:
/// `<id>-<version>.part` and its journal `.part.json`.
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

/// Throws away a mod's partial downloads.
pub fn discard_partial_downloads(paths: &Paths, id: &str) {
    for file in partial_downloads(paths, id) {
        let _ = fs::remove_file(file);
    }
}

/// Safe path segment taken from a version (`1.2/beta` gives `1.2_beta`).
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

/// What the resolver needs to know about the mods already in the game.
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

/// Versions replaced by an install (reinstalling the same version replaces
/// nothing).
fn replaced(done: &[Done]) -> Vec<InstalledMod> {
    done.iter()
        .filter_map(|d| d.previous.clone().filter(|p| p.package != d.record.package))
        .collect()
}

/// Keeps the replaced versions to go back to them, then cleans up the
/// storage.
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

/// Storage cleanup: removes what no installed mod, no kept version and no
/// edited config uses.
pub fn collect_garbage(state: &State, paths: &Paths) -> Result<Freed, InstallError> {
    let (packages, blobs) = state.referenced()?;
    Ok(paths.store().gc(&packages, &blobs)?)
}

/// Puts back a version already recorded (undo of a failed update). Best
/// effort: the user can reinstall otherwise.
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

/// Placements recorded at install time; for a mod installed before they
/// were added, computed again with the BepInEx rules of the time.
fn placements_of(m: &InstalledMod, manifest: &Manifest) -> Vec<Placement> {
    if !m.placements.is_empty() {
        return m.placements.clone();
    }
    bepinex_placements(&m.id, m.loader_root.as_deref(), &files_of(manifest))
}

/// Where a package's files go, by the BepInEx rules.
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

/// Update available for an installed mod.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Update {
    pub mod_id: String,
    pub current: String,
    pub latest: String,
}

/// Updates available among `installed`. Nothing is installed: we tell, the
/// user confirms. A mod the source no longer finds, or an unreachable
/// source, doesn't prevent checking the others.
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

/// Enables or disables a mod: its files leave the game (or come back), the
/// storage keeps them.
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

/// Uninstalls a mod. The loader only goes last, with everything BepInEx
/// wrote in its folders: the game gets its original state back. The
/// storage is kept to allow undo (`restore`), then freed by `purge`.
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

/// Removes every mod of a game, loader last: the game gets its original
/// state back. Used when Unalia is uninstalled, when its database is about
/// to disappear. Returns the number of removed mods.
pub fn uninstall_all(state: &mut State, game: &str, game_dir: &Path) -> Result<usize, InstallError> {
    let (loaders, mods): (Vec<_>, Vec<_>) = state.list(game)?.into_iter().partition(InstalledMod::is_loader);
    let count = loaders.len() + mods.len();
    for m in mods.iter().chain(&loaders) {
        deploy::undeploy(game_dir, &m.deployment, m.is_loader());
        state.remove(game, &m.id)?;
    }
    Ok(count)
}

/// Bethesda games: `plugins.txt` (`list`) follows the plugins of the
/// enabled mods, after each change. Returns `true` if it changed.
pub fn sync_plugins(state: &State, game: &str, game_dir: &Path, list: &Path) -> Result<bool, InstallError> {
    let deployed: Vec<String> = state
        .list(game)?
        .into_iter()
        .filter(|m| m.enabled)
        .flat_map(|m| m.deployment.files)
        .collect();
    Ok(crate::loaders::bethesda::sync_plugins(game_dir, list, &deployed)?)
}

/// Enabled mods with at least one file gone from the game: game
/// reinstalled, files verified by Steam, or deleted by hand. Loader first.
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

/// Places a mod again from the storage, downloading nothing again. What was
/// left of it in the game is removed first.
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

/// Goes back to a previous version of a mod, kept in the storage: nothing
/// is downloaded again. The version left becomes a previous version in
/// turn. A disabled mod changes version without coming back into the
/// game.
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

/// Forgets an uninstalled game: its mods leave the database and their
/// storage is freed. Nothing is removed from the game: its folder is gone.
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

/// Removes a game from Unalia: its mods leave its folder, which gets its
/// original state back (`game_dir` missing: the folder is gone), then the
/// database and the storage. The game itself is not touched. Returns the
/// number of removed mods.
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

/// Mod files placed in the game outside Unalia (see `foreign`). `loader`:
/// the one from the game profile.
pub fn foreign_files(state: &State, game: &str, game_dir: &Path, loader: &str) -> Result<Vec<Foreign>, InstallError> {
    let (mut files, mut dirs) = (HashSet::new(), HashSet::new());
    for m in state.list(game)? {
        files.extend(m.deployment.files);
        dirs.extend(m.deployment.created_dirs);
    }
    Ok(foreign::scan(loader, game_dir, &files, &dirs))
}

/// Result of a reset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reset {
    /// Unalia mods removed.
    pub mods: usize,
    /// Locations left by other tools, removed.
    pub foreign: usize,
    /// The ones that couldn't go.
    pub left: Vec<String>,
}

/// Resets the game's mods: Unalia's leave it and their storage is freed,
/// then what other tools left goes through `discard` (the Recycle Bin), but
/// only the locations in `remove`, ticked by the user (see
/// [`Foreign::sure`]). The rest and the game's files are not touched.
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
    // Like `uninstall_all`, but the loader folders are not force-emptied:
    // what was slipped in there by hand (a plugin in `BepInEx/`) goes to
    // the Recycle Bin with the rest, not for good.
    let (loaders, others): (Vec<_>, Vec<_>) = state.list(game)?.into_iter().partition(InstalledMod::is_loader);
    for m in others.iter().chain(&loaders) {
        deploy::undeploy(game_dir, &m.deployment, false);
        state.remove(game, &m.id)?;
    }
    let mods = loaders.len() + others.len();
    forget(state, paths, game)?;
    // Nothing from Unalia left in the game: whatever remains comes from elsewhere.
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

/// Space taken by a game's storage, in bytes: each file counted once,
/// previous versions included.
pub fn storage_size(paths: &Paths, game: &str) -> u64 {
    paths.store().size_of(&game_segment(game))
}

/// Frees the storage of an uninstalled mod, once the undo window passed:
/// its packages, its previous versions and, if no other game has it, its
/// cached archives. No effect if it was reinstalled in the meantime.
pub fn purge(state: &mut State, paths: &Paths, game: &str, id: &str) -> Result<(), InstallError> {
    // The id becomes a folder name: a single segment, nothing that goes
    // up.
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

/// Undoes an uninstall: puts the mod back as it was.
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

/// Folders of today's storage; any other folder comes from the old storage
/// (one folder per game, mod and version).
const STORE_DIRS: &[&str] = &["files", "packages", "tmp"];

/// Setting: the storage uses the package format.
const STORE_FORMAT: &str = "store_format";

/// Moves the mods of the old storage (one extracted folder per version) to
/// the hash-based storage, downloading nothing again, then removes the old
/// one's leftovers: forgotten versions, mods no longer installed, and
/// cached archives, now duplicates. Returns the number of migrated mods.
/// No effect the second time.
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
            // Computed before emptying the folder: they read its listing.
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
    // A mod without a folder (storage deleted by hand) keeps its archives:
    // enough to reinstall it without downloading everything again.
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

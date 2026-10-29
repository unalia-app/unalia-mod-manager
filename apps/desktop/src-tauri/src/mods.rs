//! Install and launch commands: wire the engine to the UI, with progress
//! and game state pushed through events.
//!
//! While a game runs, any change to its mods is queued and applied when it
//! closes.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex as SyncMutex;
use std::time::{Duration, Instant};

use mod_engine::download::Cancel;
use mod_engine::foreign::{self, Foreign};
use mod_engine::installer::{
    self, InstallError, Installer, NexusFile, NexusInstall, Paths, Progress, Target, Update,
};
use mod_engine::launch::{self, LaunchTarget, PendingOp, ProcessWatcher};
use mod_engine::sources::SourceError;
use mod_engine::sources::nexus::{self as nexus_source, Account, NxmLink, Requirement};
use mod_engine::state::{InstalledMod, ManualGame, State as ModState};
use serde::{Deserialize, Serialize};
use tauri::async_runtime::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_opener::OpenerExt;

use crate::catalog::Catalog;
use crate::{KnownGame, Library};

/// How long a launched game stays "starting" before it shows up among the
/// processes (Steam can take a while to launch it).
const STARTING_FOR: Duration = Duration::from_secs(60);
/// How often the process watch runs.
const WATCH_EVERY: Duration = Duration::from_secs(2);
/// How long a removal can be undone: the mod's storage is only freed after.
/// Much longer than the "Undo" toast, which pauses on hover.
const UNDO_FOR: Duration = Duration::from_secs(120);

pub struct Mods {
    /// One install at a time: it holds the database for its whole duration,
    /// the next ones wait their turn (queue).
    state: Mutex<ModState>,
    paths: Paths,
    /// Mods just uninstalled, kept for "Undo" with the time of removal.
    /// Engine side: the webview only sends back an id, never a path.
    removed: SyncMutex<HashMap<(String, String), (Instant, InstalledMod)>>,
    processes: SyncMutex<ProcessWatcher>,
    /// Games seen running at the last pass of the watch.
    running: SyncMutex<HashSet<u32>>,
    /// Games launched from Modalia, not visible among the processes yet.
    starting: SyncMutex<HashMap<u32, Starting>>,
    /// Downloads and extractions: one at a time, so that prefetching and an
    /// install don't write the same archive.
    fetching: Mutex<()>,
    /// Pending installs already downloaded and extracted: when the game
    /// closes, only the links remain to be placed.
    ready: SyncMutex<HashSet<(u32, String)>>,
    /// Cancel flag of each install running or queued.
    cancels: SyncMutex<HashMap<(u32, String), Cancel>>,
    /// Downloads running or interrupted (see `Paused`).
    paused: SyncMutex<Paused>,
    /// Picked files of a Nexus mod waiting for their `nxm://` link (free
    /// account: one click on the site per file).
    chosen_files: SyncMutex<HashMap<(u32, String), Vec<u32>>>,
}

impl Mods {
    pub(crate) fn state(&self) -> &Mutex<ModState> {
        &self.state
    }

    pub fn open(app: &AppHandle) -> Result<Self, Box<dyn std::error::Error>> {
        // Local data (`%LOCALAPPDATA%` on Windows), never the roaming
        // profile: hundreds of MB of mods don't need to sync between
        // machines.
        let data = app.path().app_local_data_dir()?;
        let cache = app.path().app_cache_dir()?;
        let roaming = app.path().app_data_dir()?;
        // Before anything is opened: on Windows, an open file can't be
        // replaced.
        for dir in [&roaming, &data, &cache] {
            adopt_old_dir(dir);
        }
        migrate_from_roaming(&roaming, &data);
        let mut state = ModState::open(&data.join("modalia.db"))?;
        if let Some(base) = data.parent() {
            state.rebase_store(&base.join(OLD_IDENTIFIER).join("mods"), &data.join("mods"))?;
        }
        Ok(Self {
            state: Mutex::new(state),
            paths: Paths {
                store: data.join("mods"),
                downloads: cache.join("downloads"),
            },
            removed: SyncMutex::default(),
            processes: SyncMutex::default(),
            running: SyncMutex::default(),
            starting: SyncMutex::default(),
            fetching: Mutex::new(()),
            ready: SyncMutex::default(),
            cancels: SyncMutex::default(),
            paused: SyncMutex::new(Paused::load(data.join("downloads.json"))),
            chosen_files: SyncMutex::default(),
        })
    }

    /// Executables of the running processes.
    pub(crate) fn running_exes(&self) -> Vec<std::path::PathBuf> {
        self.processes.lock().unwrap().running_exes()
    }

    /// Is the game running right now? Checked on demand, without waiting
    /// for the next pass of the watch.
    fn is_running(&self, game: &KnownGame) -> bool {
        let exes = self.processes.lock().unwrap().running_exes();
        launch::game_running(&game.dir, &exes, &game.profile.launchers)
    }

    /// Start of an install: its cancel flag.
    fn begin(&self, app_id: u32, mod_id: &str) -> Cancel {
        let cancel = Cancel::default();
        self.cancels
            .lock()
            .unwrap()
            .insert((app_id, mod_id.to_owned()), cancel.clone());
        cancel
    }

    fn end(&self, app_id: u32, mod_id: &str) {
        self.cancels
            .lock()
            .unwrap()
            .remove(&(app_id, mod_id.to_owned()));
    }

    /// Why the app must not close now (update): an install running or
    /// queued, or a game running.
    pub fn busy(&self) -> Option<&'static str> {
        if !self.cancels.lock().unwrap().is_empty() {
            Some("wait for your mods to finish installing, then try again")
        } else if !self.running.lock().unwrap().is_empty() {
            Some("close your game first, then try again")
        } else {
            None
        }
    }

    fn is_starting(&self, app_id: u32) -> bool {
        self.starting
            .lock()
            .unwrap()
            .get(&app_id)
            .is_some_and(|s| s.launcher_open || s.at.elapsed() < STARTING_FOR)
    }
}

/// Launch in progress: since when, and whether the game's launcher is open.
#[derive(Clone, Copy)]
struct Starting {
    at: Instant,
    /// Launcher seen open during this launch.
    saw_launcher: bool,
    /// Launcher open at the last pass of the watch: we wait without a limit
    /// for the user to launch the game.
    launcher_open: bool,
}

impl Starting {
    fn new() -> Self {
        Self {
            at: Instant::now(),
            saw_launcher: false,
            launcher_open: false,
        }
    }

    /// Is the launch over (game appeared) or abandoned (launcher closed
    /// without playing, or nothing after the delay)?
    fn over(&self, game_running: bool) -> bool {
        game_running
            || (self.saw_launcher && !self.launcher_open)
            || (!self.launcher_open && self.at.elapsed() >= STARTING_FOR)
    }
}

/// Identifiant de l'appli avant son nom public, Unalia Mod Manager.
const OLD_IDENTIFIER: &str = "app.modalia.desktop";

/// Takes over the folder of the old id, next to `dir`, as long as `dir`
/// doesn't exist yet. A plain rename: hard links into the games stay valid,
/// and the paths recorded in the database are rewritten afterwards
/// (`rebase_store`).
pub(crate) fn adopt_old_dir(dir: &Path) {
    let Some(old) = dir.parent().map(|base| base.join(OLD_IDENTIFIER)) else {
        return;
    };
    if old == dir || !old.is_dir() || dir.exists() {
        return;
    }
    if let Err(error) = std::fs::rename(&old, dir) {
        log::warn!("couldn't move {} to {}: {error}", old.display(), dir.display());
    }
}

/// The first versions kept the database and the mods in `%APPDATA%`
/// (roaming). We move them once to the local folder, or the mods already
/// placed in the games would become orphan files. On macOS and Linux, both
/// folders are the same: nothing to do.
fn migrate_from_roaming(roaming: &Path, local: &Path) {
    let old_db = roaming.join("modalia.db");
    let new_db = local.join("modalia.db");
    if roaming == local || !old_db.is_file() || new_db.exists() {
        return;
    }
    let old_store = roaming.join("mods");
    let new_store = local.join("mods");
    let moved = std::fs::create_dir_all(local).is_ok()
        && (!old_store.exists() || std::fs::rename(&old_store, &new_store).is_ok())
        && std::fs::rename(&old_db, &new_db).is_ok();
    if !moved {
        log::warn!("couldn't move Unalia's data out of the roaming profile");
        return;
    }
    for suffix in ["-wal", "-shm"] {
        let _ = std::fs::rename(
            roaming.join(format!("modalia.db{suffix}")),
            local.join(format!("modalia.db{suffix}")),
        );
    }
    if let Ok(mut state) = ModState::open(&new_db) {
        let _ = state.rebase_store(&old_store, &new_store);
    }
}

fn game_key(app_id: u32) -> String {
    format!("steam:{app_id}")
}

/// Removes the mods from every game (Unalia uninstalled, data deleted). A
/// game that can't be found is skipped: it has no files left to clean.
pub fn remove_all(data: &Path, steam_dirs: HashMap<u32, PathBuf>) {
    let db = data.join("modalia.db");
    if !db.is_file() {
        return;
    }
    let Ok(mut state) = ModState::open(&db) else {
        return;
    };
    // Like the library: Steam wins over a manual add.
    let mut game_dirs: HashMap<u32, PathBuf> = state
        .manual_games()
        .unwrap_or_default()
        .into_iter()
        .map(|game| (game.app_id, PathBuf::from(game.path)))
        .collect();
    game_dirs.extend(steam_dirs);
    for key in state.games().unwrap_or_default() {
        let dir = key
            .strip_prefix("steam:")
            .and_then(|id| id.parse::<u32>().ok())
            .and_then(|id| game_dirs.get(&id));
        if let Some(dir) = dir
            && let Err(error) = installer::uninstall_all(&mut state, &key, dir)
        {
            eprintln!("couldn't remove the mods of {key}: {error}");
        }
    }
}

const UNKNOWN_GAME: &str = "This game isn't in your library anymore. Go back home to rescan it.";

/// Result of an action on mods: done right away, or queued because the game
/// is running.
#[derive(Serialize)]
#[serde(rename_all = "camelCase", tag = "status")]
pub enum Outcome<T> {
    Done {
        value: T,
    },
    Queued,
    /// The site must give its consent (Nexus, free account): the UI opens
    /// `url`, the `nxm://` link of the "Mod Manager Download" button comes
    /// back to Modalia, which finishes the install.
    Browser {
        url: String,
    },
    /// Canceled by the user: nothing changed in the game.
    Cancelled,
    /// Nexus mod with several files (variants, options): the UI shows them,
    /// the install resumes with the picked ones.
    Choose {
        files: Vec<nexus_source::ModFile>,
    },
}

// ---------------------------------------------------------------- state

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingInfo {
    #[serde(flatten)]
    op: PendingOp,
    /// Name to display (installed name, else taken from the id).
    name: String,
    /// Install already downloaded during the session.
    ready: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameStatus {
    app_id: u32,
    running: bool,
    starting: bool,
    pending: Vec<PendingInfo>,
}

async fn status(app: &AppHandle, app_id: u32) -> GameStatus {
    let mods = app.state::<Mods>();
    let running = mods.running.lock().unwrap().contains(&app_id);
    let key = game_key(app_id);
    let state = mods.state.lock().await;
    let pending = state
        .pending(&key)
        .unwrap_or_default()
        .into_iter()
        .map(|op| {
            let name = state
                .get(&key, op.mod_id())
                .ok()
                .flatten()
                .map(|m| m.name)
                .unwrap_or_else(|| display_name(op.mod_id()));
            let ready = mods
                .ready
                .lock()
                .unwrap()
                .contains(&(app_id, op.mod_id().to_owned()));
            PendingInfo { op, name, ready }
        })
        .collect();
    GameStatus {
        app_id,
        starting: !running && mods.is_starting(app_id),
        running,
        pending,
    }
}

async fn emit_status(app: &AppHandle, app_id: u32) {
    let status = status(app, app_id).await;
    let _ = app.emit("game-status", status);
}

/// `Author-Cool_Mod` → `Cool Mod`.
fn display_name(id: &str) -> String {
    id.split_once('-').map_or(id, |(_, n)| n).replace('_', " ")
}

#[tauri::command]
pub async fn game_status(app: AppHandle, app_id: u32) -> Result<GameStatus, String> {
    Ok(status(&app, app_id).await)
}

/// Queues `op`: the game is running.
async fn queue(app: &AppHandle, app_id: u32, op: PendingOp) -> Result<(), String> {
    let mods = app.state::<Mods>();
    let key = game_key(app_id);
    {
        let mut state = mods.state.lock().await;
        let next = launch::enqueue(&state.pending(&key).map_err(crate::support::logged)?, op);
        state.set_pending(&key, &next).map_err(crate::support::logged)?;
    }
    // The game runs: the watch must know it to apply the queue when it
    // closes, even if it was launched outside Modalia.
    mods.running.lock().unwrap().insert(app_id);
    emit_status(app, app_id).await;
    Ok(())
}

/// Removes the pending change for this mod from the queue.
#[tauri::command]
pub async fn cancel_pending(app: AppHandle, app_id: u32, mod_id: String) -> Result<(), String> {
    {
        let mods = app.state::<Mods>();
        let key = game_key(app_id);
        let mut state = mods.state.lock().await;
        let next: Vec<_> = state
            .pending(&key)
            .map_err(crate::support::logged)?
            .into_iter()
            .filter(|op| op.mod_id() != mod_id)
            .collect();
        state.set_pending(&key, &next).map_err(crate::support::logged)?;
        mods.ready.lock().unwrap().remove(&(app_id, mod_id));
    }
    emit_status(&app, app_id).await;
    Ok(())
}

// ---------------------------------------------------------------- mods

#[tauri::command]
pub async fn installed_mods(
    mods: State<'_, Mods>,
    app_id: u32,
) -> Result<Vec<InstalledMod>, String> {
    mods.state
        .lock()
        .await
        .list(&game_key(app_id))
        .map_err(crate::support::logged)
}

/// Updates available for the game's mods. Nothing is installed: the user
/// picks. Updating = `install_mod`.
#[tauri::command]
pub async fn check_updates(
    library: State<'_, Library>,
    catalog: State<'_, Catalog>,
    mods: State<'_, Mods>,
    app_id: u32,
) -> Result<Vec<Update>, String> {
    let game = library.game(app_id).ok_or(UNKNOWN_GAME)?;
    // The database is not held during network calls.
    let installed = mods
        .state
        .lock()
        .await
        .list(&game_key(app_id))
        .map_err(crate::support::logged)?;
    if game.profile.nexus.is_some() {
        return Ok(installer::available_updates_nexus(
            &catalog.nexus,
            &catalog.github,
            &game.profile,
            &installed,
            game.mods_before(),
        )
        .await);
    }
    // Each mod is checked on the source it came from.
    let (hexium, thunderstore): (Vec<InstalledMod>, Vec<InstalledMod>) =
        installed.into_iter().partition(|m| m.source == "hexium");
    let (mut updates, from_hexium) = tokio::join!(
        installer::available_updates(&catalog.thunderstore, &game.community, &thunderstore),
        installer::available_updates(&catalog.hexium, &game.community, &hexium),
    );
    updates.extend(from_hexium);
    Ok(updates)
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallEvent {
    app_id: u32,
    mod_id: String,
    progress: Progress,
}

/// Installs a mod and its dependencies, or the install is queued if the
/// game is running. Progress comes through the `install-progress`
/// event.
#[tauri::command]
pub async fn install_mod(
    app: AppHandle,
    library: State<'_, Library>,
    mods: State<'_, Mods>,
    app_id: u32,
    mod_id: String,
    files: Option<Vec<u32>>,
    source: Option<String>,
) -> Result<Outcome<Vec<InstalledMod>>, String> {
    let game = library.game(app_id).ok_or(UNKNOWN_GAME)?;
    if game.profile.nexus.is_some() {
        if mods.is_running(&game) {
            return Err(close_first(&game));
        }
        let file = match &files {
            Some(ids) => NexusFile::Files { ids, link: None },
            None => NexusFile::Main,
        };
        let pending = (app_id, mod_id.clone());
        return match run_nexus_install(&app, app_id, &game, &mod_id, file).await {
            Ok(done) => {
                mods.chosen_files.lock().unwrap().remove(&pending);
                Ok(Outcome::Done { value: done.mods })
            }
            Err(NexusFailure::Website(url) | NexusFailure::NextFile(url)) => {
                // The site's `nxm://` links will resume with this choice.
                if let Some(ids) = files {
                    mods.chosen_files.lock().unwrap().insert(pending, ids);
                }
                Ok(Outcome::Browser { url })
            }
            Err(NexusFailure::Choose(files)) => Ok(Outcome::Choose { files }),
            Err(NexusFailure::Cancelled) => Ok(Outcome::Cancelled),
            Err(NexusFailure::Error(message)) => Err(message),
        };
    }
    // A new mod comes from the source picked in Discover, an update from
    // the one it was installed from.
    let source = match source {
        Some(source) => Some(source),
        None => mods
            .state
            .lock()
            .await
            .get(&game_key(app_id), &mod_id)
            .map_err(crate::support::logged)?
            .map(|m| m.source)
            .filter(|s| !s.is_empty()),
    };
    if mods.is_running(&game) {
        queue(
            &app,
            app_id,
            PendingOp::Install {
                mod_id: mod_id.clone(),
                source: source.clone(),
            },
        )
        .await?;
        // The game runs, but nothing prevents downloading: only the
        // deployment waits for it to close.
        tauri::async_runtime::spawn(prefetch(app.clone(), app_id, game, mod_id, source));
        return Ok(Outcome::Queued);
    }
    match run_install(&app, app_id, &game, &mod_id, source.as_deref()).await {
        Ok(value) => Ok(Outcome::Done { value }),
        Err(InstallError::Cancelled) => Ok(Outcome::Cancelled),
        Err(error) => Err(error_message(&error)),
    }
}

/// Downloads and extracts a queued install in the background. A failure
/// here is no big deal: the install tries again when the game closes and
/// reports the error then.
async fn prefetch(
    app: AppHandle,
    app_id: u32,
    game: KnownGame,
    mod_id: String,
    source: Option<String>,
) {
    let catalog = app.state::<Catalog>();
    let mods = app.state::<Mods>();
    let games_catalog = catalog.games(&app).await;
    let key = game_key(app_id);
    // List of installed mods, without holding the database during the download.
    let existing = mods.state.lock().await.list(&key).unwrap_or_default();
    let installer = Installer {
        source: &catalog.repository(source.as_deref()),
        catalog: games_catalog.as_deref(),
        client: &catalog.client,
        paths: &mods.paths,
        cancel: None,
    };
    let target = Target {
        game: &key,
        game_dir: &game.dir,
        community: &game.community,
    };
    let _fetching = mods.fetching.lock().await;
    if installer
        .prefetch(&existing, &target, &mod_id)
        .await
        .is_ok()
    {
        mods.ready.lock().unwrap().insert((app_id, mod_id));
        emit_status(&app, app_id).await;
    }
}

async fn run_install(
    app: &AppHandle,
    app_id: u32,
    game: &KnownGame,
    mod_id: &str,
    source: Option<&str>,
) -> Result<Vec<InstalledMod>, InstallError> {
    let catalog = app.state::<Catalog>();
    let mods = app.state::<Mods>();
    let games_catalog = catalog.games(app).await;
    // Before waiting its turn: a queued install can be canceled.
    let cancel = mods.begin(app_id, mod_id);
    let installer = Installer {
        source: &catalog.repository(source),
        catalog: games_catalog.as_deref(),
        client: &catalog.client,
        paths: &mods.paths,
        cancel: Some(&cancel),
    };
    let key = game_key(app_id);
    let target = Target {
        game: &key,
        game_dir: &game.dir,
        community: &game.community,
    };
    let result = {
        let _fetching = mods.fetching.lock().await;
        let mut state = mods.state.lock().await;
        installer
            .install(
                &mut state,
                &target,
                mod_id,
                progress_emitter(app, app_id, mod_id),
            )
            .await
    };
    mods.end(app_id, mod_id);
    settle_download(&mods, app_id, mod_id, result.as_ref().err());
    result
}

/// Progress events, no more than one per 1%.
fn progress_emitter<'a>(
    app: &'a AppHandle,
    app_id: u32,
    mod_id: &'a str,
) -> impl FnMut(Progress) + 'a {
    let mut last = None;
    move |progress| {
        let step = match &progress {
            Progress::Downloading {
                index,
                received,
                size,
                ..
            } => Some((
                *index,
                0,
                size.map(|s| received * 100 / s.max(1))
                    .unwrap_or(*received >> 16),
            )),
            Progress::Extracting {
                index, done, size, ..
            } => Some((*index, 1, done * 100 / (*size).max(1))),
            _ => None,
        };
        if step.is_some() && step == last {
            return;
        }
        last = step;
        // Written to disk: if the app is closed, the download shows up as
        // "paused" at the next launch.
        if let Progress::Downloading {
            name,
            received,
            size,
            ..
        } = &progress
        {
            app.state::<Mods>()
                .paused
                .lock()
                .unwrap()
                .track(PausedDownload {
                    app_id,
                    mod_id: mod_id.to_owned(),
                    name: name.clone(),
                    received: *received,
                    total: *size,
                });
        }
        let _ = app.emit(
            "install-progress",
            InstallEvent {
                app_id,
                mod_id: mod_id.to_owned(),
                progress,
            },
        );
    }
}

/// Interrupted download (app closed, network lost): shown as "paused" with
/// enough to resume where it was, or to throw it away.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PausedDownload {
    app_id: u32,
    mod_id: String,
    /// Package in progress when it stopped.
    name: String,
    received: u64,
    total: Option<u64>,
}

/// Journal of running downloads, kept in `downloads.json`. Outside the
/// database: it stays locked during a whole install.
struct Paused {
    file: std::path::PathBuf,
    entries: HashMap<(u32, String), PausedDownload>,
    saved_at: Option<Instant>,
}

impl Paused {
    fn load(file: std::path::PathBuf) -> Self {
        let entries = std::fs::read(&file)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Vec<PausedDownload>>(&bytes).ok())
            .unwrap_or_default()
            .into_iter()
            .map(|d| ((d.app_id, d.mod_id.clone()), d))
            .collect();
        Self {
            file,
            entries,
            saved_at: None,
        }
    }

    fn save(&mut self) {
        let list: Vec<&PausedDownload> = self.entries.values().collect();
        if let Ok(json) = serde_json::to_vec(&list) {
            let tmp = self.file.with_extension("tmp");
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(tmp, &self.file);
            }
        }
        self.saved_at = Some(Instant::now());
    }

    /// Progress of a download: to disk every 2 s at most.
    fn track(&mut self, download: PausedDownload) {
        let fresh = !self
            .entries
            .contains_key(&(download.app_id, download.mod_id.clone()));
        self.entries
            .insert((download.app_id, download.mod_id.clone()), download);
        if fresh
            || self
                .saved_at
                .is_none_or(|at| at.elapsed() >= Duration::from_secs(2))
        {
            self.save();
        }
    }

    fn forget(&mut self, app_id: u32, mod_id: &str) {
        if self.entries.remove(&(app_id, mod_id.to_owned())).is_some() {
            self.save();
        }
    }
}

/// End of an install: a network drop leaves the download paused (enough to
/// resume); the rest (success, cancel, error) takes it out of the
/// journal.
fn settle_download(mods: &Mods, app_id: u32, mod_id: &str, error: Option<&InstallError>) {
    let interrupted = matches!(
        error,
        Some(InstallError::Download {
            source: mod_engine::download::DownloadError::Network(_),
            ..
        })
    );
    if !interrupted {
        mods.paused.lock().unwrap().forget(app_id, mod_id);
    }
}

/// The game's paused downloads, whose partial file is still cached.
#[tauri::command]
pub async fn paused_downloads(
    mods: State<'_, Mods>,
    app_id: u32,
) -> Result<Vec<PausedDownload>, String> {
    let active: HashSet<String> = mods
        .cancels
        .lock()
        .unwrap()
        .keys()
        .filter(|(id, _)| *id == app_id)
        .map(|(_, m)| m.clone())
        .collect();
    let mut paused = mods.paused.lock().unwrap();
    let candidates: Vec<PausedDownload> = paused
        .entries
        .values()
        .filter(|d| d.app_id == app_id && !active.contains(&d.mod_id))
        .cloned()
        .collect();
    let mut kept = Vec::new();
    for download in candidates {
        if installer::partial_downloads(&mods.paths, &download.mod_id).is_empty() {
            // Nothing cached anymore: nothing to resume.
            paused.forget(app_id, &download.mod_id);
        } else {
            kept.push(download);
        }
    }
    Ok(kept)
}

/// Throws away a paused download and what it already received.
#[tauri::command]
pub async fn discard_download(
    mods: State<'_, Mods>,
    app_id: u32,
    mod_id: String,
) -> Result<(), String> {
    installer::discard_partial_downloads(&mods.paths, &mod_id);
    mods.paused.lock().unwrap().forget(app_id, &mod_id);
    Ok(())
}

enum NexusFailure {
    /// Free account: go through the site, at this URL.
    Website(String),
    /// Free account, several files: this one arrived, the next one
    /// downloads on the site, at this URL.
    NextFile(String),
    /// Several files: the user picks.
    Choose(Vec<nexus_source::ModFile>),
    Cancelled,
    Error(String),
}

fn close_first(game: &KnownGame) -> String {
    format!(
        "Close {} first, then try again.\nMods can't be changed while the game is running.",
        game.profile.name
    )
}

/// Bethesda games: `plugins.txt` follows the placed mods. A failure doesn't
/// undo the change: it is logged, the next one tries again.
fn sync_plugins(state: &ModState, app_id: u32, game: &KnownGame) {
    let Some(list) = game.profile.plugins_file() else {
        return;
    };
    match installer::sync_plugins(state, &game_key(app_id), &game.dir, &list) {
        Ok(true) => log::info!("updated {}", list.display()),
        Ok(false) => {}
        Err(error) => log::warn!("couldn't update {}: {error}", list.display()),
    }
}

/// Installs a Nexus mod (and its frameworks, from GitHub).
async fn run_nexus_install(
    app: &AppHandle,
    app_id: u32,
    game: &KnownGame,
    mod_id: &str,
    file: NexusFile<'_>,
) -> Result<NexusInstall, NexusFailure> {
    let catalog = app.state::<Catalog>();
    let mods = app.state::<Mods>();
    let nexus_id = nexus_source::parse_mod_key(mod_id)
        .ok_or_else(|| NexusFailure::Error("This mod doesn't exist on Nexus Mods.".into()))?;
    let cancel = mods.begin(app_id, mod_id);
    let installer = Installer {
        source: &catalog.nexus,
        catalog: None,
        client: &catalog.client,
        paths: &mods.paths,
        cancel: Some(&cancel),
    };
    let key = game_key(app_id);
    let target = Target {
        game: &key,
        game_dir: &game.dir,
        community: &game.community,
    };
    let result = {
        let _fetching = mods.fetching.lock().await;
        let mut state = mods.state.lock().await;
        let result = installer
            .install_nexus(
                &mut state,
                &target,
                &game.profile,
                &catalog.github,
                nexus_id,
                file,
                game.mods_before(),
                progress_emitter(app, app_id, mod_id),
            )
            .await;
        sync_plugins(&state, app_id, game);
        result
    };
    mods.end(app_id, mod_id);
    settle_download(&mods, app_id, mod_id, result.as_ref().err());
    match result {
        Ok(done) => Ok(done),
        Err(InstallError::Cancelled) => Err(NexusFailure::Cancelled),
        Err(InstallError::Source(SourceError::WebsiteOnly { .. })) => Err(NexusFailure::Website(
            website_download_url(&catalog, game, nexus_id, None).await,
        )),
        Err(InstallError::NexusWebsite { file_id }) => {
            let url = website_download_url(&catalog, game, nexus_id, Some(file_id)).await;
            Err(match file {
                NexusFile::Files { link: Some(_), .. } => NexusFailure::NextFile(url),
                _ => NexusFailure::Website(url),
            })
        }
        Err(InstallError::ChooseFiles(files)) => Err(NexusFailure::Choose(files)),
        Err(error) => Err(NexusFailure::Error(error_message(&error))),
    }
}

/// Download page on Nexus of the file to install (the main one, or the one
/// from before for an old version of the game): its "Slow download" button
/// opens the `nxm://` link that comes back to Modalia.
async fn website_download_url(
    catalog: &Catalog,
    game: &KnownGame,
    mod_id: u32,
    file_id: Option<u32>,
) -> String {
    let domain = game
        .profile
        .nexus
        .as_ref()
        .map_or("", |n| n.domain.as_str());
    let page = format!("https://www.nexusmods.com/{domain}/mods/{mod_id}");
    if let Some(file_id) = file_id {
        return format!("{page}?tab=files&file_id={file_id}&nmm=1");
    }
    match catalog.nexus.mod_info(domain, mod_id).await {
        Ok(info) => match info.file_for(game.mods_before()) {
            Some(file) => format!("{page}?tab=files&file_id={}&nmm=1", file.file_id),
            None => format!("{page}?tab=files"),
        },
        Err(_) => format!("{page}?tab=files"),
    }
}

/// Stops a running or queued install. What is already downloaded stays
/// cached for next time; the game is not touched.
#[tauri::command]
pub async fn cancel_install(
    mods: State<'_, Mods>,
    app_id: u32,
    mod_id: String,
) -> Result<bool, String> {
    let cancel = mods.cancels.lock().unwrap().get(&(app_id, mod_id)).cloned();
    Ok(match cancel {
        Some(cancel) => {
            cancel.cancel();
            true
        }
        None => false,
    })
}

#[tauri::command]
pub async fn set_mod_enabled(
    app: AppHandle,
    library: State<'_, Library>,
    mods: State<'_, Mods>,
    app_id: u32,
    mod_id: String,
    enabled: bool,
) -> Result<Outcome<InstalledMod>, String> {
    let game = library.game(app_id).ok_or(UNKNOWN_GAME)?;
    if mods.is_running(&game) {
        queue(&app, app_id, PendingOp::SetEnabled { mod_id, enabled }).await?;
        return Ok(Outcome::Queued);
    }
    let mut state = mods.state.lock().await;
    let value = installer::set_enabled(&mut state, &mods.paths, &game_key(app_id), &game.dir, &mod_id, enabled)
        .map_err(|e| error_message(&e))?;
    sync_plugins(&state, app_id, &game);
    Ok(Outcome::Done { value })
}

#[tauri::command]
pub async fn uninstall_mod(
    app: AppHandle,
    library: State<'_, Library>,
    mods: State<'_, Mods>,
    app_id: u32,
    mod_id: String,
) -> Result<Outcome<()>, String> {
    let game = library.game(app_id).ok_or(UNKNOWN_GAME)?;
    if mods.is_running(&game) {
        queue(&app, app_id, PendingOp::Uninstall { mod_id }).await?;
        return Ok(Outcome::Queued);
    }
    let key = game_key(app_id);
    let mut state = mods.state.lock().await;
    let removed = installer::uninstall(&mut state, &mods.paths, &key, &game.dir, &mod_id)
        .map_err(|e| error_message(&e))?;
    sync_plugins(&state, app_id, &game);
    let at = Instant::now();
    mods.removed
        .lock()
        .unwrap()
        .insert((key.clone(), mod_id.clone()), (at, removed));
    tauri::async_runtime::spawn(purge_later(app, key, mod_id, at));
    Ok(Outcome::Done { value: () })
}

/// Frees the storage of a removed mod, if "Undo" wasn't used.
async fn purge_later(app: AppHandle, key: String, mod_id: String, at: Instant) {
    tokio::time::sleep(UNDO_FOR).await;
    let mods = app.state::<Mods>();
    let entry = (key, mod_id);
    {
        let mut removed = mods.removed.lock().unwrap();
        // Restored, or removed again since: it is no longer this removal.
        if removed.get(&entry).is_none_or(|(when, _)| *when != at) {
            return;
        }
        removed.remove(&entry);
    }
    let _fetching = mods.fetching.lock().await;
    let mut state = mods.state.lock().await;
    if let Err(error) = installer::purge(&mut state, &mods.paths, &entry.0, &entry.1) {
        log::warn!("couldn't free the storage of {}: {error}", entry.1);
    }
}

/// Undoes a recent uninstall.
#[tauri::command]
pub async fn restore_mod(
    library: State<'_, Library>,
    mods: State<'_, Mods>,
    app_id: u32,
    mod_id: String,
) -> Result<InstalledMod, String> {
    let game = library.game(app_id).ok_or(UNKNOWN_GAME)?;
    if mods.is_running(&game) {
        return Err("Close the game first, then try again.".into());
    }
    let key = game_key(app_id);
    let (_, removed) = mods
        .removed
        .lock()
        .unwrap()
        .remove(&(key, mod_id))
        .ok_or("This mod can't be restored anymore.")?;
    let mut state = mods.state.lock().await;
    let restored = installer::restore(&mut state, &mods.paths, &game.dir, removed).map_err(|e| error_message(&e))?;
    sync_plugins(&state, app_id, &game);
    Ok(restored)
}

/// Goes back to a previous version of a mod (`package`, one of its
/// `previousVersions`), downloading nothing again.
#[tauri::command]
pub async fn revert_mod(
    library: State<'_, Library>,
    mods: State<'_, Mods>,
    app_id: u32,
    mod_id: String,
    package: String,
) -> Result<InstalledMod, String> {
    let game = library.game(app_id).ok_or(UNKNOWN_GAME)?;
    if mods.is_running(&game) {
        return Err("Close the game first, then try again.".into());
    }
    let mut state = mods.state.lock().await;
    let reverted = installer::revert(&mut state, &mods.paths, &game_key(app_id), &game.dir, &mod_id, &package)
        .map_err(|e| error_message(&e))?;
    sync_plugins(&state, app_id, &game);
    Ok(reverted)
}

/// At startup: moves the old storage to the hash-based storage (only once,
/// downloading nothing again), then cleans up. Commands that touch mods
/// wait for it to finish.
pub fn tidy_storage(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mods = app.state::<Mods>();
        let _fetching = mods.fetching.lock().await;
        let mut state = mods.state.lock().await;
        let result = tokio::task::block_in_place(|| {
            let migrated = installer::migrate_store(&mut state, &mods.paths)?;
            let freed = installer::collect_garbage(&state, &mods.paths)?;
            Ok::<_, installer::InstallError>((migrated, freed))
        });
        match result {
            Ok((migrated, freed)) => log::info!(
                "storage: {migrated} mods moved to packages, {} files freed ({})",
                freed.files,
                mod_engine::download::human_size(freed.bytes)
            ),
            Err(error) => log::warn!("couldn't tidy the storage: {error}"),
        }
    });
}

// ------------------------------------------------- game reinstalled or gone

/// Enabled mods deleted from the game folder (game reinstalled, files
/// verified by Steam): their names.
#[tauri::command]
pub async fn missing_mods(
    library: State<'_, Library>,
    mods: State<'_, Mods>,
    app_id: u32,
) -> Result<Vec<String>, String> {
    let game = library.game(app_id).ok_or(UNKNOWN_GAME)?;
    let state = mods.state.lock().await;
    let missing = installer::missing(&state, &game_key(app_id), &game.dir).map_err(|e| error_message(&e))?;
    Ok(missing.into_iter().map(|m| m.name).collect())
}

/// Places the mods deleted from the game again from the storage, downloading
/// nothing again. Returns their count.
#[tauri::command]
pub async fn put_back_mods(
    library: State<'_, Library>,
    mods: State<'_, Mods>,
    app_id: u32,
) -> Result<usize, String> {
    let game = library.game(app_id).ok_or(UNKNOWN_GAME)?;
    if mods.is_running(&game) {
        return Err("Close the game first, then try again.".into());
    }
    let key = game_key(app_id);
    let mut state = mods.state.lock().await;
    let missing = installer::missing(&state, &key, &game.dir).map_err(|e| error_message(&e))?;
    for m in &missing {
        installer::put_back(&mut state, &mods.paths, &key, &game.dir, &m.id).map_err(|e| error_message(&e))?;
    }
    sync_plugins(&state, app_id, &game);
    log::info!("put back {} mods in {key}", missing.len());
    Ok(missing.len())
}

/// Uninstalled game whose mods Unalia still keeps in its storage.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForgottenGame {
    app_id: u32,
    name: String,
    mods: usize,
    /// Storage bytes freed by forgetting it.
    size: u64,
}

/// The game is nowhere anymore: neither in Steam nor in its manually added
/// folder. Steam unreadable: only manual adds count.
fn game_gone(app_id: u32, steam: Option<&HashMap<u32, PathBuf>>, manual: &[ManualGame]) -> bool {
    match manual.iter().find(|g| g.app_id == app_id) {
        Some(game) => !Path::new(&game.path).is_dir() && steam.is_none_or(|s| !s.contains_key(&app_id)),
        None => steam.is_some_and(|s| !s.contains_key(&app_id)),
    }
}

fn app_id_of(key: &str) -> Option<u32> {
    key.strip_prefix("steam:")?.parse().ok()
}

#[tauri::command]
pub async fn forgotten_games(
    app: AppHandle,
    catalog: State<'_, Catalog>,
    mods: State<'_, Mods>,
) -> Result<Vec<ForgottenGame>, String> {
    let steam = tauri::async_runtime::spawn_blocking(crate::steam_dirs)
        .await
        .map_err(crate::support::logged)?;
    let games_catalog = catalog.games(&app).await;
    let state = mods.state.lock().await;
    let manual = state.manual_games().map_err(crate::support::logged)?;
    let mut forgotten = Vec::new();
    for key in state.games().map_err(crate::support::logged)? {
        let Some(app_id) = app_id_of(&key) else {
            continue;
        };
        if !game_gone(app_id, steam.as_ref(), &manual) {
            continue;
        }
        let profile = games_catalog.as_ref().and_then(|c| {
            c.by_steam_app_id(app_id).or_else(|| {
                let game = manual.iter().find(|g| g.app_id == app_id)?;
                c.by_id(&game.profile_id)
            })
        });
        forgotten.push(ForgottenGame {
            app_id,
            name: profile.map_or_else(|| format!("Steam game {app_id}"), |p| p.name.clone()),
            mods: state.list(&key).map_err(crate::support::logged)?.len(),
            size: installer::storage_size(&mods.paths, &key),
        });
    }
    Ok(forgotten)
}

/// Forgets an uninstalled game and frees the storage of its mods.
#[tauri::command]
pub async fn forget_game(mods: State<'_, Mods>, app_id: u32) -> Result<(), String> {
    let steam = tauri::async_runtime::spawn_blocking(crate::steam_dirs)
        .await
        .map_err(crate::support::logged)?;
    let _fetching = mods.fetching.lock().await;
    let mut state = mods.state.lock().await;
    let manual = state.manual_games().map_err(crate::support::logged)?;
    // Checked again here: forgetting a game still there would leave its
    // mods in its folder with no way left to remove them.
    if !game_gone(app_id, steam.as_ref(), &manual) {
        return Err("This game is installed again: open it to manage its mods.".into());
    }
    let key = game_key(app_id);
    let count = installer::forget(&mut state, &mods.paths, &key).map_err(|e| error_message(&e))?;
    if manual.iter().any(|g| g.app_id == app_id) {
        state.remove_manual_game(app_id).map_err(crate::support::logged)?;
    }
    log::info!("forgot {key} and its {count} mods");
    Ok(())
}

/// Removes a manually added game: its mods leave its folder, which gets its
/// original state back, then it disappears from Unalia with its settings
/// and its storage. The game itself stays installed. Returns the number of
/// removed mods.
#[tauri::command]
pub async fn remove_manual_game(
    library: State<'_, Library>,
    mods: State<'_, Mods>,
    app_id: u32,
) -> Result<usize, String> {
    if let Some(game) = library.game(app_id)
        && mods.is_running(&game)
    {
        return Err(close_first(&game));
    }
    let _fetching = mods.fetching.lock().await;
    let mut state = mods.state.lock().await;
    let manual = state.manual_games().map_err(crate::support::logged)?;
    let game = manual
        .iter()
        .find(|g| g.app_id == app_id)
        .ok_or("Only games added by picking their folder can be removed.")?;
    let key = game_key(app_id);
    let dir = PathBuf::from(&game.path);
    let count = installer::remove_game(&mut state, &mods.paths, &key, dir.is_dir().then_some(dir.as_path()))
        .map_err(|e| error_message(&e))?;
    state.remove_manual_game(app_id).map_err(crate::support::logged)?;
    for setting in [crate::launch_setting(app_id), crate::version_setting(app_id)] {
        state.set_setting(&setting, None).map_err(crate::support::logged)?;
    }
    // No more "Undo" for the mods removed just before.
    mods.removed.lock().unwrap().retain(|(game, _), _| *game != key);
    log::info!("removed {key} and its {count} mods");
    Ok(count)
}

/// What a reset would remove from the game.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetPreview {
    /// Mods installed by Unalia.
    mods: usize,
    /// Mod files placed by other tools.
    foreign: Vec<Foreign>,
}

#[tauri::command]
pub async fn reset_preview(
    library: State<'_, Library>,
    mods: State<'_, Mods>,
    app_id: u32,
) -> Result<ResetPreview, String> {
    let game = library.game(app_id).ok_or(UNKNOWN_GAME)?;
    let key = game_key(app_id);
    let state = mods.state.lock().await;
    let foreign =
        installer::foreign_files(&state, &key, &game.dir, &game.profile.loader).map_err(|e| error_message(&e))?;
    let count = state.list(&key).map_err(crate::support::logged)?.len();
    Ok(ResetPreview { mods: count, foreign })
}

/// Resets the game's mods: Unalia's leave it, what other tools left goes to
/// the Recycle Bin if it is in `foreign` (ticked in the dialog). The game's
/// files and the saves are not touched.
#[tauri::command]
pub async fn reset_game(
    library: State<'_, Library>,
    mods: State<'_, Mods>,
    app_id: u32,
    foreign: Vec<String>,
) -> Result<installer::Reset, String> {
    let game = library.game(app_id).ok_or(UNKNOWN_GAME)?;
    if mods.is_running(&game) {
        return Err(close_first(&game));
    }
    let _fetching = mods.fetching.lock().await;
    let mut state = mods.state.lock().await;
    let key = game_key(app_id);
    let reset = installer::reset(
        &mut state,
        &mods.paths,
        &key,
        &game.dir,
        &game.profile.loader,
        &foreign.into_iter().collect(),
        foreign::to_recycle_bin,
    )
    .map_err(|e| error_message(&e))?;
    sync_plugins(&state, app_id, &game);
    // No more "Undo" for the mods removed just before.
    mods.removed.lock().unwrap().retain(|(game, _), _| *game != key);
    log::info!(
        "reset {key}: {} mods, {} leftovers from other tools, {} left",
        reset.mods,
        reset.foreign,
        reset.left.len()
    );
    Ok(reset)
}

// ---------------------------------------------------------------- game

/// Launches the game: through Steam, or through its executable for a
/// manually added game.
#[tauri::command]
pub async fn play_game(
    app: AppHandle,
    library: State<'_, Library>,
    mods: State<'_, Mods>,
    app_id: u32,
) -> Result<(), String> {
    let game = library.game(app_id).ok_or(UNKNOWN_GAME)?;
    if mods.is_running(&game) {
        return Ok(());
    }
    // A plugin removed by hand since: the list is brought back in line.
    sync_plugins(&*mods.state.lock().await, app_id, &game);
    let target = if let Some(url) = &game.steam_shortcut {
        // Non-Steam game added to Steam: through Steam, for its overlay.
        LaunchTarget::Steam(url.clone())
    } else if game.manual {
        launch::executable_target(&game.dir, &game.exe_names)
            .ok_or("Unalia couldn't find the game's executable in its folder.")?
    } else {
        launch::steam_target(app_id)
    };
    match target {
        LaunchTarget::Steam(url) => app
            .opener()
            .open_url(url, None::<&str>)
            .map_err(|e| format!("Couldn't open Steam.\n{e}"))?,
        LaunchTarget::Executable(exe) => {
            // `open_path` goes through the system (like a double click): the
            // game starts detached from Modalia, in its own folder.
            app.opener()
                .open_path(exe.to_string_lossy(), None::<&str>)
                .map_err(|e| format!("Couldn't start the game.\n{e}"))?
        }
    }
    mods.starting
        .lock()
        .unwrap()
        .insert(app_id, Starting::new());
    emit_status(&app, app_id).await;
    Ok(())
}

/// Stops waiting for a starting game (launch abandoned in its launcher, or
/// a game that doesn't open).
#[tauri::command]
pub async fn cancel_starting(
    app: AppHandle,
    mods: State<'_, Mods>,
    app_id: u32,
) -> Result<(), String> {
    mods.starting.lock().unwrap().remove(&app_id);
    emit_status(&app, app_id).await;
    Ok(())
}

/// Process watch: spots games starting and closing, and applies the queue
/// when a game closes.
pub async fn watch(app: AppHandle) {
    loop {
        tokio::time::sleep(WATCH_EVERY).await;
        let games: Vec<(u32, KnownGame)> = app
            .state::<Library>()
            .games
            .lock()
            .unwrap()
            .iter()
            .map(|(id, g)| (*id, g.clone()))
            .collect();
        if games.is_empty() {
            continue;
        }
        let mods = app.state::<Mods>();
        let exes = mods.processes.lock().unwrap().running_exes();

        for (app_id, game) in games {
            let launchers = &game.profile.launchers;
            let now = launch::game_running(&game.dir, &exes, launchers);
            let launcher_open = exes.iter().any(|exe| launch::is_launcher(exe, launchers));
            let before = mods.running.lock().unwrap().contains(&app_id);
            let starting_expired = {
                let mut starting = mods.starting.lock().unwrap();
                let over = starting.get_mut(&app_id).is_some_and(|s| {
                    s.launcher_open = launcher_open;
                    s.saw_launcher |= launcher_open;
                    s.over(now)
                });
                if over {
                    starting.remove(&app_id);
                }
                over
            };
            // Game closed with a pending queue, even without a seen
            // transition (Modalia closed during the session, then restarted): apply.
            if !now && !before && has_pending(&app, app_id).await {
                apply_pending(&app, app_id, &game).await;
                emit_status(&app, app_id).await;
                continue;
            }
            if now == before {
                if starting_expired {
                    emit_status(&app, app_id).await;
                }
                continue;
            }
            if now {
                mods.running.lock().unwrap().insert(app_id);
            } else {
                mods.running.lock().unwrap().remove(&app_id);
                apply_pending(&app, app_id, &game).await;
            }
            emit_status(&app, app_id).await;
        }
    }
}

async fn has_pending(app: &AppHandle, app_id: u32) -> bool {
    let mods = app.state::<Mods>();
    let state = mods.state.lock().await;
    state
        .pending(&game_key(app_id))
        .is_ok_and(|ops| !ops.is_empty())
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppliedEvent {
    app_id: u32,
    applied: usize,
    failed: Vec<Failure>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Failure {
    mod_id: String,
    message: String,
}

/// Applies, in order, the changes requested during the session.
async fn apply_pending(app: &AppHandle, app_id: u32, game: &KnownGame) {
    let mods = app.state::<Mods>();
    let key = game_key(app_id);
    let ops = {
        let mut state = mods.state.lock().await;
        let ops = state.pending(&key).unwrap_or_default();
        let _ = state.set_pending(&key, &[]);
        ops
    };
    if ops.is_empty() {
        return;
    }
    mods.ready.lock().unwrap().retain(|(id, _)| *id != app_id);

    let mut applied = 0;
    let mut failed = Vec::new();
    for op in ops {
        let result = match &op {
            PendingOp::Install { mod_id, .. } if game.profile.nexus.is_some() => {
                match run_nexus_install(app, app_id, game, mod_id, NexusFile::Main).await {
                    Ok(_) => Ok(()),
                    Err(NexusFailure::Website(_) | NexusFailure::NextFile(_)) => {
                        Err("Nexus Mods needs you to start this download from its website.".into())
                    }
                    Err(NexusFailure::Choose(_)) => {
                        Err("This mod has several files: install it again to pick yours.".into())
                    }
                    Err(NexusFailure::Cancelled) => Err("The install was cancelled.".into()),
                    Err(NexusFailure::Error(message)) => Err(message),
                }
            }
            PendingOp::Install { mod_id, source } => {
                run_install(app, app_id, game, mod_id, source.as_deref())
                    .await
                    .map(|_| ())
                    .map_err(|e| error_message(&e))
            }
            PendingOp::SetEnabled { mod_id, enabled } => {
                let mut state = mods.state.lock().await;
                installer::set_enabled(&mut state, &mods.paths, &key, &game.dir, mod_id, *enabled)
                    .map(|_| ())
                    .map_err(|e| error_message(&e))
            }
            PendingOp::Uninstall { mod_id } => {
                // No "Undo" for a removal applied when the game closes: the
                // storage is freed right away.
                let mut state = mods.state.lock().await;
                installer::uninstall(&mut state, &mods.paths, &key, &game.dir, mod_id)
                    .and_then(|_| installer::purge(&mut state, &mods.paths, &key, mod_id))
                    .map_err(|e| error_message(&e))
            }
        };
        match result {
            Ok(()) => applied += 1,
            Err(message) => failed.push(Failure {
                mod_id: op.mod_id().to_owned(),
                message,
            }),
        }
    }
    sync_plugins(&*mods.state.lock().await, app_id, game);
    let _ = app.emit(
        "pending-applied",
        AppliedEvent {
            app_id,
            applied,
            failed,
        },
    );
}

/// Main message (what is blocking, then what to do), then the technical
/// chain after a line break: the UI shows the first line and folds the
/// rest.
fn error_message(error: &(dyn std::error::Error + 'static)) -> String {
    let mut message = mod_engine::advice::explain(error);
    let mut source = error.source();
    let mut details = Vec::new();
    while let Some(cause) = source {
        details.push(cause.to_string());
        source = cause.source();
    }
    if !details.is_empty() {
        message.push('\n');
        message.push_str(&details.join(": "));
    }
    // Error shown to the user: also written to the log.
    log::warn!("{}", message.replace('\n', " | "));
    message
}

// ---------------------------------------------------------------- Nexus

/// Setting where the user's Nexus API key lives.
const NEXUS_KEY: &str = "nexus_api_key";

/// Last account recognized by Nexus: shown as is when offline.
const NEXUS_ACCOUNT: &str = "nexus_account";

async fn remember_account(mods: &Mods, account: Option<&Account>) {
    let value = account.and_then(|a| serde_json::to_string(a).ok());
    let _ = mods.state.lock().await.set_setting(NEXUS_ACCOUNT, value.as_deref());
}

/// At startup: saved Nexus key (in dev, falls back on `NEXUS_API_KEY` from
/// the `.env` at the repo root), and `nxm://` links.
pub fn nexus_setup(app: &AppHandle) {
    let saved = tauri::async_runtime::block_on(async {
        app.state::<Mods>()
            .state
            .lock()
            .await
            .setting(NEXUS_KEY)
            .ok()
            .flatten()
    });
    // An empty string: the user logged out, the dev `.env` key must not
    // come back.
    let key = match saved {
        Some(saved) if saved.is_empty() => None,
        Some(saved) => Some(saved),
        None => dev_env("NEXUS_API_KEY"),
    };
    let connected = key.is_some();
    app.state::<Catalog>().nexus.set_api_key(key);
    if connected {
        register_nxm(app);
    }

    let handle = app.clone();
    app.deep_link().on_open_url(move |event| {
        for url in event.urls() {
            tauri::async_runtime::spawn(handle_nxm(handle.clone(), url.to_string()));
        }
    });
    // Modalia started by an `nxm://` link.
    if let Ok(Some(urls)) = app.deep_link().get_current() {
        for url in urls {
            tauri::async_runtime::spawn(handle_nxm(app.clone(), url.to_string()));
        }
    }
}

/// Development value (environment variable, else `.env` at the repo root),
/// never read in a release build.
pub fn dev_env(name: &str) -> Option<String> {
    if !cfg!(debug_assertions) {
        return None;
    }
    if let Ok(value) = std::env::var(name) {
        return Some(value);
    }
    let env = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../.env");
    let prefix = format!("{name}=");
    std::fs::read_to_string(env).ok()?.lines().find_map(|line| {
        let value = line.trim().strip_prefix(&prefix)?;
        let value = value.trim().trim_matches(['"', '\'']);
        (!value.is_empty()).then(|| value.to_owned())
    })
}

/// Modalia becomes the app that opens Nexus "Mod Manager Download" links
/// (Windows, Linux; macOS does it when the app is installed).
fn register_nxm(app: &AppHandle) {
    #[cfg(any(windows, target_os = "linux"))]
    if let Err(error) = app.deep_link().register("nxm") {
        log::warn!("couldn't register nxm:// links: {error}");
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    let _ = app;
}

/// Application id that Nexus gives to the mod managers it approves, for
/// one-click login (SSO). Pending: while it is missing, login goes through
/// the personal API key.
const NEXUS_SSO_APP: Option<&str> = None;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NexusStatus {
    account: Option<Account>,
    /// One-click login available (Modalia approved by Nexus).
    sso: bool,
    /// Key saved but Nexus unreachable: `account` is the last recognized
    /// account (missing if it was never checked).
    offline: bool,
}

#[tauri::command]
pub async fn nexus_status(
    catalog: State<'_, Catalog>,
    mods: State<'_, Mods>,
) -> Result<NexusStatus, String> {
    let mut status = NexusStatus {
        account: None,
        sso: NEXUS_SSO_APP.is_some(),
        offline: false,
    };
    if !catalog.nexus.connected() {
        return Ok(status);
    }
    match catalog.nexus.account().await {
        Ok(account) => {
            remember_account(&mods, Some(&account)).await;
            status.account = Some(account);
        }
        Err(SourceError::Network { .. }) => {
            let saved = mods.state.lock().await.setting(NEXUS_ACCOUNT).ok().flatten();
            status.account = saved.and_then(|s| serde_json::from_str(&s).ok());
            status.offline = true;
        }
        Err(error) => return Err(error.to_string()),
    }
    Ok(status)
}

/// One-click login: Nexus's authorization page opens in the browser, the
/// key comes back by itself.
#[tauri::command]
pub async fn nexus_login(
    app: AppHandle,
    catalog: State<'_, Catalog>,
    mods: State<'_, Mods>,
) -> Result<Account, String> {
    let application = NEXUS_SSO_APP.ok_or("Nexus Mods hasn't approved Unalia's login yet.")?;
    let opener = app.clone();
    let key = mod_engine::sources::nexus_sso::login(
        mod_engine::sources::nexus_sso::SSO_URL,
        application,
        move |url| {
            let _ = opener.opener().open_url(url, None::<&str>);
        },
    )
    .await
    .map_err(|e| error_message(&e))?;
    save_nexus_key(&app, &catalog, &mods, key).await
}

/// Keeps a key only if Nexus recognizes it.
async fn save_nexus_key(
    app: &AppHandle,
    catalog: &Catalog,
    mods: &Mods,
    key: String,
) -> Result<Account, String> {
    let previous = catalog.nexus.connected();
    catalog.nexus.set_api_key(Some(key.clone()));
    let account = match catalog.nexus.account().await {
        Ok(account) => account,
        Err(error) => {
            if !previous {
                catalog.nexus.set_api_key(None);
            }
            return Err(error.to_string());
        }
    };
    mods.state
        .lock()
        .await
        .set_setting(NEXUS_KEY, Some(&key))
        .map_err(crate::support::logged)?;
    remember_account(mods, Some(&account)).await;
    register_nxm(app);
    Ok(account)
}

/// Checks the key with Nexus, then saves it on this machine.
#[tauri::command]
pub async fn nexus_connect(
    app: AppHandle,
    catalog: State<'_, Catalog>,
    mods: State<'_, Mods>,
    api_key: String,
) -> Result<Account, String> {
    let key = api_key.trim().to_owned();
    if key.is_empty() || key.len() > 512 || key.chars().any(char::is_whitespace) {
        return Err("This doesn't look like a Nexus Mods API key.".into());
    }
    save_nexus_key(&app, &catalog, &mods, key).await
}

#[tauri::command]
pub async fn nexus_disconnect(
    catalog: State<'_, Catalog>,
    mods: State<'_, Mods>,
) -> Result<(), String> {
    catalog.nexus.set_api_key(None);
    remember_account(&mods, None).await;
    // Empty rather than missing: a deliberate logout holds, even in dev
    // with a key in the `.env`.
    mods.state
        .lock()
        .await
        .set_setting(NEXUS_KEY, Some(""))
        .map_err(crate::support::logged)
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct NexusInstalled {
    app_id: u32,
    mod_id: String,
    /// Canceled by the user (not an error to show).
    cancelled: bool,
    /// Name of the installed mod, if it was installed.
    name: Option<String>,
    missing: Vec<Requirement>,
    error: Option<String>,
    /// Next file of a multiple choice: its download page, already open in
    /// the browser.
    website: Option<String>,
}

/// `nxm://` link from the site: installs the picked file, in the matching
/// game.
async fn handle_nxm(app: AppHandle, url: String) {
    let Some(link) = NxmLink::parse(&url) else {
        return;
    };
    // Started by the link: the library fills in at the first display.
    let mut found = None;
    for _ in 0..60 {
        found = app
            .state::<Library>()
            .games
            .lock()
            .unwrap()
            .iter()
            .find(|(_, g)| {
                g.profile
                    .nexus
                    .as_ref()
                    .is_some_and(|n| n.domain == link.domain)
            })
            .map(|(id, g)| (*id, g.clone()));
        if found.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let mod_id = nexus_source::mod_key(link.mod_id);
    let Some((app_id, game)) = found else {
        let _ = app.emit(
            "nexus-installed",
            NexusInstalled {
                app_id: 0,
                cancelled: false,
                mod_id,
                name: None,
                missing: Vec::new(),
                error: Some(format!(
                    "This link is for a game Unalia can't find ({}).",
                    link.domain
                )),
                website: None,
            },
        );
        return;
    };
    let mods = app.state::<Mods>();
    // One of the files picked in Unalia: the install resumes with the whole
    // choice.
    let pending = (app_id, mod_id.clone());
    let chosen = mods
        .chosen_files
        .lock()
        .unwrap()
        .get(&pending)
        .filter(|ids| ids.contains(&link.file_id))
        .cloned();
    let file = match &chosen {
        Some(ids) => NexusFile::Files {
            ids,
            link: Some(&link),
        },
        None => NexusFile::Link(&link),
    };
    let mut website = None;
    let result = if mods.is_running(&game) {
        Err(Some(close_first(&game)))
    } else {
        run_nexus_install(&app, app_id, &game, &mod_id, file)
            .await
            .map_err(|failure| match failure {
                NexusFailure::NextFile(url) => {
                    website = Some(url);
                    None
                }
                NexusFailure::Website(_) => Some(
                    "Nexus Mods refused this link. It may have expired: click the download button again.".into(),
                ),
                NexusFailure::Choose(_) | NexusFailure::Cancelled => None,
                NexusFailure::Error(message) => Some(message),
            })
    };
    if result.is_ok() {
        mods.chosen_files.lock().unwrap().remove(&pending);
    }
    // Next file: its page opens, the UI waits for its link.
    if let Some(url) = &website {
        use tauri_plugin_opener::OpenerExt;
        let _ = app.opener().open_url(url, None::<&str>);
    }
    let event = match result {
        Ok(done) => NexusInstalled {
            app_id,
            name: done
                .mods
                .iter()
                .find(|m| m.id == mod_id)
                .map(|m| m.name.clone()),
            mod_id,
            cancelled: false,
            missing: done.missing,
            error: None,
            website: None,
        },
        Err(error) => NexusInstalled {
            app_id,
            mod_id,
            cancelled: error.is_none() && website.is_none(),
            name: None,
            missing: Vec::new(),
            error,
            website,
        },
    };
    let _ = app.emit("nexus-installed", event);
}

//! Commandes d'installation et de lancement : brancher le moteur sur
//! l'interface, avec la progression et l'état des jeux poussés par
//! événements (SPEC §6.4).
//!
//! Pendant qu'un jeu tourne, toute modification de ses mods est mise en
//! file et appliquée à sa fermeture (SPEC §3.6).

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Mutex as SyncMutex;
use std::time::{Duration, Instant};

use mod_engine::installer::{self, Installer, Paths, Progress, Target, Update};
use mod_engine::launch::{self, LaunchTarget, PendingOp, ProcessWatcher};
use mod_engine::state::{InstalledMod, State as ModState};
use serde::Serialize;
use tauri::async_runtime::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;

use crate::catalog::Catalog;
use crate::{KnownGame, Library};

/// Délai pendant lequel un jeu lancé est « en démarrage » avant d'apparaître
/// dans les processus (Steam peut mettre un moment à le lancer).
const STARTING_FOR: Duration = Duration::from_secs(60);
/// Fréquence de la veille des processus.
const WATCH_EVERY: Duration = Duration::from_secs(2);

pub struct Mods {
    /// Une installation à la fois : elle garde la base pendant toute sa
    /// durée, les suivantes attendent leur tour (file d'attente).
    state: Mutex<ModState>,
    paths: Paths,
    /// Mods tout juste désinstallés, gardés pour « Undo ». Côté moteur : la
    /// webview ne renvoie qu'un identifiant, jamais un chemin.
    removed: SyncMutex<HashMap<(String, String), InstalledMod>>,
    processes: SyncMutex<ProcessWatcher>,
    /// Jeux vus en cours d'exécution au dernier passage de la veille.
    running: SyncMutex<HashSet<u32>>,
    /// Jeux lancés depuis Modalia, pas encore visibles dans les processus.
    starting: SyncMutex<HashMap<u32, Instant>>,
    /// Téléchargements et extractions : un à la fois, pour que le
    /// préchargement et une installation n'écrivent pas la même archive.
    fetching: Mutex<()>,
    /// Installations en attente déjà téléchargées et extraites : à la
    /// fermeture du jeu, il ne reste qu'à poser les liens.
    ready: SyncMutex<HashSet<(u32, String)>>,
}

impl Mods {
    pub(crate) fn state(&self) -> &Mutex<ModState> {
        &self.state
    }

    pub fn open(app: &AppHandle) -> Result<Self, Box<dyn std::error::Error>> {
        // Données locales (`%LOCALAPPDATA%` sous Windows), jamais le profil
        // itinérant : des centaines de Mo de mods n'ont pas à être
        // synchronisés entre machines.
        let data = app.path().app_local_data_dir()?;
        let cache = app.path().app_cache_dir()?;
        // Avant toute ouverture : sous Windows, un fichier ouvert ne peut
        // pas être remplacé.
        migrate_from_roaming(&app.path().app_data_dir()?, &data);
        Ok(Self {
            state: Mutex::new(ModState::open(&data.join("modalia.db"))?),
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
        })
    }

    /// Le jeu tourne-t-il en ce moment ? Vérifié à la demande, sans
    /// attendre le prochain passage de la veille.
    fn is_running(&self, game: &KnownGame) -> bool {
        let exes = self.processes.lock().unwrap().running_exes();
        launch::is_running(&game.dir, &exes)
    }

    fn is_starting(&self, app_id: u32) -> bool {
        self.starting
            .lock()
            .unwrap()
            .get(&app_id)
            .is_some_and(|at| at.elapsed() < STARTING_FOR)
    }
}

/// Les premières versions rangeaient base et mods dans `%APPDATA%`
/// (itinérant). On les déplace une fois vers le dossier local, sinon les
/// mods déjà posés dans les jeux deviendraient des fichiers orphelins.
/// Sous macOS et Linux, les deux dossiers sont les mêmes : rien à faire.
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
        eprintln!("couldn't move Modalia's data out of the roaming profile");
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

const UNKNOWN_GAME: &str = "This game isn't in your library anymore. Go back home to rescan it.";

/// Résultat d'une action sur les mods : faite tout de suite, ou mise en
/// attente parce que le jeu tourne.
#[derive(Serialize)]
#[serde(rename_all = "camelCase", tag = "status")]
pub enum Outcome<T> {
    Done { value: T },
    Queued,
}

// ---------------------------------------------------------------- état

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingInfo {
    #[serde(flatten)]
    op: PendingOp,
    /// Nom à afficher (nom installé, sinon tiré de l'identifiant).
    name: String,
    /// Installation déjà téléchargée pendant la partie.
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

/// Met `op` en attente : le jeu tourne.
async fn queue(app: &AppHandle, app_id: u32, op: PendingOp) -> Result<(), String> {
    let mods = app.state::<Mods>();
    let key = game_key(app_id);
    {
        let mut state = mods.state.lock().await;
        let next = launch::enqueue(&state.pending(&key).map_err(|e| e.to_string())?, op);
        state.set_pending(&key, &next).map_err(|e| e.to_string())?;
    }
    // Le jeu tourne : la veille doit le savoir pour appliquer la file à sa
    // fermeture, même s'il a été lancé hors de Modalia.
    mods.running.lock().unwrap().insert(app_id);
    emit_status(app, app_id).await;
    Ok(())
}

/// Retire de la file le changement en attente pour ce mod.
#[tauri::command]
pub async fn cancel_pending(app: AppHandle, app_id: u32, mod_id: String) -> Result<(), String> {
    {
        let mods = app.state::<Mods>();
        let key = game_key(app_id);
        let mut state = mods.state.lock().await;
        let next: Vec<_> = state
            .pending(&key)
            .map_err(|e| e.to_string())?
            .into_iter()
            .filter(|op| op.mod_id() != mod_id)
            .collect();
        state.set_pending(&key, &next).map_err(|e| e.to_string())?;
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
        .map_err(|e| e.to_string())
}

/// Mises à jour disponibles pour les mods du jeu. Rien n'est installé :
/// l'utilisateur choisit (SPEC §3.5). Mettre à jour = `install_mod`.
#[tauri::command]
pub async fn check_updates(
    library: State<'_, Library>,
    catalog: State<'_, Catalog>,
    mods: State<'_, Mods>,
    app_id: u32,
) -> Result<Vec<Update>, String> {
    let game = library.game(app_id).ok_or(UNKNOWN_GAME)?;
    // La base n'est pas gardée pendant les appels réseau.
    let installed = mods
        .state
        .lock()
        .await
        .list(&game_key(app_id))
        .map_err(|e| e.to_string())?;
    Ok(installer::available_updates(&catalog.thunderstore, &game.community, &installed).await)
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallEvent {
    app_id: u32,
    mod_id: String,
    progress: Progress,
}

/// Installe un mod et ses dépendances, ou l'installation est mise en
/// attente si le jeu tourne. La progression arrive par l'événement
/// `install-progress`.
#[tauri::command]
pub async fn install_mod(
    app: AppHandle,
    library: State<'_, Library>,
    mods: State<'_, Mods>,
    app_id: u32,
    mod_id: String,
) -> Result<Outcome<Vec<InstalledMod>>, String> {
    let game = library.game(app_id).ok_or(UNKNOWN_GAME)?;
    if mods.is_running(&game) {
        queue(
            &app,
            app_id,
            PendingOp::Install {
                mod_id: mod_id.clone(),
            },
        )
        .await?;
        // Le jeu tourne, mais rien n'empêche de télécharger : seul le
        // déploiement attend sa fermeture.
        tauri::async_runtime::spawn(prefetch(app.clone(), app_id, game, mod_id));
        return Ok(Outcome::Queued);
    }
    let value = run_install(&app, app_id, &game, &mod_id).await?;
    Ok(Outcome::Done { value })
}

/// Télécharge et extrait en arrière-plan une installation mise en attente.
/// Un échec ici n'est pas grave : l'installation réessaiera à la fermeture
/// du jeu et signalera alors l'erreur.
async fn prefetch(app: AppHandle, app_id: u32, game: KnownGame, mod_id: String) {
    let catalog = app.state::<Catalog>();
    let mods = app.state::<Mods>();
    let games_catalog = catalog.games(&app).await;
    let key = game_key(app_id);
    // Liste des mods installés, sans garder la base pendant le téléchargement.
    let existing = mods.state.lock().await.list(&key).unwrap_or_default();
    let installer = Installer {
        source: &catalog.thunderstore,
        catalog: games_catalog.as_deref(),
        client: &catalog.client,
        paths: &mods.paths,
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
) -> Result<Vec<InstalledMod>, String> {
    let catalog = app.state::<Catalog>();
    let mods = app.state::<Mods>();
    let games_catalog = catalog.games(app).await;
    let installer = Installer {
        source: &catalog.thunderstore,
        catalog: games_catalog.as_deref(),
        client: &catalog.client,
        paths: &mods.paths,
    };
    let key = game_key(app_id);
    let target = Target {
        game: &key,
        game_dir: &game.dir,
        community: &game.community,
    };

    let _fetching = mods.fetching.lock().await;
    let mut state = mods.state.lock().await;
    // Pas plus d'un événement par 1 % : l'interface n'en a pas besoin.
    let mut last = None;
    let result = installer
        .install(&mut state, &target, mod_id, |progress| {
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
            let _ = app.emit(
                "install-progress",
                InstallEvent {
                    app_id,
                    mod_id: mod_id.to_owned(),
                    progress,
                },
            );
        })
        .await;
    result.map_err(|e| error_message(&e))
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
    let value = installer::set_enabled(&mut state, &game_key(app_id), &game.dir, &mod_id, enabled)
        .map_err(|e| error_message(&e))?;
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
    let removed = installer::uninstall(&mut state, &key, &game.dir, &mod_id)
        .map_err(|e| error_message(&e))?;
    mods.removed.lock().unwrap().insert((key, mod_id), removed);
    Ok(Outcome::Done { value: () })
}

/// Annule une désinstallation récente.
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
    let removed = mods
        .removed
        .lock()
        .unwrap()
        .remove(&(key, mod_id))
        .ok_or("This mod can't be restored anymore.")?;
    let mut state = mods.state.lock().await;
    installer::restore(&mut state, &game.dir, removed).map_err(|e| error_message(&e))
}

// ---------------------------------------------------------------- jeu

/// Lance le jeu : par Steam, ou par son exécutable pour un jeu ajouté à la
/// main.
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
    let target = if game.manual {
        launch::executable_target(&game.dir, &game.exe_names)
            .ok_or("Modalia couldn't find the game's executable in its folder.")?
    } else {
        launch::steam_target(app_id)
    };
    match target {
        LaunchTarget::Steam(url) => app
            .opener()
            .open_url(url, None::<&str>)
            .map_err(|e| format!("Couldn't open Steam.\n{e}"))?,
        LaunchTarget::Executable(exe) => {
            // `open_path` passe par le système (comme un double-clic) : le
            // jeu démarre détaché de Modalia, dans son propre dossier.
            app.opener()
                .open_path(exe.to_string_lossy(), None::<&str>)
                .map_err(|e| format!("Couldn't start the game.\n{e}"))?
        }
    }
    mods.starting.lock().unwrap().insert(app_id, Instant::now());
    emit_status(&app, app_id).await;
    Ok(())
}

/// Veille des processus : repère le démarrage et la fermeture des jeux, et
/// applique la file d'attente quand un jeu se ferme.
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
            let now = launch::is_running(&game.dir, &exes);
            let before = mods.running.lock().unwrap().contains(&app_id);
            let starting_expired = mods
                .starting
                .lock()
                .unwrap()
                .get(&app_id)
                .is_some_and(|at| now || at.elapsed() >= STARTING_FOR);
            if starting_expired {
                mods.starting.lock().unwrap().remove(&app_id);
            }
            // Jeu fermé avec une file en attente, même sans transition vue
            // (Modalia fermé pendant la partie, puis relancé) : on applique.
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

/// Applique, dans l'ordre, les changements demandés pendant la partie.
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
            PendingOp::Install { mod_id } => {
                run_install(app, app_id, game, mod_id).await.map(|_| ())
            }
            PendingOp::SetEnabled { mod_id, enabled } => {
                let mut state = mods.state.lock().await;
                installer::set_enabled(&mut state, &key, &game.dir, mod_id, *enabled)
                    .map(|_| ())
                    .map_err(|e| error_message(&e))
            }
            PendingOp::Uninstall { mod_id } => {
                let mut state = mods.state.lock().await;
                installer::uninstall(&mut state, &key, &game.dir, mod_id)
                    .map(|_| ())
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
    let _ = app.emit(
        "pending-applied",
        AppliedEvent {
            app_id,
            applied,
            failed,
        },
    );
}

/// Message principal, puis la cause technique après un retour à la ligne :
/// l'interface affiche la première ligne et replie le reste.
fn error_message(error: &dyn std::error::Error) -> String {
    let mut message = error.to_string();
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
    message
}

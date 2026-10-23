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

use mod_engine::download::Cancel;
use mod_engine::installer::{
    self, InstallError, Installer, NexusFile, NexusInstall, Paths, Progress, Target, Update,
};
use mod_engine::launch::{self, LaunchTarget, PendingOp, ProcessWatcher};
use mod_engine::sources::SourceError;
use mod_engine::sources::nexus::{self as nexus_source, Account, NxmLink, Requirement};
use mod_engine::state::{InstalledMod, State as ModState};
use serde::{Deserialize, Serialize};
use tauri::async_runtime::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_deep_link::DeepLinkExt;
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
    starting: SyncMutex<HashMap<u32, Starting>>,
    /// Téléchargements et extractions : un à la fois, pour que le
    /// préchargement et une installation n'écrivent pas la même archive.
    fetching: Mutex<()>,
    /// Installations en attente déjà téléchargées et extraites : à la
    /// fermeture du jeu, il ne reste qu'à poser les liens.
    ready: SyncMutex<HashSet<(u32, String)>>,
    /// Drapeau d'annulation de chaque installation en cours ou en file.
    cancels: SyncMutex<HashMap<(u32, String), Cancel>>,
    /// Téléchargements en cours ou interrompus (voir `Paused`).
    paused: SyncMutex<Paused>,
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
            cancels: SyncMutex::default(),
            paused: SyncMutex::new(Paused::load(data.join("downloads.json"))),
        })
    }

    /// Exécutables des processus en cours.
    pub(crate) fn running_exes(&self) -> Vec<std::path::PathBuf> {
        self.processes.lock().unwrap().running_exes()
    }

    /// Le jeu tourne-t-il en ce moment ? Vérifié à la demande, sans
    /// attendre le prochain passage de la veille.
    fn is_running(&self, game: &KnownGame) -> bool {
        let exes = self.processes.lock().unwrap().running_exes();
        launch::game_running(&game.dir, &exes, &game.profile.launchers)
    }

    /// Début d'une installation : son drapeau d'annulation.
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

    fn is_starting(&self, app_id: u32) -> bool {
        self.starting
            .lock()
            .unwrap()
            .get(&app_id)
            .is_some_and(|s| s.launcher_open || s.at.elapsed() < STARTING_FOR)
    }
}

/// Lancement en cours : depuis quand, et si le launcher du jeu est ouvert.
#[derive(Clone, Copy)]
struct Starting {
    at: Instant,
    /// Launcher vu ouvert pendant ce lancement.
    saw_launcher: bool,
    /// Launcher ouvert au dernier passage de la veille : on attend sans
    /// limite que l'utilisateur lance le jeu.
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

    /// Le lancement est-il fini (jeu apparu) ou abandonné (launcher fermé
    /// sans jouer, ou rien après le délai) ?
    fn over(&self, game_running: bool) -> bool {
        game_running
            || (self.saw_launcher && !self.launcher_open)
            || (!self.launcher_open && self.at.elapsed() >= STARTING_FOR)
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
        eprintln!("couldn't move Unalia's data out of the roaming profile");
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
    Done {
        value: T,
    },
    Queued,
    /// Le site doit donner son accord (Nexus, compte gratuit) : l'interface
    /// ouvre `url`, le lien `nxm://` du bouton « Mod Manager Download »
    /// revient à Modalia qui termine l'installation.
    Browser {
        url: String,
    },
    /// Annulée par l'utilisateur : rien n'a changé dans le jeu.
    Cancelled,
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
    if game.profile.nexus.is_some() {
        return Ok(installer::available_updates_nexus(
            &catalog.nexus,
            &catalog.github,
            &game.profile,
            &installed,
        )
        .await);
    }
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
    if game.profile.nexus.is_some() {
        if mods.is_running(&game) {
            return Err(close_first(&game));
        }
        return match run_nexus_install(&app, app_id, &game, &mod_id, NexusFile::Main).await {
            Ok(done) => Ok(Outcome::Done { value: done.mods }),
            Err(NexusFailure::Website(url)) => Ok(Outcome::Browser { url }),
            Err(NexusFailure::Cancelled) => Ok(Outcome::Cancelled),
            Err(NexusFailure::Error(message)) => Err(message),
        };
    }
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
    match run_install(&app, app_id, &game, &mod_id).await {
        Ok(value) => Ok(Outcome::Done { value }),
        Err(InstallError::Cancelled) => Ok(Outcome::Cancelled),
        Err(error) => Err(error_message(&error)),
    }
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
) -> Result<Vec<InstalledMod>, InstallError> {
    let catalog = app.state::<Catalog>();
    let mods = app.state::<Mods>();
    let games_catalog = catalog.games(app).await;
    // Avant d'attendre son tour : on peut annuler une installation en file.
    let cancel = mods.begin(app_id, mod_id);
    let installer = Installer {
        source: &catalog.thunderstore,
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

/// Événements de progression, pas plus d'un par 1 %.
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
        // Noté sur disque : si l'appli est quittée, le téléchargement
        // apparaîtra « en pause » au prochain lancement.
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

/// Téléchargement interrompu (appli quittée, réseau perdu) : affiché « en
/// pause » avec de quoi reprendre là où il en était, ou le jeter.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PausedDownload {
    app_id: u32,
    mod_id: String,
    /// Paquet en cours au moment de l'arrêt.
    name: String,
    received: u64,
    total: Option<u64>,
}

/// Journal des téléchargements en cours, gardé dans `downloads.json`. Hors
/// de la base : celle-ci reste verrouillée pendant toute une installation.
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

    /// Avancement d'un téléchargement : sur disque toutes les 2 s au plus.
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

/// Fin d'une installation : une coupure réseau laisse le téléchargement en
/// pause (de quoi reprendre) ; le reste (réussite, annulation, erreur) le
/// sort du journal.
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

/// Téléchargements du jeu en pause, dont le début est toujours en cache.
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
            // Plus rien en cache : rien à reprendre.
            paused.forget(app_id, &download.mod_id);
        } else {
            kept.push(download);
        }
    }
    Ok(kept)
}

/// Jette un téléchargement en pause et ce qu'il avait déjà reçu.
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
    /// Compte gratuit : il faut passer par le site, à cette adresse.
    Website(String),
    Cancelled,
    Error(String),
}

fn close_first(game: &KnownGame) -> String {
    format!(
        "Close {} first, then try again.\nMods can't be changed while the game is running.",
        game.profile.name
    )
}

/// Installe un mod Nexus (et ses frameworks, depuis GitHub).
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
        installer
            .install_nexus(
                &mut state,
                &target,
                &game.profile,
                &catalog.github,
                nexus_id,
                file,
                progress_emitter(app, app_id, mod_id),
            )
            .await
    };
    mods.end(app_id, mod_id);
    settle_download(&mods, app_id, mod_id, result.as_ref().err());
    match result {
        Ok(done) => Ok(done),
        Err(InstallError::Cancelled) => Err(NexusFailure::Cancelled),
        Err(InstallError::Source(SourceError::WebsiteOnly { .. })) => Err(NexusFailure::Website(
            website_download_url(&catalog, game, nexus_id).await,
        )),
        Err(error) => Err(NexusFailure::Error(error_message(&error))),
    }
}

/// Page de téléchargement du fichier principal sur Nexus : son bouton
/// « Slow download » ouvre le lien `nxm://` qui revient à Modalia.
async fn website_download_url(catalog: &Catalog, game: &KnownGame, mod_id: u32) -> String {
    let domain = game
        .profile
        .nexus
        .as_ref()
        .map_or("", |n| n.domain.as_str());
    let page = format!("https://www.nexusmods.com/{domain}/mods/{mod_id}");
    match catalog.nexus.mod_info(domain, mod_id).await {
        Ok(info) => match info.main_file() {
            Some(file) => format!("{page}?tab=files&file_id={}&nmm=1", file.file_id),
            None => format!("{page}?tab=files"),
        },
        Err(_) => format!("{page}?tab=files"),
    }
}

/// Arrête une installation en cours ou en file. Ce qui est déjà téléchargé
/// reste en cache pour une prochaine fois ; le jeu n'est pas touché.
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
    let target = if let Some(url) = &game.steam_shortcut {
        // Jeu hors Steam ajouté à Steam : par Steam, pour son overlay.
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
            // `open_path` passe par le système (comme un double-clic) : le
            // jeu démarre détaché de Modalia, dans son propre dossier.
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

/// Arrête d'attendre un jeu en démarrage (lancement abandonné dans son
/// launcher, ou jeu qui ne s'ouvre pas).
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
            PendingOp::Install { mod_id } if game.profile.nexus.is_some() => {
                match run_nexus_install(app, app_id, game, mod_id, NexusFile::Main).await {
                    Ok(_) => Ok(()),
                    Err(NexusFailure::Website(_)) => {
                        Err("Nexus Mods needs you to start this download from its website.".into())
                    }
                    Err(NexusFailure::Cancelled) => Err("The install was cancelled.".into()),
                    Err(NexusFailure::Error(message)) => Err(message),
                }
            }
            PendingOp::Install { mod_id } => run_install(app, app_id, game, mod_id)
                .await
                .map(|_| ())
                .map_err(|e| error_message(&e)),
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

// ---------------------------------------------------------------- Nexus

/// Réglage où vit la clé API Nexus de l'utilisateur.
const NEXUS_KEY: &str = "nexus_api_key";

/// Au démarrage : clé Nexus enregistrée (en dev, repli sur `NEXUS_API_KEY`
/// du `.env` à la racine du dépôt), et liens `nxm://`.
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
    // Une chaîne vide : l'utilisateur s'est déconnecté, la clé du `.env` de
    // dev ne doit pas revenir.
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
    // Modalia lancé par un lien `nxm://`.
    if let Ok(Some(urls)) = app.deep_link().get_current() {
        for url in urls {
            tauri::async_runtime::spawn(handle_nxm(app.clone(), url.to_string()));
        }
    }
}

/// Valeur de développement (variable d'environnement, sinon `.env` à la
/// racine du dépôt), jamais lue dans un build de release.
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

/// Modalia devient l'appli qui ouvre les liens « Mod Manager Download » de
/// Nexus (Windows, Linux ; macOS le fait à l'installation de l'appli).
fn register_nxm(app: &AppHandle) {
    #[cfg(any(windows, target_os = "linux"))]
    if let Err(error) = app.deep_link().register("nxm") {
        eprintln!("couldn't register nxm:// links: {error}");
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    let _ = app;
}

/// Identifiant d'application que Nexus attribue aux gestionnaires de mods
/// qu'il approuve, pour la connexion en un clic (SSO). En attente : tant
/// qu'il manque, la connexion passe par la clé API personnelle.
const NEXUS_SSO_APP: Option<&str> = None;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NexusStatus {
    account: Option<Account>,
    /// Connexion en un clic disponible (Modalia approuvé par Nexus).
    sso: bool,
}

#[tauri::command]
pub async fn nexus_status(catalog: State<'_, Catalog>) -> Result<NexusStatus, String> {
    let account = if catalog.nexus.connected() {
        Some(catalog.nexus.account().await.map_err(|e| e.to_string())?)
    } else {
        None
    };
    Ok(NexusStatus {
        account,
        sso: NEXUS_SSO_APP.is_some(),
    })
}

/// Connexion en un clic : la page d'autorisation de Nexus s'ouvre dans le
/// navigateur, la clé revient toute seule.
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

/// Garde une clé seulement si Nexus la reconnaît.
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
        .map_err(|e| e.to_string())?;
    register_nxm(app);
    Ok(account)
}

/// Vérifie la clé auprès de Nexus, puis l'enregistre sur cette machine.
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
    // Vide plutôt qu'absent : une déconnexion voulue tient, même en dev
    // avec une clé dans le `.env`.
    mods.state
        .lock()
        .await
        .set_setting(NEXUS_KEY, Some(""))
        .map_err(|e| e.to_string())
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct NexusInstalled {
    app_id: u32,
    mod_id: String,
    /// Annulée par l'utilisateur (pas une erreur à afficher).
    cancelled: bool,
    /// Nom du mod installé, s'il l'a été.
    name: Option<String>,
    missing: Vec<Requirement>,
    error: Option<String>,
}

/// Lien `nxm://` venu du site : installe le fichier choisi, dans le jeu
/// qui correspond.
async fn handle_nxm(app: AppHandle, url: String) {
    let Some(link) = NxmLink::parse(&url) else {
        return;
    };
    // Lancé par le lien : la bibliothèque se remplit au premier affichage.
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
            },
        );
        return;
    };
    let mods = app.state::<Mods>();
    let result = if mods.is_running(&game) {
        Err(Some(close_first(&game)))
    } else {
        run_nexus_install(&app, app_id, &game, &mod_id, NexusFile::Link(&link))
            .await
            .map_err(|failure| match failure {
                NexusFailure::Website(_) => Some(
                    "Nexus Mods refused this link. It may have expired: click the download button again.".into(),
                ),
                NexusFailure::Cancelled => None,
                NexusFailure::Error(message) => Some(message),
            })
    };
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
        },
        Err(error) => NexusInstalled {
            app_id,
            mod_id,
            cancelled: error.is_none(),
            name: None,
            missing: Vec::new(),
            error,
        },
    };
    let _ = app.emit("nexus-installed", event);
}

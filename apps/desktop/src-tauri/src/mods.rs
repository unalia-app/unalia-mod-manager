//! Commandes d'installation et de lancement : brancher le moteur sur
//! l'interface, avec la progression et l'état des jeux poussés par
//! événements (SPEC §6.4).
//!
//! Pendant qu'un jeu tourne, toute modification de ses mods est mise en
//! file et appliquée à sa fermeture (SPEC §3.6).

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

/// Délai pendant lequel un jeu lancé est « en démarrage » avant d'apparaître
/// dans les processus (Steam peut mettre un moment à le lancer).
const STARTING_FOR: Duration = Duration::from_secs(60);
/// Fréquence de la veille des processus.
const WATCH_EVERY: Duration = Duration::from_secs(2);
/// Délai pendant lequel un retrait peut être annulé : le stockage du mod
/// n'est libéré qu'après. Large devant le toast « Undo », qui se met en
/// pause au survol.
const UNDO_FOR: Duration = Duration::from_secs(120);

pub struct Mods {
    /// Une installation à la fois : elle garde la base pendant toute sa
    /// durée, les suivantes attendent leur tour (file d'attente).
    state: Mutex<ModState>,
    paths: Paths,
    /// Mods tout juste désinstallés, gardés pour « Undo » avec l'instant du
    /// retrait. Côté moteur : la webview ne renvoie qu'un identifiant,
    /// jamais un chemin.
    removed: SyncMutex<HashMap<(String, String), (Instant, InstalledMod)>>,
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
    /// Fichiers choisis d'un mod Nexus qui attendent leur lien `nxm://`
    /// (compte gratuit : un clic sur le site par fichier).
    chosen_files: SyncMutex<HashMap<(u32, String), Vec<u32>>>,
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
        let roaming = app.path().app_data_dir()?;
        // Avant toute ouverture : sous Windows, un fichier ouvert ne peut
        // pas être remplacé.
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

    /// Pourquoi l'appli ne doit pas se fermer maintenant (mise à jour) :
    /// une installation en cours ou en file, ou un jeu qui tourne.
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

/// Identifiant de l'appli avant son nom public, Unalia Mod Manager.
const OLD_IDENTIFIER: &str = "app.modalia.desktop";

/// Reprend le dossier de l'ancien identifiant, voisin de `dir`, tant que
/// `dir` n'existe pas encore. Un simple renommage : les liens physiques vers
/// les jeux restent valides, et les chemins enregistrés dans la base sont
/// ensuite réécrits (`rebase_store`).
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

/// Retire les mods de tous les jeux (désinstallation d'Unalia, données
/// effacées). Un jeu introuvable est sauté : il n'a plus de fichiers à
/// nettoyer.
pub fn remove_all(data: &Path, steam_dirs: HashMap<u32, PathBuf>) {
    let db = data.join("modalia.db");
    if !db.is_file() {
        return;
    }
    let Ok(mut state) = ModState::open(&db) else {
        return;
    };
    // Comme la bibliothèque : Steam l'emporte sur un ajout à la main.
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
    /// Mod Nexus à plusieurs fichiers (variantes, options) : l'interface
    /// les montre, l'installation reprend avec ceux choisis.
    Choose {
        files: Vec<nexus_source::ModFile>,
    },
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
        let next = launch::enqueue(&state.pending(&key).map_err(crate::support::logged)?, op);
        state.set_pending(&key, &next).map_err(crate::support::logged)?;
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
    files: Option<Vec<u32>>,
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
                // Les liens `nxm://` du site reprendront avec ce choix.
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
    /// Compte gratuit, plusieurs fichiers : celui-ci est arrivé, le suivant
    /// se télécharge sur le site, à cette adresse.
    NextFile(String),
    /// Plusieurs fichiers : à l'utilisateur de choisir.
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
                game.mods_before(),
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

/// Page de téléchargement du fichier à installer sur Nexus (le principal,
/// ou celui d'avant pour une ancienne version du jeu) : son bouton
/// « Slow download » ouvre le lien `nxm://` qui revient à Modalia.
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
    let value = installer::set_enabled(&mut state, &mods.paths, &game_key(app_id), &game.dir, &mod_id, enabled)
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
    let removed = installer::uninstall(&mut state, &mods.paths, &key, &game.dir, &mod_id)
        .map_err(|e| error_message(&e))?;
    let at = Instant::now();
    mods.removed
        .lock()
        .unwrap()
        .insert((key.clone(), mod_id.clone()), (at, removed));
    tauri::async_runtime::spawn(purge_later(app, key, mod_id, at));
    Ok(Outcome::Done { value: () })
}

/// Libère le stockage d'un mod retiré, si « Undo » n'a pas servi.
async fn purge_later(app: AppHandle, key: String, mod_id: String, at: Instant) {
    tokio::time::sleep(UNDO_FOR).await;
    let mods = app.state::<Mods>();
    let entry = (key, mod_id);
    {
        let mut removed = mods.removed.lock().unwrap();
        // Restauré, ou retiré à nouveau depuis : ce n'est plus ce retrait-là.
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
    let (_, removed) = mods
        .removed
        .lock()
        .unwrap()
        .remove(&(key, mod_id))
        .ok_or("This mod can't be restored anymore.")?;
    let mut state = mods.state.lock().await;
    installer::restore(&mut state, &mods.paths, &game.dir, removed).map_err(|e| error_message(&e))
}

/// Revient à une version précédente d'un mod (`package`, l'une de ses
/// `previousVersions`), sans rien retélécharger.
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
    installer::revert(&mut state, &mods.paths, &game_key(app_id), &game.dir, &mod_id, &package)
        .map_err(|e| error_message(&e))
}

/// Au démarrage : passe l'ancien stockage au stockage par empreinte (une
/// seule fois, sans rien retélécharger), puis fait le ménage. Les commandes
/// qui touchent aux mods attendent la fin.
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

// ------------------------------------------------- jeu réinstallé ou parti

/// Mods actifs effacés du dossier du jeu (jeu réinstallé, fichiers vérifiés
/// par Steam) : leurs noms.
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

/// Repose depuis le stockage les mods effacés du jeu, sans rien
/// retélécharger. Renvoie leur nombre.
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
    log::info!("put back {} mods in {key}", missing.len());
    Ok(missing.len())
}

/// Jeu désinstallé dont Unalia garde encore les mods dans son stockage.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForgottenGame {
    app_id: u32,
    name: String,
    mods: usize,
    /// Octets du stockage libérés en l'oubliant.
    size: u64,
}

/// Le jeu n'est plus nulle part : ni dans Steam, ni dans son dossier
/// ajouté à la main. Steam illisible : seuls les ajouts à la main comptent.
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

/// Oublie un jeu désinstallé et libère le stockage de ses mods.
#[tauri::command]
pub async fn forget_game(mods: State<'_, Mods>, app_id: u32) -> Result<(), String> {
    let steam = tauri::async_runtime::spawn_blocking(crate::steam_dirs)
        .await
        .map_err(crate::support::logged)?;
    let _fetching = mods.fetching.lock().await;
    let mut state = mods.state.lock().await;
    let manual = state.manual_games().map_err(crate::support::logged)?;
    // Revérifié ici : oublier un jeu encore là laisserait ses mods dans son
    // dossier sans plus aucun moyen de les retirer.
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

/// Retire un jeu ajouté à la main : ses mods quittent son dossier, qui
/// retrouve son état d'origine, puis il disparaît d'Unalia avec ses
/// réglages et son stockage. Le jeu lui-même reste installé. Renvoie le
/// nombre de mods retirés.
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
    // Plus d'« Undo » possible pour les mods retirés juste avant.
    mods.removed.lock().unwrap().retain(|(game, _), _| *game != key);
    log::info!("removed {key} and its {count} mods");
    Ok(count)
}

/// Ce qu'une remise à zéro retirerait du jeu.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetPreview {
    /// Mods installés par Unalia.
    mods: usize,
    /// Fichiers de mods posés par d'autres outils.
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

/// Remet le jeu à zéro côté mods : ceux d'Unalia le quittent, ce que
/// d'autres outils ont laissé part à la corbeille s'il est dans `foreign`
/// (cochés dans la fenêtre). Les fichiers du jeu et les sauvegardes ne
/// sont pas touchés.
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
    // Plus d'« Undo » possible pour les mods retirés juste avant.
    mods.removed.lock().unwrap().retain(|(game, _), _| *game != key);
    log::info!(
        "reset {key}: {} mods, {} leftovers from other tools, {} left",
        reset.mods,
        reset.foreign,
        reset.left.len()
    );
    Ok(reset)
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
            PendingOp::Install { mod_id } => run_install(app, app_id, game, mod_id)
                .await
                .map(|_| ())
                .map_err(|e| error_message(&e)),
            PendingOp::SetEnabled { mod_id, enabled } => {
                let mut state = mods.state.lock().await;
                installer::set_enabled(&mut state, &mods.paths, &key, &game.dir, mod_id, *enabled)
                    .map(|_| ())
                    .map_err(|e| error_message(&e))
            }
            PendingOp::Uninstall { mod_id } => {
                // Pas de « Undo » pour un retrait appliqué à la fermeture du
                // jeu : le stockage est libéré tout de suite.
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
    let _ = app.emit(
        "pending-applied",
        AppliedEvent {
            app_id,
            applied,
            failed,
        },
    );
}

/// Message principal (ce qui bloque, puis quoi faire), puis la chaîne
/// technique après un retour à la ligne : l'interface affiche la première
/// ligne et replie le reste.
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
    // Erreur montrée à l'utilisateur : notée aussi dans le journal.
    log::warn!("{}", message.replace('\n', " | "));
    message
}

// ---------------------------------------------------------------- Nexus

/// Réglage où vit la clé API Nexus de l'utilisateur.
const NEXUS_KEY: &str = "nexus_api_key";

/// Dernier compte reconnu par Nexus : affiché tel quel hors ligne.
const NEXUS_ACCOUNT: &str = "nexus_account";

async fn remember_account(mods: &Mods, account: Option<&Account>) {
    let value = account.and_then(|a| serde_json::to_string(a).ok());
    let _ = mods.state.lock().await.set_setting(NEXUS_ACCOUNT, value.as_deref());
}

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
        log::warn!("couldn't register nxm:// links: {error}");
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
    /// Clé enregistrée mais Nexus injoignable : `account` est le dernier
    /// compte reconnu (absent s'il n'a jamais été vérifié).
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
        .map_err(crate::support::logged)?;
    remember_account(mods, Some(&account)).await;
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
    remember_account(&mods, None).await;
    // Vide plutôt qu'absent : une déconnexion voulue tient, même en dev
    // avec une clé dans le `.env`.
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
    /// Annulée par l'utilisateur (pas une erreur à afficher).
    cancelled: bool,
    /// Nom du mod installé, s'il l'a été.
    name: Option<String>,
    missing: Vec<Requirement>,
    error: Option<String>,
    /// Fichier suivant d'un choix multiple : sa page de téléchargement,
    /// déjà ouverte dans le navigateur.
    website: Option<String>,
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
                website: None,
            },
        );
        return;
    };
    let mods = app.state::<Mods>();
    // Un des fichiers choisis dans Unalia : l'installation reprend avec
    // tout le choix.
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
    // Fichier suivant : sa page s'ouvre, l'interface attend son lien.
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

//! Commandes d'installation : brancher l'installateur du moteur sur
//! l'interface, avec la progression poussée par événements (SPEC §6.4).

use std::collections::HashMap;
use std::sync::Mutex as SyncMutex;

use mod_engine::installer::{self, Installer, Paths, Progress, Target};
use mod_engine::state::{InstalledMod, State as ModState};
use serde::Serialize;
use tauri::async_runtime::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::Library;
use crate::catalog::Catalog;

pub struct Mods {
    /// Une installation à la fois : elle garde la base pendant toute sa
    /// durée, les suivantes attendent leur tour (file d'attente).
    state: Mutex<ModState>,
    paths: Paths,
    /// Mods tout juste désinstallés, gardés pour « Undo ». Côté moteur : la
    /// webview ne renvoie qu'un identifiant, jamais un chemin.
    removed: SyncMutex<HashMap<(String, String), InstalledMod>>,
}

impl Mods {
    pub fn open(app: &AppHandle) -> Result<Self, Box<dyn std::error::Error>> {
        let data = app.path().app_data_dir()?;
        let cache = app.path().app_cache_dir()?;
        Ok(Self {
            state: Mutex::new(ModState::open(&data.join("modalia.db"))?),
            paths: Paths {
                store: data.join("mods"),
                downloads: cache.join("downloads"),
            },
            removed: SyncMutex::default(),
        })
    }
}

fn game_key(app_id: u32) -> String {
    format!("steam:{app_id}")
}

const UNKNOWN_GAME: &str = "This game isn't in your library anymore. Go back home to rescan it.";

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

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallEvent {
    app_id: u32,
    mod_id: String,
    progress: Progress,
}

/// Installe un mod et ses dépendances. La progression arrive par
/// l'événement `install-progress`.
#[tauri::command]
pub async fn install_mod(
    app: AppHandle,
    library: State<'_, Library>,
    catalog: State<'_, Catalog>,
    mods: State<'_, Mods>,
    app_id: u32,
    mod_id: String,
) -> Result<Vec<InstalledMod>, String> {
    let game = library.game(app_id).ok_or(UNKNOWN_GAME)?;
    let games_catalog = catalog.games(&app).await;
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

    let mut state = mods.state.lock().await;
    // Pas plus d'un événement par 1 % : l'interface n'en a pas besoin.
    let mut last = None;
    let result = installer
        .install(&mut state, &target, &mod_id, |progress| {
            let step = match &progress {
                Progress::Downloading {
                    index,
                    received,
                    size,
                    ..
                } => Some((
                    *index,
                    size.map(|s| received * 100 / s.max(1))
                        .unwrap_or(*received >> 16),
                )),
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
                    mod_id: mod_id.clone(),
                    progress,
                },
            );
        })
        .await;
    result.map_err(|e| error_message(&e))
}

#[tauri::command]
pub async fn set_mod_enabled(
    library: State<'_, Library>,
    mods: State<'_, Mods>,
    app_id: u32,
    mod_id: String,
    enabled: bool,
) -> Result<InstalledMod, String> {
    let game = library.game(app_id).ok_or(UNKNOWN_GAME)?;
    let mut state = mods.state.lock().await;
    installer::set_enabled(&mut state, &game_key(app_id), &game.dir, &mod_id, enabled)
        .map_err(|e| error_message(&e))
}

#[tauri::command]
pub async fn uninstall_mod(
    library: State<'_, Library>,
    mods: State<'_, Mods>,
    app_id: u32,
    mod_id: String,
) -> Result<(), String> {
    let game = library.game(app_id).ok_or(UNKNOWN_GAME)?;
    let key = game_key(app_id);
    let mut state = mods.state.lock().await;
    let removed = installer::uninstall(&mut state, &key, &game.dir, &mod_id)
        .map_err(|e| error_message(&e))?;
    mods.removed.lock().unwrap().insert((key, mod_id), removed);
    Ok(())
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

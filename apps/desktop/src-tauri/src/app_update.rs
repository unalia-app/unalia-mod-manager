//! Mises à jour de l'appli (updater Tauri). Cherchées au démarrage, jamais
//! installées sans l'accord de l'utilisateur. Le paquet téléchargé est
//! vérifié avec la clé publique de `tauri.conf.json` avant d'être lancé : un
//! fichier modifié en route ou sur le serveur est refusé.

use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::mods::Mods;

/// Dernière mise à jour trouvée : l'installation reprend celle-ci, sans
/// redemander au serveur.
#[derive(Default)]
pub struct AppUpdate(Mutex<Option<Update>>);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailableUpdate {
    version: String,
    /// Notes de version publiées avec la release.
    notes: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateProgress {
    downloaded: u64,
    total: Option<u64>,
}

/// Une version plus récente, ou `None` : l'appli est à jour.
#[tauri::command]
pub async fn check_app_update(
    app: AppHandle,
    pending: State<'_, AppUpdate>,
) -> Result<Option<AvailableUpdate>, String> {
    let update = app
        .updater()
        .map_err(failed("couldn't check for updates, check your connection and try again"))?
        .check()
        .await
        .map_err(failed("couldn't check for updates, check your connection and try again"))?;
    let available = update.as_ref().map(|u| AvailableUpdate {
        version: u.version.clone(),
        notes: u.body.clone().filter(|b| !b.trim().is_empty()),
    });
    if let Some(update) = &update {
        log::info!("update available: {}", update.version);
    }
    *pending.0.lock().unwrap() = update;
    Ok(available)
}

/// Télécharge et installe la mise à jour trouvée, puis relance l'appli.
/// Sous Windows, l'installeur ferme l'appli lui-même. Refusé tant qu'un mod
/// s'installe ou qu'un jeu tourne : ses changements en attente seraient
/// appliqués trop tard.
#[tauri::command]
pub async fn install_app_update(
    app: AppHandle,
    mods: State<'_, Mods>,
    pending: State<'_, AppUpdate>,
) -> Result<(), String> {
    if let Some(reason) = mods.busy() {
        return Err(reason.to_owned());
    }
    let update = pending
        .0
        .lock()
        .unwrap()
        .clone()
        .ok_or("no update to install, check again")?;
    log::info!("installing update {}", update.version);
    let mut downloaded = 0u64;
    update
        .download_and_install(
            |chunk, total| {
                downloaded += chunk as u64;
                let _ = app.emit("app-update-progress", UpdateProgress { downloaded, total });
            },
            || {},
        )
        .await
        .map_err(failed("couldn't install the update"))?;
    // macOS et Linux : la nouvelle version est en place, il faut relancer.
    app.restart();
}

/// Erreur du updater : un message lisible, la cause technique en dessous.
fn failed(message: &'static str) -> impl Fn(tauri_plugin_updater::Error) -> String {
    move |error| crate::support::logged(format!("{message}\n{error}"))
}

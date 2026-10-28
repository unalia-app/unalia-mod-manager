//! App updates (Tauri updater). Checked at startup, never installed
//! without the user's consent. The downloaded package is verified with the
//! public key of `tauri.conf.json` before it runs: a file modified on the
//! way or on the server is refused.

use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::mods::Mods;

/// Last update found: the install reuses this one, without asking the
/// server again.
#[derive(Default)]
pub struct AppUpdate(Mutex<Option<Update>>);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailableUpdate {
    version: String,
    /// Release notes published with the release.
    notes: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateProgress {
    downloaded: u64,
    total: Option<u64>,
}

/// A newer version, or `None`: the app is up to date.
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

/// Downloads and installs the update found, then restarts the app. On
/// Windows, the installer closes the app itself. Refused while a mod is
/// installing or a game is running: its pending changes would be applied
/// too late.
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
    // macOS and Linux: the new version is in place, a restart is needed.
    app.restart();
}

/// Updater error: a readable message, the technical cause below.
fn failed(message: &'static str) -> impl Fn(tauri_plugin_updater::Error) -> String {
    move |error| crate::support::logged(format!("{message}\n{error}"))
}

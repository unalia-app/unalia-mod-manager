//! App log and diagnostic report: enough to understand a reported bug,
//! without the user having to look for a file.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use tauri_plugin_log::{RotationStrategy, Target, TargetKind};
use tauri_plugin_opener::OpenerExt;

/// Name of the log file, in the app's log folder.
const LOG_NAME: &str = "unalia";

/// Log lines attached to the report: the last ones, the most useful.
const TAIL_LINES: usize = 150;

/// Log to a file (500 KB, the previous one kept once), and to the terminal
/// in dev.
pub fn log_plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri_plugin_log::Builder::new()
        .clear_targets()
        .targets([
            Target::new(TargetKind::LogDir {
                file_name: Some(LOG_NAME.into()),
            }),
            Target::new(TargetKind::Stdout),
        ])
        .level(log::LevelFilter::Info)
        .max_file_size(500_000)
        .rotation_strategy(RotationStrategy::KeepOne)
        .build()
}

/// A crash ends up in the log, with the code location.
pub fn log_panics() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("panic: {info}");
        previous(info);
    }));
}

/// Error sent back to the UI, logged on the way.
pub fn logged(error: impl std::fmt::Display) -> String {
    let message = error.to_string();
    log::warn!("{message}");
    message
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostics {
    /// System and version (e.g. "Windows 11 Home 10.0.26200").
    os: String,
    arch: &'static str,
    /// End of the log, personal paths and keys masked.
    log: String,
}

#[tauri::command]
pub fn diagnostics(app: AppHandle) -> Diagnostics {
    let info = os_info::get();
    let os = match info.edition() {
        // "Core" is the internal name of the Home edition.
        Some(edition) => format!("{} {}", edition.replace(" Core", " Home"), info.version()),
        None => format!("{} {}", info.os_type(), info.version()),
    };
    let log = log_file(&app)
        .and_then(|file| fs::read_to_string(file).ok())
        .map(|text| {
            let lines: Vec<&str> = text.lines().collect();
            let tail = &lines[lines.len().saturating_sub(TAIL_LINES)..];
            anonymize(&tail.join("\n"))
        })
        .unwrap_or_default();
    Diagnostics {
        os,
        arch: std::env::consts::ARCH,
        log,
    }
}

/// Opens the log folder in the file explorer.
#[tauri::command]
pub fn open_logs_folder(app: AppHandle) -> Result<(), String> {
    let dir = app.path().app_log_dir().map_err(logged)?;
    fs::create_dir_all(&dir).map_err(logged)?;
    app.opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(logged)
}

/// License texts shipped with the app (`bundle.resources` of
/// `tauri.conf.json`). As `.txt`: Notepad opens them on every PC.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum License {
    /// GPL v3, celle de l'appli.
    App,
    /// The dependencies' licenses, and UnRAR's.
    ThirdParty,
}

/// Opens a license text in the system's text editor.
#[tauri::command]
pub fn open_license(app: AppHandle, license: License) -> Result<(), String> {
    let name = match license {
        License::App => "LICENSE.txt",
        License::ThirdParty => "THIRD_PARTY_LICENSES.txt",
    };
    let file = app.path().resource_dir().map_err(logged)?.join(name);
    app.opener()
        .open_path(file.to_string_lossy(), None::<&str>)
        .map_err(logged)
}

/// Uninstaller placed by the NSIS installer next to the exe. Missing in dev.
fn uninstaller() -> Option<PathBuf> {
    let file = std::env::current_exe()
        .ok()?
        .parent()?
        .join("uninstall.exe");
    file.is_file().then_some(file)
}

/// The installed app can uninstall itself from Settings.
#[tauri::command]
pub fn can_uninstall() -> bool {
    cfg!(windows) && uninstaller().is_some()
}

/// Opens the Windows uninstaller then closes the app: it refuses to go on
/// while the app runs. Its "delete the data" box also removes the mods
/// from the games (`--remove-all-mods`, `windows/hooks.nsh`).
#[tauri::command]
pub fn uninstall_app(app: AppHandle) -> Result<(), String> {
    let file = uninstaller().ok_or_else(|| {
        "The uninstaller is missing. Use Windows Settings > Apps instead.".to_owned()
    })?;
    std::process::Command::new(&file).spawn().map_err(logged)?;
    log::info!("uninstaller started, closing the app");
    app.exit(0);
    Ok(())
}

fn log_file(app: &AppHandle) -> Option<PathBuf> {
    Some(app.path().app_log_dir().ok()?.join(format!("{LOG_NAME}.log")))
}

/// Removes what identifies the user: their home folder (Windows account
/// name) and the keys of `nxm://` links.
fn anonymize(text: &str) -> String {
    let mut text = text.to_owned();
    for var in ["USERPROFILE", "HOME"] {
        if let Some(home) = std::env::var_os(var).map(|h| h.to_string_lossy().into_owned())
            && home.len() > 3
        {
            text = text.replace(&home, "~").replace(&home.replace('\\', "/"), "~");
        }
    }
    redact_params(&text, &["key=", "expires=", "apikey="])
}

/// Replaces the value of each listed parameter (up to the next `&`, space
/// or quote) with `…`.
fn redact_params(text: &str, params: &[&str]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some((at, param)) = params
        .iter()
        .filter_map(|p| rest.find(p).map(|i| (i, *p)))
        .min_by_key(|(i, _)| *i)
    {
        let value_start = at + param.len();
        out.push_str(&rest[..value_start]);
        let value_len = rest[value_start..]
            .find(|c: char| c == '&' || c == '"' || c == '\'' || c.is_whitespace())
            .unwrap_or(rest.len() - value_start);
        if value_len > 0 {
            out.push('…');
        }
        rest = &rest[value_start + value_len..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::redact_params;

    #[test]
    fn redacts_link_keys() {
        assert_eq!(
            redact_params(
                "nxm://cyberpunk2077/mods/1/files/2?key=abc123&expires=999&user_id=5 done",
                &["key=", "expires="]
            ),
            "nxm://cyberpunk2077/mods/1/files/2?key=…&expires=…&user_id=5 done"
        );
        assert_eq!(redact_params("no secrets here", &["key="]), "no secrets here");
    }
}

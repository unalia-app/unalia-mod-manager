//! Journal de l'appli et rapport de diagnostic : de quoi comprendre un bug
//! signalé, sans que l'utilisateur ait à chercher un fichier.

use std::fs;
use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_log::{RotationStrategy, Target, TargetKind};
use tauri_plugin_opener::OpenerExt;

/// Nom du fichier de journal, dans le dossier des logs de l'appli.
const LOG_NAME: &str = "unalia";

/// Lignes du journal jointes au rapport : les dernières, les plus utiles.
const TAIL_LINES: usize = 150;

/// Journal dans un fichier (500 Ko, l'ancien gardé une fois), et dans le
/// terminal en dev.
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

/// Un plantage finit dans le journal, avec l'endroit du code.
pub fn log_panics() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("panic: {info}");
        previous(info);
    }));
}

/// Erreur renvoyée à l'interface, notée au passage dans le journal.
pub fn logged(error: impl std::fmt::Display) -> String {
    let message = error.to_string();
    log::warn!("{message}");
    message
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostics {
    /// Système et version (ex. « Windows 11 Home 10.0.26200 »).
    os: String,
    arch: &'static str,
    /// Fin du journal, chemins personnels et clés masqués.
    log: String,
}

#[tauri::command]
pub fn diagnostics(app: AppHandle) -> Diagnostics {
    let info = os_info::get();
    let os = match info.edition() {
        // « Core » est le nom interne de l'édition Famille (Home).
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

/// Ouvre le dossier des logs dans l'explorateur de fichiers.
#[tauri::command]
pub fn open_logs_folder(app: AppHandle) -> Result<(), String> {
    let dir = app.path().app_log_dir().map_err(logged)?;
    fs::create_dir_all(&dir).map_err(logged)?;
    app.opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(logged)
}

fn log_file(app: &AppHandle) -> Option<PathBuf> {
    Some(app.path().app_log_dir().ok()?.join(format!("{LOG_NAME}.log")))
}

/// Retire ce qui identifie l'utilisateur : son dossier personnel (nom de
/// compte Windows) et les clés des liens `nxm://`.
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

/// Remplace la valeur de chaque paramètre listé (jusqu'au prochain `&`,
/// espace ou guillemet) par `…`.
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

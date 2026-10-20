//! Brique **detect** : lister les jeux Steam installés et leur moteur.
//!
//! On lit uniquement les fichiers de Steam (`libraryfolders.vdf`, puis les
//! `appmanifest_*.acf` de chaque bibliothèque) : aucun parcours disque lourd.
//!
//! Un problème local (bibliothèque débranchée, manifeste corrompu, jeu
//! désinstallé) n'interrompt jamais le scan : l'élément concerné est ignoré.
//! Seule l'absence du dossier `steamapps` à la racine est une erreur.

mod engine;
pub mod vdf;

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

pub use engine::{Engine, detect_engine};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamGame {
    pub app_id: u32,
    pub name: String,
    /// Dossier du jeu : `<library>/steamapps/common/<installdir>`.
    pub install_dir: PathBuf,
    /// Bibliothèque Steam qui contient le jeu.
    pub library: PathBuf,
    pub engine: Engine,
}

#[derive(Debug, thiserror::Error)]
pub enum DetectError {
    #[error("no Steam installation in {0}")]
    SteamNotFound(PathBuf),
}

/// Cherche le dossier d'installation de Steam aux emplacements habituels.
pub fn find_steam_root() -> Option<PathBuf> {
    steam_root_candidates()
        .into_iter()
        .find(|p| p.join("steamapps").is_dir())
}

#[cfg(windows)]
fn steam_root_candidates() -> Vec<PathBuf> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

    let mut out = Vec::new();
    let registry = [
        (HKEY_CURRENT_USER, r"Software\Valve\Steam", "SteamPath"),
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\WOW6432Node\Valve\Steam",
            "InstallPath",
        ),
        (HKEY_LOCAL_MACHINE, r"SOFTWARE\Valve\Steam", "InstallPath"),
    ];
    for (hive, key, value) in registry {
        if let Ok(path) = RegKey::predef(hive)
            .open_subkey(key)
            .and_then(|k| k.get_value::<String, _>(value))
        {
            out.push(PathBuf::from(path));
        }
    }
    if let Some(pf) = std::env::var_os("ProgramFiles(x86)") {
        out.push(PathBuf::from(pf).join("Steam"));
    }
    out.push(PathBuf::from(r"C:\Program Files (x86)\Steam"));
    out
}

#[cfg(target_os = "macos")]
fn steam_root_candidates() -> Vec<PathBuf> {
    home()
        .map(|h| vec![h.join("Library/Application Support/Steam")])
        .unwrap_or_default()
}

#[cfg(all(unix, not(target_os = "macos")))]
fn steam_root_candidates() -> Vec<PathBuf> {
    let Some(h) = home() else {
        return Vec::new();
    };
    vec![
        h.join(".steam/steam"),
        h.join(".local/share/Steam"),
        h.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"),
    ]
}

#[cfg(unix)]
fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Liste les jeux installés de toutes les bibliothèques connues de Steam,
/// triés par nom.
pub fn scan_steam(steam_root: &Path) -> Result<Vec<SteamGame>, DetectError> {
    let steamapps = steam_root.join("steamapps");
    if !steamapps.is_dir() {
        return Err(DetectError::SteamNotFound(steam_root.to_path_buf()));
    }

    let mut seen_apps = HashSet::new();
    let mut games = Vec::new();
    for library in library_folders(steam_root) {
        for game in scan_library(&library) {
            if seen_apps.insert(game.app_id) {
                games.push(game);
            }
        }
    }
    games.sort_by_cached_key(|g| (g.name.to_lowercase(), g.app_id));
    Ok(games)
}

/// Bibliothèques déclarées dans `libraryfolders.vdf`, racine de Steam en
/// tête, sans doublon ni dossier absent.
pub fn library_folders(steam_root: &Path) -> Vec<PathBuf> {
    let mut candidates = vec![steam_root.to_path_buf()];
    let vdf_path = steam_root.join("steamapps").join("libraryfolders.vdf");
    if let Some(doc) = fs::read_to_string(&vdf_path)
        .ok()
        .and_then(|text| vdf::parse(&text).ok())
        && let Some(folders) = doc.obj("libraryfolders")
    {
        for (key, value) in folders.iter() {
            // Format actuel : "0" { "path" "..." }. Ancien format : "1" "D:\\...".
            // Les autres clés (ex. "TimeNextStatsReport") sont ignorées.
            if key.parse::<u32>().is_err() {
                continue;
            }
            let path = match value {
                vdf::Value::Obj(o) => o.str("path"),
                vdf::Value::Str(s) => Some(s.as_str()),
            };
            if let Some(path) = path {
                candidates.push(PathBuf::from(path));
            }
        }
    }

    let mut seen = HashSet::new();
    candidates
        .into_iter()
        .filter(|p| p.join("steamapps").is_dir())
        .filter(|p| seen.insert(fs::canonicalize(p).unwrap_or_else(|_| p.clone())))
        .collect()
}

fn scan_library(library: &Path) -> Vec<SteamGame> {
    let steamapps = library.join("steamapps");
    let Ok(read) = fs::read_dir(&steamapps) else {
        return Vec::new();
    };
    read.flatten()
        .map(|e| e.path())
        .filter(|p| is_app_manifest(p))
        .filter_map(|p| read_app_manifest(&p, library))
        .collect()
}

fn is_app_manifest(path: &Path) -> bool {
    path.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
        let n = n.to_ascii_lowercase();
        n.starts_with("appmanifest_") && n.ends_with(".acf")
    })
}

fn read_app_manifest(path: &Path, library: &Path) -> Option<SteamGame> {
    let text = fs::read_to_string(path).ok()?;
    let doc = vdf::parse(&text).ok()?;
    let state = doc.obj("AppState")?;

    let app_id = state.str("appid")?.parse().ok()?;
    let name = state.str("name")?.to_owned();
    let install_dir = library
        .join("steamapps")
        .join("common")
        .join(state.str("installdir").filter(|d| is_plain_dir_name(d))?);
    // Manifeste présent mais jeu supprimé ou jamais téléchargé.
    if !install_dir.is_dir() {
        return None;
    }

    Some(SteamGame {
        app_id,
        name,
        engine: detect_engine(&install_dir),
        install_dir,
        library: library.to_path_buf(),
    })
}

/// `installdir` doit désigner un dossier direct de `common/` : on refuse
/// tout ce qui pourrait en sortir (`..`, séparateurs, chemin absolu).
fn is_plain_dir_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', ':'])
}

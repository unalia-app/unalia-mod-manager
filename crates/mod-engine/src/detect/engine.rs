//! Identification du moteur d'un jeu par signature de fichiers.
//!
//! On ne lit que les noms des entrées, à deux ou trois niveaux de profondeur
//! au plus : la détection doit rester instantanée même sur 1 000 jeux.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    Unity,
    Unreal,
    Unknown,
}

pub fn detect_engine(game_dir: &Path) -> Engine {
    let root = entries(game_dir);
    if is_unity(&root) {
        Engine::Unity
    } else if is_unreal(&root) {
        Engine::Unreal
    } else {
        Engine::Unknown
    }
}

/// `UnityPlayer.dll` / `.so` à la racine, ou un dossier `<Jeu>_Data` qui
/// contient les fichiers propres à Unity (couvre aussi les vieilles versions
/// sans `UnityPlayer`). Sous macOS, le lecteur est dans le bundle `.app`.
fn is_unity(root: &[Entry]) -> bool {
    const PLAYERS: [&str; 2] = ["unityplayer.dll", "unityplayer.so"];
    const DATA_MARKERS: [&str; 5] = [
        "globalgamemanagers",
        "data.unity3d",
        "maindata",
        "managed",
        "il2cpp_data",
    ];

    if root
        .iter()
        .any(|e| !e.is_dir && PLAYERS.contains(&e.name.as_str()))
    {
        return true;
    }
    root.iter().filter(|e| e.is_dir).any(|e| {
        if e.name.ends_with("_data") {
            entries(&e.path)
                .iter()
                .any(|inner| DATA_MARKERS.contains(&inner.name.as_str()))
        } else if e.name.ends_with(".app") {
            find(&e.path, &["contents", "frameworks", "unityplayer.dylib"]).is_some()
        } else {
            false
        }
    })
}

/// Un dossier `Engine` à la racine et au moins une archive `.pak` dans
/// `<Projet>/Content/Paks`.
fn is_unreal(root: &[Entry]) -> bool {
    if !root.iter().any(|e| e.is_dir && e.name == "engine") {
        return false;
    }
    root.iter()
        .filter(|e| e.is_dir && e.name != "engine")
        .filter_map(|e| find(&e.path, &["content", "paks"]))
        .any(|paks| {
            entries(&paks)
                .iter()
                .any(|f| !f.is_dir && f.name.ends_with(".pak"))
        })
}

struct Entry {
    /// Nom en minuscules : les systèmes de fichiers Windows ignorent la casse,
    /// les jeux ne sont donc pas cohérents d'une version à l'autre.
    name: String,
    path: PathBuf,
    is_dir: bool,
}

fn entries(dir: &Path) -> Vec<Entry> {
    let Ok(read) = fs::read_dir(dir) else {
        return Vec::new();
    };
    read.flatten()
        .filter_map(|e| {
            let is_dir = e.file_type().ok()?.is_dir();
            Some(Entry {
                name: e.file_name().to_string_lossy().to_lowercase(),
                path: e.path(),
                is_dir,
            })
        })
        .collect()
}

/// Suit un chemin relatif en ignorant la casse de chaque segment.
fn find(dir: &Path, segments: &[&str]) -> Option<PathBuf> {
    let mut current = dir.to_path_buf();
    for segment in segments {
        current = entries(&current)
            .into_iter()
            .find(|e| e.name == *segment)?
            .path;
    }
    Some(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(files: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for file in files {
            let path = dir.path().join(file);
            if file.ends_with('/') {
                fs::create_dir_all(path).unwrap();
            } else {
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, b"").unwrap();
            }
        }
        dir
    }

    #[test]
    fn unity_player_ignores_case() {
        let dir = game(&["unityplayer.DLL", "Game.exe"]);
        assert_eq!(detect_engine(dir.path()), Engine::Unity);
    }

    #[test]
    fn unity_linux_player() {
        let dir = game(&["UnityPlayer.so", "Game.x86_64"]);
        assert_eq!(detect_engine(dir.path()), Engine::Unity);
    }

    #[test]
    fn unity_legacy_data_folder() {
        let dir = game(&["Old.exe", "Old_Data/mainData"]);
        assert_eq!(detect_engine(dir.path()), Engine::Unity);
    }

    #[test]
    fn unity_macos_bundle() {
        let dir = game(&["Game.app/Contents/Frameworks/UnityPlayer.dylib"]);
        assert_eq!(detect_engine(dir.path()), Engine::Unity);
    }

    #[test]
    fn data_folder_without_unity_markers_is_unknown() {
        let dir = game(&["Game.exe", "Game_Data/saves/slot1.sav"]);
        assert_eq!(detect_engine(dir.path()), Engine::Unknown);
    }

    #[test]
    fn unreal_needs_engine_and_pak() {
        let with_pak = game(&["Engine/Binaries/", "Proj/Content/Paks/Proj-Windows.pak"]);
        assert_eq!(detect_engine(with_pak.path()), Engine::Unreal);

        let without_engine = game(&["Proj/Content/Paks/Proj-Windows.pak"]);
        assert_eq!(detect_engine(without_engine.path()), Engine::Unknown);

        let without_pak = game(&["Engine/Binaries/", "Proj/Content/Paks/"]);
        assert_eq!(detect_engine(without_pak.path()), Engine::Unknown);
    }

    #[test]
    fn missing_dir_is_unknown() {
        assert_eq!(
            detect_engine(Path::new("/nonexistent/modalia/game")),
            Engine::Unknown
        );
    }
}

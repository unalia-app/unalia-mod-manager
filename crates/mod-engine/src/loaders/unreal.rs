//! **Unreal Engine** adapter (4 and 5), for games whose mods come from
//! Nexus: Phantom Blade Zero, Hogwarts Legacy…
//!
//! An Unreal game keeps its files in its project folder,
//! `<Project>/Content/Paks` and `<Project>/Binaries/Win64`. Its name is only
//! known once the game is installed: `find_project` finds it. Then, from the
//! archive content, never by a random guess:
//! - `.pak`, `.utoc`, `.ucas` (and `.sig`) go to `<Project>/Content/Paks/~mods/`;
//! - `LogicMods/…` (Blueprint mods) go to `<Project>/Content/Paks/LogicMods/`;
//! - a folder with `Scripts/main.lua` is a UE4SS mod, placed in
//!   `<binaries>/ue4ss/Mods/<mod>/`;
//! - `dwmapi.dll` and `ue4ss/` (UE4SS itself) go to `<binaries>/`;
//! - an archive that mirrors the game (`<Project>/…`, `<binaries>/…`) or the
//!   project (`Content/…`, `Binaries/…`) is placed as is.
//!
//! `<binaries>` is the folder of the game's executable (`find_binaries`):
//! `<Project>/Binaries/Win64`, except for games that keep it elsewhere.
//!
//! The rest (readme, screenshots) is not installed.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use super::{Loader, PackageKind, Placement};

/// Extensions of Unreal content packages (IoStore included).
const PAK_FILES: [&str; 4] = [".pak", ".utoc", ".ucas", ".sig"];

/// An Unreal game executable holds the whole engine: far above this. Below,
/// it is a launcher or a tool.
const GAME_EXE_SIZE: u64 = 50 * 1024 * 1024;

/// Unreal project folder: the one that holds both `Content/Paks` and
/// `Binaries/Win64` (not `Engine`, present in every game). Several
/// candidates: we don't guess.
pub fn find_project(game_dir: &Path) -> Option<String> {
    let mut found = fs::read_dir(game_dir).ok()?.flatten().filter_map(|entry| {
        let name = entry.file_name().to_string_lossy().into_owned();
        let dir = entry.path();
        (!name.eq_ignore_ascii_case("engine")
            && dir.join("Content").join("Paks").is_dir()
            && dir.join("Binaries").join("Win64").is_dir())
        .then_some(name)
    });
    let first = found.next()?;
    found.next().is_none().then_some(first)
}

/// Folder of the game's executable, relative to the game: UE4SS goes there
/// (the game loads the `dwmapi.dll` next to its executable). Usually
/// `<Project>/Binaries/Win64`. Some games start a small launcher and keep
/// the real executable elsewhere (Phantom Blade Zero:
/// `platform/steam/PBZ.exe`, about 500 MB): then the folder of the only
/// executable big enough to be the game. Several, or none: the usual
/// folder, we don't guess.
pub fn find_binaries(game_dir: &Path, project: &str) -> String {
    let usual = format!("{project}/Binaries/Win64");
    let mut big = Vec::new();
    big_exes(game_dir, "", 3, &mut big);
    match big.as_slice() {
        [only] if !only.eq_ignore_ascii_case(&usual) => only.clone(),
        _ => usual,
    }
}

/// Folders (relative, `/`) holding an executable of at least
/// `GAME_EXE_SIZE`, down to `depth` levels. `Engine/` is skipped: its tools
/// are not the game.
fn big_exes(dir: &Path, rel: &str, depth: usize, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            if depth > 0 && !(rel.is_empty() && name.eq_ignore_ascii_case("engine")) {
                let sub = if rel.is_empty() {
                    name
                } else {
                    format!("{rel}/{name}")
                };
                big_exes(&entry.path(), &sub, depth - 1, out);
            }
        } else if kind.is_file()
            && name.to_ascii_lowercase().ends_with(".exe")
            && entry.metadata().is_ok_and(|m| m.len() >= GAME_EXE_SIZE)
            && !out.iter().any(|d| d == rel)
        {
            out.push(rel.to_owned());
        }
    }
}

pub struct Unreal {
    /// Name of the project folder, e.g. `PhantomBladeZero`.
    pub project: String,
    /// Folder of the game's executable (`find_binaries`), e.g.
    /// `PhantomBladeZero/Binaries/Win64`.
    pub binaries: String,
}

impl Loader for Unreal {
    fn id(&self) -> &'static str {
        "unreal"
    }

    fn installed_manually(&self, _game_dir: &Path) -> bool {
        // No single loader: UE4SS is checked like a framework.
        false
    }

    fn place(&self, package_id: &str, _kind: &PackageKind, files: &[String]) -> Vec<Placement> {
        let skip = wrapper_depth(files, &self.project);
        let stripped: Vec<(&String, Vec<&str>)> = files
            .iter()
            .map(|f| (f, f.split('/').skip(skip).collect()))
            .collect();
        let all: HashSet<String> = stripped
            .iter()
            .map(|(_, parts)| parts.join("/").to_ascii_lowercase())
            .collect();
        let folder = folder_name(package_id);

        stripped
            .iter()
            .filter_map(|(source, parts)| {
                Some(Placement {
                    source: (*source).clone(),
                    target: self.target(parts, &all, &folder)?,
                })
            })
            .collect()
    }
}

impl Unreal {
    fn target(&self, parts: &[&str], all: &HashSet<String>, folder: &str) -> Option<String> {
        let project = &self.project;
        let win64 = &self.binaries;
        let (name, dirs) = parts.split_last()?;
        let lower = name.to_ascii_lowercase();
        let first = dirs.first().map(|d| d.to_ascii_lowercase());

        // 1. Game tree (the executable's folder, the project), then project
        //    tree.
        let bin: Vec<&str> = win64.split('/').collect();
        if dirs.len() >= bin.len()
            && dirs
                .iter()
                .zip(&bin)
                .all(|(d, b)| d.eq_ignore_ascii_case(b))
        {
            return Some(format!("{win64}/{}", parts[bin.len()..].join("/")));
        }
        if dirs
            .first()
            .is_some_and(|d| d.eq_ignore_ascii_case(project))
        {
            return Some(format!("{project}/{}", parts[1..].join("/")));
        }
        if matches!(first.as_deref(), Some("content" | "binaries")) {
            return Some(format!("{project}/{}", parts.join("/")));
        }

        // 2. UE4SS itself.
        if dirs.is_empty() && lower == "dwmapi.dll" {
            return Some(format!("{win64}/{name}"));
        }
        if first.as_deref() == Some("ue4ss") {
            return Some(format!("{win64}/{}", parts.join("/")));
        }

        // 3. UE4SS mod: the closest folder that holds `Scripts/main.lua`.
        //    At the package root: a folder named after the mod.
        for depth in (0..=dirs.len()).rev() {
            let base = dirs[..depth].join("/");
            let main = if base.is_empty() {
                "scripts/main.lua".to_owned()
            } else {
                format!("{}/scripts/main.lua", base.to_ascii_lowercase())
            };
            if all.contains(&main) {
                let mod_folder = if depth > 0 { dirs[depth - 1] } else { folder };
                return Some(format!(
                    "{win64}/ue4ss/Mods/{mod_folder}/{}",
                    parts[depth..].join("/")
                ));
            }
        }

        // 4. Blueprint mods, then content packages.
        if let Some(at) = dirs
            .iter()
            .position(|d| d.eq_ignore_ascii_case("logicmods"))
        {
            return Some(format!(
                "{project}/Content/Paks/LogicMods/{}",
                parts[at + 1..].join("/")
            ));
        }
        if PAK_FILES.iter().any(|e| lower.ends_with(e)) {
            return Some(format!("{project}/Content/Paks/~mods/{name}"));
        }
        None
    }
}

/// Wrapper folders to skip (`My Mod v1.2/…`), as for REDengine: we go down
/// as long as the whole package sits in a single folder that means nothing
/// to the game.
fn wrapper_depth(files: &[String], project: &str) -> usize {
    let meaningful = |segment: &str| {
        segment.eq_ignore_ascii_case(project)
            || [
                "content",
                "binaries",
                "ue4ss",
                "logicmods",
                "~mods",
                "scripts",
            ]
            .iter()
            .any(|s| segment.eq_ignore_ascii_case(s))
    };
    let mut depth = 0;
    loop {
        let mut first = None;
        for file in files {
            let parts: Vec<&str> = file.split('/').skip(depth).collect();
            if parts.len() < 2 {
                return depth;
            }
            match first {
                None => first = Some(parts[0]),
                Some(f) if f != parts[0] => return depth,
                Some(_) => {}
            }
            // `Mod/Scripts/main.lua`: `Mod` is the UE4SS mod name, keep it.
            if parts.len() >= 3 && parts[1].eq_ignore_ascii_case("scripts") {
                return depth;
            }
        }
        match first {
            Some(f) if !meaningful(f) => depth += 1,
            _ => return depth,
        }
    }
}

/// Safe, readable folder name taken from the mod name.
fn folder_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let cleaned = cleaned.trim_matches(['.', ' ']).to_owned();
    if cleaned.is_empty() {
        "mod".into()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: &str = "PhantomBladeZero";

    fn place(files: &[&str]) -> Vec<(String, String)> {
        let files: Vec<String> = files.iter().map(|f| f.to_string()).collect();
        let mut out: Vec<_> = Unreal {
            project: P.into(),
            binaries: format!("{P}/Binaries/Win64"),
        }
        .place("Cool Mod", &PackageKind::Mod, &files)
        .into_iter()
        .map(|p| (p.source, p.target))
        .collect();
        out.sort();
        out
    }

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        let mut v: Vec<_> = list
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect();
        v.sort();
        v
    }

    #[test]
    fn pak_mods_go_to_tilde_mods() {
        assert_eq!(
            place(&[
                "Better Swords/BetterSwords_P.pak",
                "Better Swords/BetterSwords_P.utoc",
                "Better Swords/BetterSwords_P.ucas",
                "Better Swords/readme.txt",
            ]),
            pairs(&[
                (
                    "Better Swords/BetterSwords_P.pak",
                    "PhantomBladeZero/Content/Paks/~mods/BetterSwords_P.pak"
                ),
                (
                    "Better Swords/BetterSwords_P.utoc",
                    "PhantomBladeZero/Content/Paks/~mods/BetterSwords_P.utoc"
                ),
                (
                    "Better Swords/BetterSwords_P.ucas",
                    "PhantomBladeZero/Content/Paks/~mods/BetterSwords_P.ucas"
                ),
            ])
        );
    }

    #[test]
    fn game_and_project_layouts_are_kept() {
        assert_eq!(
            place(&[
                "PhantomBladeZero/Content/Paks/~mods/A_P.pak",
                "Content/Paks/LogicMods/B.pak",
            ]),
            pairs(&[
                (
                    "PhantomBladeZero/Content/Paks/~mods/A_P.pak",
                    "PhantomBladeZero/Content/Paks/~mods/A_P.pak"
                ),
                (
                    "Content/Paks/LogicMods/B.pak",
                    "PhantomBladeZero/Content/Paks/LogicMods/B.pak"
                ),
            ])
        );
    }

    #[test]
    fn ue4ss_and_its_lua_mods_go_to_binaries() {
        assert_eq!(
            place(&["dwmapi.dll", "ue4ss/UE4SS.dll", "ue4ss/Mods/mods.txt"]),
            pairs(&[
                ("dwmapi.dll", "PhantomBladeZero/Binaries/Win64/dwmapi.dll"),
                (
                    "ue4ss/UE4SS.dll",
                    "PhantomBladeZero/Binaries/Win64/ue4ss/UE4SS.dll"
                ),
                (
                    "ue4ss/Mods/mods.txt",
                    "PhantomBladeZero/Binaries/Win64/ue4ss/Mods/mods.txt"
                ),
            ])
        );
        assert_eq!(
            place(&["NoStamina/Scripts/main.lua", "NoStamina/enabled.txt"]),
            pairs(&[
                (
                    "NoStamina/Scripts/main.lua",
                    "PhantomBladeZero/Binaries/Win64/ue4ss/Mods/NoStamina/Scripts/main.lua"
                ),
                (
                    "NoStamina/enabled.txt",
                    "PhantomBladeZero/Binaries/Win64/ue4ss/Mods/NoStamina/enabled.txt"
                ),
            ])
        );
        // Lua at the package root: a folder named after the mod.
        assert_eq!(
            place(&["Scripts/main.lua"]),
            pairs(&[(
                "Scripts/main.lua",
                "PhantomBladeZero/Binaries/Win64/ue4ss/Mods/Cool Mod/Scripts/main.lua"
            )])
        );
    }

    #[test]
    fn logic_mods_keep_their_folder() {
        assert_eq!(
            place(&["MyBP v2/LogicMods/MyBP.pak"]),
            pairs(&[(
                "MyBP v2/LogicMods/MyBP.pak",
                "PhantomBladeZero/Content/Paks/LogicMods/MyBP.pak"
            )])
        );
    }

    #[test]
    fn ue4ss_follows_an_executable_kept_outside_the_project() {
        let files: Vec<String> = [
            "dwmapi.dll",
            "ue4ss/UE4SS.dll",
            "NoStamina/Scripts/main.lua",
            "platform/steam/ue4ss/Mods/mods.txt",
            "Paks/A_P.pak",
        ]
        .iter()
        .map(|f| f.to_string())
        .collect();
        let mut out: Vec<(String, String)> = Unreal {
            project: "ProjectPBZ".into(),
            binaries: "platform/steam".into(),
        }
        .place("Cool Mod", &PackageKind::Mod, &files)
        .into_iter()
        .map(|p| (p.source, p.target))
        .collect();
        out.sort();
        assert_eq!(
            out,
            pairs(&[
                (
                    "NoStamina/Scripts/main.lua",
                    "platform/steam/ue4ss/Mods/NoStamina/Scripts/main.lua"
                ),
                ("Paks/A_P.pak", "ProjectPBZ/Content/Paks/~mods/A_P.pak"),
                ("dwmapi.dll", "platform/steam/dwmapi.dll"),
                (
                    "platform/steam/ue4ss/Mods/mods.txt",
                    "platform/steam/ue4ss/Mods/mods.txt"
                ),
                ("ue4ss/UE4SS.dll", "platform/steam/ue4ss/UE4SS.dll"),
            ])
        );
    }

    #[test]
    fn the_executable_folder_is_the_big_exe_one() {
        let dir = tempfile::tempdir().unwrap();
        let game = dir.path();
        let exe = |path: &str, size: u64| {
            let path = game.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            // Sparse: the size is there, not the bytes.
            std::fs::File::create(path).unwrap().set_len(size).unwrap();
        };
        std::fs::create_dir_all(game.join("ProjectPBZ/Binaries/Win64")).unwrap();
        // No big executable: the usual folder.
        assert_eq!(
            find_binaries(game, "ProjectPBZ"),
            "ProjectPBZ/Binaries/Win64"
        );

        // Phantom Blade Zero: a launcher in `steam/`, the game in
        // `platform/steam/`.
        exe("steam/PBZ.exe", 1024 * 1024);
        exe("platform/steam/PBZ.exe", GAME_EXE_SIZE + 1);
        exe("Engine/Binaries/Win64/UnrealEditor.exe", GAME_EXE_SIZE * 2);
        assert_eq!(find_binaries(game, "ProjectPBZ"), "platform/steam");

        // Two candidates: we don't guess.
        exe("platform/epic/PBZ.exe", GAME_EXE_SIZE + 1);
        assert_eq!(
            find_binaries(game, "ProjectPBZ"),
            "ProjectPBZ/Binaries/Win64"
        );
    }

    #[test]
    fn the_project_folder_is_found_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let game = dir.path();
        std::fs::create_dir_all(game.join("Engine/Binaries/Win64")).unwrap();
        std::fs::create_dir_all(game.join("Engine/Content/Paks")).unwrap();
        assert_eq!(find_project(game), None);
        std::fs::create_dir_all(game.join(P).join("Content/Paks")).unwrap();
        std::fs::create_dir_all(game.join(P).join("Binaries/Win64")).unwrap();
        assert_eq!(find_project(game).as_deref(), Some(P));
    }
}

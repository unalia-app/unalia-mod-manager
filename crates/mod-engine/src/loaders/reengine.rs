//! **RE Engine** adapter (Capcom: Monster Hunter Wilds, Resident Evil…).
//!
//! Mods load through REFramework (`dinput8.dll` at the game root), which
//! the profile describes as a framework (`games/auto/re-engine.toml`). The
//! files are sorted by their place in the archive, never by a guess:
//! - `reframework/` (Lua scripts in `autorun/`, plugins in `plugins/`) and
//!   `natives/` (loose files, Fluffy Mod Manager's layout) are placed as is,
//!   from wherever they sit in the archive. REFramework's own folders alone
//!   (`autorun/`, `plugins/`) go into `reframework/`;
//! - a `.pak` goes to `pak_mods/<mod>/`, where REFramework loads it, in the
//!   games where it can (`paks`). Elsewhere a pak only loads under a Capcom
//!   patch name, among the game's own patches: Unalia doesn't do it;
//! - a `.lua` at the archive root is a script (`reframework/autorun/`), a
//!   `.dll` at the archive root goes to the game root (REFramework itself,
//!   a DirectStorage upgrade…).
//!
//! The rest (readme, screenshots, Fluffy's `modinfo.ini`) is not installed.
//!
//! REFramework reads `natives/` only with its loose file loader on, off by
//! default: `set_loose_files` turns it on in its settings.

use std::fs;
use std::io;
use std::path::Path;

use super::redengine::folder_name;
use super::{Loader, PackageKind, Placement};

/// Folders an archive can mirror, placed as is.
const ROOTS: [&str; 3] = ["natives", "reframework", PAK_MODS];

/// Folders of REFramework that an archive can hold without it.
const SCRIPT_FOLDERS: [&str; 2] = ["autorun", "plugins"];

/// Where REFramework loads `.pak` mods, at the game root.
pub const PAK_MODS: &str = "pak_mods";

/// REFramework's settings, at the game root: `key=value` lines.
pub const CONFIG: &str = "re2_fw_config.txt";

/// Its loose file loader: off, `natives/` isn't read.
const LOOSE_FILES: &str = "LooseFileLoader_Enabled";

pub struct ReEngine {
    /// REFramework loads `.pak` mods from `pak_mods/` in this game.
    pub paks: bool,
}

impl Loader for ReEngine {
    fn id(&self) -> &'static str {
        "reengine"
    }

    fn installed_manually(&self, _game_dir: &Path) -> bool {
        // REFramework is a framework, checked on its own.
        false
    }

    fn place(&self, package_id: &str, _kind: &PackageKind, files: &[String]) -> Vec<Placement> {
        let skip = wrapper_depth(files);
        let folder = folder_name(package_id);
        files
            .iter()
            .filter_map(|source| {
                Some(Placement {
                    source: source.clone(),
                    target: self.target(source, skip, &folder)?,
                })
            })
            .collect()
    }
}

impl ReEngine {
    fn target(&self, source: &str, skip: usize, folder: &str) -> Option<String> {
        let parts: Vec<&str> = source.split('/').collect();
        let (name, dirs) = parts.split_last()?;
        let lower = name.to_ascii_lowercase();
        let at_root = parts.len() == skip + 1;

        // 1. A game folder, from where it starts.
        if let Some(at) = dirs.iter().position(|d| is_one_of(d, &ROOTS)) {
            let root = dirs[at].to_ascii_lowercase();
            if root == PAK_MODS && !self.paks {
                return None;
            }
            return Some(format!("{root}/{}", parts[at + 1..].join("/")));
        }
        // 2. REFramework's folders, without it.
        if let Some(at) = dirs.iter().position(|d| is_one_of(d, &SCRIPT_FOLDERS)) {
            return Some(format!(
                "reframework/{}/{}",
                dirs[at].to_ascii_lowercase(),
                parts[at + 1..].join("/")
            ));
        }
        // 3. Loose files.
        if lower.ends_with(".pak") {
            return self.paks.then(|| format!("{PAK_MODS}/{folder}/{name}"));
        }
        if at_root && lower.ends_with(".lua") {
            return Some(format!("reframework/autorun/{name}"));
        }
        if at_root && lower.ends_with(".dll") {
            return Some((*name).to_owned());
        }
        None
    }
}

/// Wrapper folders to skip: `My Mod v1.2/MyMod.lua`. We go down as long as
/// the whole package sits in a single folder that isn't one we place.
fn wrapper_depth(files: &[String]) -> usize {
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
        }
        match first {
            Some(f) if !is_one_of(f, &ROOTS) && !is_one_of(f, &SCRIPT_FOLDERS) => depth += 1,
            _ => return depth,
        }
    }
}

fn is_one_of(segment: &str, names: &[&str]) -> bool {
    names.iter().any(|n| n.eq_ignore_ascii_case(segment))
}

/// Is a file placed at `target` a loose file, read only with the loose file
/// loader on?
pub fn is_loose(target: &str) -> bool {
    target
        .split('/')
        .next()
        .is_some_and(|first| first.eq_ignore_ascii_case("natives"))
}

/// The loose file loader's value in REFramework's settings (`None`: not
/// there, off).
pub fn loose_files(game_dir: &Path) -> io::Result<Option<String>> {
    let text = match fs::read_to_string(game_dir.join(CONFIG)) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    Ok(text.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        (key.trim() == LOOSE_FILES).then(|| value.trim().to_owned())
    }))
}

/// Sets the loose file loader's value (`None`: removes the key, and the
/// settings file if Unalia made it for this key alone). The other lines
/// stay as REFramework wrote them.
pub fn set_loose_files(game_dir: &Path, value: Option<&str>) -> io::Result<()> {
    let path = game_dir.join(CONFIG);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = text
        .lines()
        .filter(|line| line.split_once('=').is_none_or(|(key, _)| key.trim() != LOOSE_FILES))
        .map(str::to_owned)
        .collect();
    if let Some(value) = value {
        lines.push(format!("{LOOSE_FILES}={value}"));
    }
    if lines.iter().all(|l| l.trim().is_empty()) {
        return match fs::remove_file(&path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    let tmp = path.with_extension("txt.unalia");
    fs::write(&tmp, lines.join(newline) + newline)?;
    fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(paks: bool, files: &[&str]) -> Vec<(String, String)> {
        let files: Vec<String> = files.iter().map(|f| f.to_string()).collect();
        ReEngine { paks }
            .place("Ver.R Body", &PackageKind::Mod, &files)
            .into_iter()
            .map(|p| (p.source, p.target))
            .collect()
    }

    fn targets(paks: bool, files: &[&str]) -> Vec<String> {
        place(paks, files).into_iter().map(|(_, t)| t).collect()
    }

    #[test]
    fn game_folders_are_placed_from_where_they_start() {
        assert_eq!(
            targets(
                true,
                &[
                    "Ver.R Body v2/natives/STM/Art/body.mesh.2109148288",
                    "Ver.R Body v2/modinfo.ini",
                    "Ver.R Body v2/screenshot.png",
                ]
            ),
            ["natives/STM/Art/body.mesh.2109148288"]
        );
        assert_eq!(
            targets(true, &["reframework/autorun/shop.lua", "reframework/data/shop.json"]),
            ["reframework/autorun/shop.lua", "reframework/data/shop.json"]
        );
        // Case from the archive, folder names as the game spells them.
        assert_eq!(targets(true, &["Natives/STM/a.tex"]), ["natives/STM/a.tex"]);
    }

    #[test]
    fn scripts_without_their_framework_folder_go_into_it() {
        assert_eq!(
            targets(true, &["Auto Collect/autorun/AutoCollect.lua", "Auto Collect/README.txt"]),
            ["reframework/autorun/AutoCollect.lua"]
        );
        assert_eq!(targets(true, &["AutoCollect.lua"]), ["reframework/autorun/AutoCollect.lua"]);
        assert_eq!(targets(true, &["Overlay/plugins/Overlay.dll"]), ["reframework/plugins/Overlay.dll"]);
        // A Lua file deeper than the root is a script's library, not a script.
        assert!(targets(true, &["Mod/lib/util.lua", "Mod/other/x.txt"]).is_empty());
    }

    #[test]
    fn a_dll_at_the_root_goes_to_the_game_root() {
        assert_eq!(targets(true, &["dinput8.dll", "reframework_revision.txt"]), ["dinput8.dll"]);
        assert_eq!(
            targets(true, &["DirectStorage 1.2.3/dstorage.dll", "DirectStorage 1.2.3/dstoragecore.dll"]),
            ["dstorage.dll", "dstoragecore.dll"]
        );
    }

    #[test]
    fn paks_go_to_pak_mods_only_where_reframework_loads_them() {
        assert_eq!(
            targets(true, &["Ver.R/re_chunk_000.pak.sub_000.pak.patch_002.pak"]),
            ["pak_mods/Ver.R Body/re_chunk_000.pak.sub_000.pak.patch_002.pak"]
        );
        assert_eq!(targets(true, &["pak_mods/Body/body.pak"]), ["pak_mods/Body/body.pak"]);
        assert!(targets(false, &["re_chunk_000.pak.patch_004.pak"]).is_empty());
        assert!(targets(false, &["pak_mods/body.pak"]).is_empty());
    }

    #[test]
    fn variants_keep_their_folders_in_the_sources() {
        // Fluffy options: one folder each, the same files in the game. The
        // installer asks which one (`variants`).
        let placed = place(
            true,
            &[
                "Body/Light/modinfo.ini",
                "Body/Light/natives/STM/body.tex",
                "Body/Dark/modinfo.ini",
                "Body/Dark/natives/STM/body.tex",
            ],
        );
        assert_eq!(
            placed,
            [
                ("Body/Light/natives/STM/body.tex".into(), "natives/STM/body.tex".into()),
                ("Body/Dark/natives/STM/body.tex".into(), "natives/STM/body.tex".into()),
            ]
        );
    }

    #[test]
    fn the_loose_file_loader_is_set_without_touching_the_other_settings() {
        let dir = tempfile::tempdir().unwrap();
        let game = dir.path();
        assert_eq!(loose_files(game).unwrap(), None);

        // Before the first launch: a file with this key alone, removed after.
        set_loose_files(game, Some("true")).unwrap();
        assert_eq!(fs::read_to_string(game.join(CONFIG)).unwrap(), "LooseFileLoader_Enabled=true\n");
        set_loose_files(game, None).unwrap();
        assert!(!game.join(CONFIG).exists());

        // Written by REFramework since: its lines and line endings stay.
        fs::write(
            game.join(CONFIG),
            "REFrameworkConfig_MenuKey=45\r\nLooseFileLoader_Enabled=false\r\nFreeCam_Enabled=false\r\n",
        )
        .unwrap();
        assert_eq!(loose_files(game).unwrap().as_deref(), Some("false"));
        set_loose_files(game, Some("true")).unwrap();
        assert_eq!(
            fs::read_to_string(game.join(CONFIG)).unwrap(),
            "REFrameworkConfig_MenuKey=45\r\nFreeCam_Enabled=false\r\nLooseFileLoader_Enabled=true\r\n"
        );
        assert!(is_loose("natives/STM/a.tex") && !is_loose("reframework/autorun/a.lua"));
    }
}

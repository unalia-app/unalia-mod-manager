//! Mod files placed **outside Unalia**: by hand, by Vortex, by Mod
//! Organizer… They block a clean install (`winhttp.dll` from an old
//! BepInEx, for example) and the user doesn't know what to delete.
//!
//! We don't guess: we only look at the known locations of the game's
//! loaders (its "footprint"), never at the game's own files. Whatever sits
//! there without coming from Unalia goes to the Recycle Bin, where it can be
//! recovered, and only what the user ticked.
//!
//! A common DLL name (`winhttp.dll`, `version.dll`…) can also come from the
//! game or from a program it needs: it is only `sure` next to the loader
//! that uses it, else its box stays unticked.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::Path;

use serde::Serialize;

use crate::loaders::unreal::find_project;

/// Footprint location found in the game, with what doesn't come from
/// Unalia.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Foreign {
    /// Path relative to the game, with `/`.
    pub path: String,
    pub dir: bool,
    /// Files that don't come from Unalia (0: empty folder left behind).
    pub files: usize,
    pub bytes: u64,
    /// Surely a loader leftover: ticked by default in "Reset mods".
    pub sure: bool,
}

/// Footprint location. `proof`: paths, any one of which proves the loader
/// when the name doesn't (empty: the name is enough).
struct Spot {
    path: String,
    proof: Vec<String>,
}

fn spots(list: &[(&str, &[&str])], prefix: &str) -> Vec<Spot> {
    list.iter()
        .map(|(path, proof)| Spot {
            path: format!("{prefix}{path}"),
            proof: proof.iter().map(|p| format!("{prefix}{p}")).collect(),
        })
        .collect()
}

/// Placed by Doorstop (BepInEx) next to its `winhttp.dll`.
const DOORSTOP: &[&str] = &["doorstop_config.ini", ".doorstop_version"];
const CET: &[&str] = &["bin/x64/plugins/cyber_engine_tweaks"];
const UE4SS: &[&str] = &["Binaries/Win64/ue4ss", "Binaries/Win64/UE4SS.dll"];

/// MelonLoader folders with names too common to remove without proof: only
/// when empty, or next to `MelonLoader/`.
const MELON_DIRS: [&str; 4] = ["Mods", "Plugins", "UserData", "UserLibs"];

/// Locations where this game's loaders (the profile's `loader`) put their
/// files and their mods' files, relative to the game.
fn footprint(loader: &str, game_dir: &Path) -> Vec<Spot> {
    let mut paths: Vec<Spot> = match loader {
        // BepInEx and MelonLoader: a leftover of one also blocks the other.
        "bepinex" | "bepisloader" | "melonloader" | "recursive-melonloader" => spots(
            &[
                ("BepInEx", &[]),
                ("winhttp.dll", DOORSTOP),
                ("doorstop_config.ini", &[]),
                (".doorstop_version", &[]),
                ("doorstop_libs", &[]),
                ("dotnet", DOORSTOP),
                ("MelonLoader", &[]),
                ("version.dll", &["MelonLoader"]),
            ],
            "",
        )
        .into_iter()
        .chain(
            MELON_DIRS
                .iter()
                .filter(|d| game_dir.join("MelonLoader").is_dir() || is_empty_dir(&game_dir.join(d)))
                .map(|d| Spot { path: d.to_string(), proof: Vec::new() }),
        )
        .collect(),
        "redengine" => spots(
            &[
                ("archive/pc/mod", &[]),
                ("bin/x64/plugins", &[]),
                ("bin/x64/version.dll", CET),
                ("bin/x64/winmm.dll", &["red4ext"]),
                ("bin/x64/global.ini", CET),
                ("mods", &[]),
                ("r6/scripts", &[]),
                ("r6/tweaks", &[]),
                ("r6/cache/modded", &[]),
                ("red4ext", &[]),
            ],
            "",
        ),
        "unreal" => match find_project(game_dir) {
            Some(project) => spots(
                &[
                    ("Binaries/Win64/ue4ss", &[]),
                    ("Binaries/Win64/Mods", &[]),
                    ("Binaries/Win64/dwmapi.dll", UE4SS),
                    ("Binaries/Win64/UE4SS.dll", &[]),
                    ("Binaries/Win64/UE4SS-settings.ini", &[]),
                    ("Content/Paks/~mods", &[]),
                    ("Content/Paks/LogicMods", &[]),
                ],
                &format!("{project}/"),
            ),
            None => Vec::new(),
        },
        _ => Vec::new(),
    };
    // Vortex traces at the root: its deployment manifest and its managed
    // folder marker.
    if let Ok(entries) = fs::read_dir(game_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let lower = name.to_ascii_lowercase();
            if (lower.starts_with("vortex.deployment") && lower.ends_with(".json"))
                || lower == "__folder_managed_by_vortex"
            {
                paths.push(Spot { path: name, proof: Vec::new() });
            }
        }
    }
    paths
}

/// What the footprint holds that doesn't come from Unalia. `ours`: files
/// placed by Unalia; `our_dirs`: folders it created (relative paths, `/`).
pub fn scan(
    loader: &str,
    game_dir: &Path,
    ours: &HashSet<String>,
    our_dirs: &HashSet<String>,
) -> Vec<Foreign> {
    let ours: HashSet<String> = ours.iter().map(|p| p.to_ascii_lowercase()).collect();
    let our_dirs: HashSet<String> = our_dirs.iter().map(|p| p.to_ascii_lowercase()).collect();
    footprint(loader, game_dir)
        .into_iter()
        .filter_map(|Spot { path, proof }| {
            let full = game_dir.join(&path);
            let meta = fs::symlink_metadata(&full).ok()?;
            // A proof placed by Unalia doesn't prove the other tool.
            let sure = proof.is_empty()
                || proof.iter().any(|p| {
                    let lower = p.to_ascii_lowercase();
                    !ours.contains(&lower)
                        && !our_dirs.contains(&lower)
                        && fs::symlink_metadata(game_dir.join(p)).is_ok()
                });
            if !meta.is_dir() {
                return (!ours.contains(&path.to_ascii_lowercase())).then_some(Foreign {
                    path,
                    dir: false,
                    files: 1,
                    bytes: meta.len(),
                    sure,
                });
            }
            let (mut files, mut bytes, mut any) = (0, 0, false);
            for (relative, size) in files_under(&full, &path) {
                any = true;
                if !ours.contains(&relative.to_ascii_lowercase()) {
                    files += 1;
                    bytes += size;
                }
            }
            // Empty folder that Unalia didn't create: another tool's leftover.
            let leftover = !any && !our_dirs.contains(&path.to_ascii_lowercase());
            (files > 0 || leftover).then_some(Foreign {
                path,
                dir: true,
                files,
                bytes,
                sure,
            })
        })
        .collect()
}

/// Removes these locations from the game with `discard` (the Recycle Bin,
/// [`to_recycle_bin`], outside tests). Returns the ones that couldn't go
/// (file held open by the game, permissions…).
pub fn remove(game_dir: &Path, found: &[Foreign], discard: impl Fn(&Path) -> io::Result<()>) -> Vec<String> {
    found
        .iter()
        .filter(|f| {
            let full = game_dir.join(&f.path);
            fs::symlink_metadata(&full).is_ok() && discard(&full).is_err()
        })
        .map(|f| f.path.clone())
        .collect()
}

/// System Recycle Bin: the user can recover a file from it.
pub fn to_recycle_bin(path: &Path) -> io::Result<()> {
    trash::delete(path).map_err(io::Error::other)
}

fn is_empty_dir(dir: &Path) -> bool {
    fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_none())
}

/// Files under `dir` (recursively), path relative to the game and size.
fn files_under(dir: &Path, prefix: &str) -> Vec<(String, u64)> {
    fn walk(dir: &Path, prefix: &str, out: &mut Vec<(String, u64)>) -> io::Result<()> {
        for entry in fs::read_dir(dir)?.flatten() {
            let name = format!("{prefix}/{}", entry.file_name().to_string_lossy());
            let meta = entry.metadata()?;
            // A junction counts as a file: we don't follow it.
            if meta.is_dir() && !meta.is_symlink() {
                walk(&entry.path(), &name, out)?;
            } else {
                out.push((name, meta.len()));
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    let _ = walk(dir, prefix, &mut out);
    out
}

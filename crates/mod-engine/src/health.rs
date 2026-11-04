//! Did the mod loaders start?
//!
//! After a game update, a loader can fail while the game still runs: its
//! mods do nothing and the player can't tell. Each loader writes a log next
//! to its files, and that log says it. One entry per family of loaders: a
//! game needs nothing in its profile.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::Serialize;

use crate::state::InstalledMod;

/// A loader that didn't start in the last session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoaderProblem {
    pub loader: String,
    /// Enabled mods that needed it.
    pub mods: Vec<String>,
}

enum Proof {
    /// A line only a successful start writes.
    Started(&'static [&'static str]),
    /// The loader retries as long as the game runs: its last lines say it
    /// still failed. A retry that worked goes on to write other lines.
    StillFailing(&'static [&'static str]),
}

struct Loader {
    name: &'static str,
    /// Its main file. Where a mod placed it gives the folder of the paths
    /// below, which can change with the game (UE4SS).
    marker: &'static str,
    /// `*` for a log with a new name each session.
    log: &'static str,
    mods_dir: &'static str,
    proof: Proof,
}

const LOADERS: &[Loader] = &[
    Loader {
        name: "UE4SS",
        marker: "UE4SS.dll",
        log: "UE4SS.log",
        mods_dir: "Mods/",
        proof: Proof::StillFailing(&["Was unable to find AOB", "AOB scans could not be completed"]),
    },
    Loader {
        name: "BepInEx",
        marker: "BepInEx/core/BepInEx.dll",
        log: "BepInEx/LogOutput.log",
        mods_dir: "BepInEx/plugins/",
        proof: Proof::Started(&["Chainloader started", "Chainloader startup complete"]),
    },
    Loader {
        name: "RED4ext",
        marker: "red4ext/RED4ext.dll",
        log: "red4ext/logs/red4ext-*.log",
        mods_dir: "red4ext/plugins/",
        proof: Proof::Started(&["RED4ext has been successfully initialized"]),
    },
];

/// Last lines read for `Proof::StillFailing`.
const TAIL: usize = 8;

/// The loaders of these mods that failed in the last session. To call once
/// the game closed: a running loader hasn't written everything yet.
pub fn loader_problems(mods: &[InstalledMod], game_dir: &Path) -> Vec<LoaderProblem> {
    let enabled: Vec<&InstalledMod> = mods.iter().filter(|m| m.enabled).collect();
    LOADERS
        .iter()
        .filter_map(|loader| {
            let (owner, root) = enabled.iter().find_map(|m| {
                let root = m.deployment.files.iter().find_map(|f| root_of(f, loader.marker))?;
                Some((m.id.as_str(), root))
            })?;
            if !failed(loader, &game_dir.join(root)) {
                return None;
            }
            let dir = format!("{root}{}", loader.mods_dir);
            let names: Vec<String> = enabled
                .iter()
                .filter(|m| m.id != owner && m.deployment.files.iter().any(|f| f.starts_with(&dir)))
                .map(|m| m.name.clone())
                .collect();
            // Nothing was lost: no need to worry the player.
            (!names.is_empty()).then(|| LoaderProblem {
                loader: loader.name.to_owned(),
                mods: names,
            })
        })
        .collect()
}

/// The folder a loader's paths start from, with its final `/`.
fn root_of<'a>(file: &'a str, marker: &str) -> Option<&'a str> {
    let root = file.strip_suffix(marker)?;
    (root.is_empty() || root.ends_with('/')).then_some(root)
}

fn failed(loader: &Loader, root: &Path) -> bool {
    // Gone for a session without mods.
    let Ok(installed) = modified(&root.join(loader.marker)) else {
        return false;
    };
    let Some((log, written)) = newest_log(root, loader.log) else {
        return false;
    };
    // Older than the loader: it says nothing about this version, which
    // hasn't run yet.
    if written < installed {
        return false;
    }
    let Ok(bytes) = fs::read(log) else {
        return false;
    };
    let text = String::from_utf8_lossy(&bytes);
    if text.trim().is_empty() {
        return false;
    }
    match loader.proof {
        Proof::Started(lines) => !lines.iter().any(|l| text.contains(l)),
        Proof::StillFailing(lines) => text
            .lines()
            .rev()
            .filter(|l| !l.trim().is_empty())
            .take(TAIL)
            .any(|l| lines.iter().any(|f| l.contains(f))),
    }
}

fn modified(path: &Path) -> std::io::Result<SystemTime> {
    fs::metadata(path)?.modified()
}

fn newest_log(root: &Path, pattern: &str) -> Option<(PathBuf, SystemTime)> {
    let Some((prefix, suffix)) = pattern.split_once('*') else {
        let path = root.join(pattern);
        let written = modified(&path).ok()?;
        return Some((path, written));
    };
    let (folder, start) = prefix.rsplit_once('/').unwrap_or(("", prefix));
    fs::read_dir(root.join(folder))
        .ok()?
        .flatten()
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name.starts_with(start) && name.ends_with(suffix)
        })
        .filter_map(|e| Some((e.path(), e.metadata().ok()?.modified().ok()?)))
        .max_by_key(|(_, written)| *written)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::deploy::Deployment;

    /// What UE4SS wrote after the Phantom Blade Zero update of 2026-11-04.
    const UE4SS_FAILED: &str = "\
[12:36:18] [PS] Found EngineVersion: 5.7
[12:36:55] Lua Scan attempt 346 (Phase 2)
[12:36:55] AOB scans could not be completed because of the following reasons:
Was unable to find AOB for 'GNatives' via Lua script

Was unable to find AOB for 'GUObjectArray' via Lua script

[12:36:55] Lua Scan attempt 347 (Phase 2)
[12:36:55] AOB scans could not be completed because of the following reasons:
Was unable to find AOB for 'GNatives' via Lua script

Was unable to find AOB for 'GUObjectArray' via Lua script

";

    fn installed(id: &str, files: &[&str]) -> InstalledMod {
        InstalledMod {
            game: "steam:4115450".into(),
            id: id.into(),
            name: id.into(),
            author: String::new(),
            version: "1.0".into(),
            icon_url: None,
            adult: false,
            loader_root: None,
            enabled: true,
            explicit: true,
            sha256: String::new(),
            installed_at: 0,
            package: String::new(),
            legacy_store_dir: String::new(),
            files: Vec::new(),
            source: String::new(),
            priority: 0,
            covered_by: Vec::new(),
            needs: Vec::new(),
            deployment: Deployment {
                files: files.iter().map(|f| f.to_string()).collect(),
                ..Default::default()
            },
            placements: Vec::new(),
            edits: Default::default(),
            previous_versions: Vec::new(),
        }
    }

    fn write(dir: &Path, file: &str, text: &str) {
        let path = dir.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn age(dir: &Path, file: &str, by: Duration) {
        let file = fs::File::options().write(true).open(dir.join(file)).unwrap();
        file.set_modified(SystemTime::now() - by).unwrap();
    }

    fn ue4ss_mods() -> Vec<InstalledMod> {
        vec![
            installed(
                "ue4ss",
                &["platform/steam/ue4ss/UE4SS.dll", "platform/steam/ue4ss/Mods/BPModLoaderMod/main.lua"],
            ),
            installed("lua-mod", &["platform/steam/ue4ss/Mods/Feel/Scripts/main.lua"]),
            installed("pak-mod", &["ProjectPBZ/Content/Paks/~mods/skip.pak"]),
        ]
    }

    fn ue4ss_game(log: &str) -> tempfile::TempDir {
        let game = tempfile::tempdir().unwrap();
        write(game.path(), "platform/steam/ue4ss/UE4SS.dll", "");
        write(game.path(), "platform/steam/ue4ss/UE4SS.log", log);
        game
    }

    #[test]
    fn a_loader_lost_after_a_game_update_is_reported_with_its_mods() {
        let game = ue4ss_game(UE4SS_FAILED);

        assert_eq!(
            loader_problems(&ue4ss_mods(), game.path()),
            vec![LoaderProblem {
                loader: "UE4SS".into(),
                mods: vec!["lua-mod".into()],
            }]
        );
    }

    #[test]
    fn a_scan_that_worked_on_a_later_try_is_no_problem() {
        let started = format!("{UE4SS_FAILED}{}", "[12:37:00] Found GUObjectArray\n".repeat(TAIL));
        let game = ue4ss_game(&started);

        assert!(loader_problems(&ue4ss_mods(), game.path()).is_empty());
    }

    #[test]
    fn a_log_older_than_the_loader_says_nothing_about_it() {
        let game = ue4ss_game(UE4SS_FAILED);
        age(game.path(), "platform/steam/ue4ss/UE4SS.log", Duration::from_secs(3600));

        assert!(loader_problems(&ue4ss_mods(), game.path()).is_empty());
    }

    #[test]
    fn a_loader_out_of_the_game_is_not_checked() {
        let game = ue4ss_game(UE4SS_FAILED);
        fs::remove_file(game.path().join("platform/steam/ue4ss/UE4SS.dll")).unwrap();

        assert!(loader_problems(&ue4ss_mods(), game.path()).is_empty());
    }

    #[test]
    fn a_turned_off_loader_is_not_checked() {
        let game = ue4ss_game(UE4SS_FAILED);
        let mut mods = ue4ss_mods();
        mods[0].enabled = false;

        assert!(loader_problems(&mods, game.path()).is_empty());
    }

    #[test]
    fn a_failed_loader_without_mods_stays_quiet() {
        let game = ue4ss_game(UE4SS_FAILED);

        assert!(loader_problems(&ue4ss_mods()[..1], game.path()).is_empty());
    }

    #[test]
    fn bepinex_is_read_from_the_line_of_a_good_start() {
        let mods = [
            installed("pack", &["BepInEx/core/BepInEx.dll"]),
            installed("plugin", &["BepInEx/plugins/Plugin/Plugin.dll"]),
        ];
        let game = tempfile::tempdir().unwrap();
        write(game.path(), "BepInEx/core/BepInEx.dll", "");
        write(game.path(), "BepInEx/LogOutput.log", "[Info   :   BepInEx] Patching\n");
        assert_eq!(loader_problems(&mods, game.path())[0].loader, "BepInEx");

        write(game.path(), "BepInEx/LogOutput.log", "[Message:   BepInEx] Chainloader startup complete\n");
        assert!(loader_problems(&mods, game.path()).is_empty());
    }

    #[test]
    fn red4ext_is_read_from_its_last_session() {
        let mods = [
            installed("red4ext", &["red4ext/RED4ext.dll"]),
            installed("plugin", &["red4ext/plugins/Plugin/Plugin.dll"]),
        ];
        let game = tempfile::tempdir().unwrap();
        write(game.path(), "red4ext/RED4ext.dll", "");
        write(game.path(), "red4ext/logs/red4ext-1.log", "RED4ext has been successfully initialized\n");
        write(game.path(), "red4ext/logs/red4ext-2.log", "RED4ext (v1.31.0) is initializing...\n");
        write(game.path(), "red4ext/logs/mod_settings-3.log", "");
        age(game.path(), "red4ext/logs/red4ext-1.log", Duration::from_secs(60));
        age(game.path(), "red4ext/logs/mod_settings-3.log", Duration::from_secs(120));

        assert_eq!(loader_problems(&mods, game.path())[0].loader, "RED4ext");

        age(game.path(), "red4ext/logs/red4ext-2.log", Duration::from_secs(120));
        assert!(loader_problems(&mods, game.path()).is_empty());
    }
}

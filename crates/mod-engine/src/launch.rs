//! **launch** block: play, know whether the game is running, and queue
//! changes while it runs.
//!
//! A running game keeps its files open: on Windows, a loaded DLL can't be
//! removed, and a mod added mid-session would only load at the next launch
//! anyway. During a session, changes therefore go into a queue, applied
//! when the game closes.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

/// Change requested during a session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum PendingOp {
    #[serde(rename_all = "camelCase")]
    Install {
        mod_id: String,
        /// Source picked in Discover (`hexium`). `None`: the game's source.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    SetEnabled { mod_id: String, enabled: bool },
    #[serde(rename_all = "camelCase")]
    Uninstall { mod_id: String },
}

impl PendingOp {
    pub fn mod_id(&self) -> &str {
        match self {
            Self::Install { mod_id, .. }
            | Self::SetEnabled { mod_id, .. }
            | Self::Uninstall { mod_id } => mod_id,
        }
    }
}

/// Adds `op` to the queue and simplifies it: one change per mod, and two
/// changes that cancel out disappear.
///
/// - disable then enable again: nothing left to do;
/// - install then remove a mod not yet installed: nothing left to do;
/// - enable or disable a mod whose install is pending: the install stays,
///   the toggle is ignored;
/// - otherwise, the last requested change replaces the previous one.
pub fn enqueue(queue: &[PendingOp], op: PendingOp) -> Vec<PendingOp> {
    let previous = queue.iter().find(|p| p.mod_id() == op.mod_id()).cloned();
    let mut next: Vec<PendingOp> = queue
        .iter()
        .filter(|p| p.mod_id() != op.mod_id())
        .cloned()
        .collect();
    let keep = match (&previous, &op) {
        (
            Some(PendingOp::SetEnabled { enabled: a, .. }),
            PendingOp::SetEnabled { enabled: b, .. },
        ) if a != b => None,
        (Some(PendingOp::Install { .. }), PendingOp::Uninstall { .. }) => None,
        (Some(install @ PendingOp::Install { .. }), PendingOp::SetEnabled { .. }) => {
            Some(install.clone())
        }
        _ => Some(op),
    };
    next.extend(keep);
    next
}

/// Is the game running? True if a process was started from its folder.
/// Works for Steam games as well as manually added ones, whatever the
/// executable name.
pub fn is_running(game_dir: &Path, running_exes: &[PathBuf]) -> bool {
    let dir = normalize(game_dir);
    running_exes.iter().any(|exe| {
        let exe = normalize(exe);
        exe.len() > dir.len() && exe.starts_with(&dir) && exe[dir.len()..].starts_with('/')
    })
}

/// Lenient path comparison: unified separators, and case ignored on
/// Windows (case-insensitive file system).
fn normalize(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    let s = s.trim_end_matches('/');
    if cfg!(windows) {
        s.to_lowercase()
    } else {
        s.to_owned()
    }
}

/// List of running executables, refreshed on demand.
pub struct ProcessWatcher {
    system: System,
}

impl Default for ProcessWatcher {
    fn default() -> Self {
        Self {
            system: System::new(),
        }
    }
}

impl ProcessWatcher {
    pub fn running_exes(&mut self) -> Vec<PathBuf> {
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet),
        );
        self.system
            .processes()
            .values()
            .filter_map(|p| p.exe().map(Path::to_path_buf))
            .collect()
    }
}

/// Is the executable one of the game's launchers (compared by name)?
pub fn is_launcher(exe: &Path, launchers: &[String]) -> bool {
    exe.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|name| launchers.iter().any(|l| l.eq_ignore_ascii_case(name)))
}

/// Is the game running, its launcher aside?
pub fn game_running(game_dir: &Path, running_exes: &[PathBuf], launchers: &[String]) -> bool {
    let game: Vec<PathBuf> = running_exes
        .iter()
        .filter(|exe| !is_launcher(exe, launchers))
        .cloned()
        .collect();
    is_running(game_dir, &game)
}

/// Is Steam open? (its executable among the processes).
pub fn steam_running(running_exes: &[PathBuf]) -> bool {
    running_exes.iter().any(|exe| {
        exe.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
            ["steam.exe", "steam_osx", "steam"].contains(&n.to_ascii_lowercase().as_str())
        })
    })
}

/// How to launch a game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchTarget {
    /// Through Steam (`steam://rungameid/<id>`): Steam handles updates, the
    /// Steam Overlay and any launch options.
    Steam(String),
    /// The game's executable, for a manually added game. Never a file from
    /// a mod archive: only executable names known to the
    /// catalog, in the game folder.
    Executable(PathBuf),
}

pub fn steam_target(app_id: u32) -> LaunchTarget {
    LaunchTarget::Steam(format!("steam://rungameid/{app_id}"))
}

/// First executable of the profile found in the game folder. On Windows we
/// prefer an `.exe`, on macOS an `.app`.
pub fn executable_target(game_dir: &Path, exe_names: &[String]) -> Option<LaunchTarget> {
    let preferred = if cfg!(windows) {
        ".exe"
    } else if cfg!(target_os = "macos") {
        ".app"
    } else {
        ""
    };
    let mut names: Vec<&String> = exe_names.iter().collect();
    names.sort_by_key(|n| !n.to_lowercase().ends_with(preferred));
    names
        .into_iter()
        .map(|n| game_dir.join(n))
        .find(|p| p.exists())
        .map(LaunchTarget::Executable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_launcher_alone_is_not_the_game() {
        let dir = Path::new("/games/Cyberpunk 2077");
        let launchers = vec!["REDprelauncher.exe".to_owned()];
        let launcher = vec![PathBuf::from("/games/Cyberpunk 2077/REDprelauncher.exe")];
        assert!(is_running(dir, &launcher));
        assert!(!game_running(dir, &launcher, &launchers));
        let both = vec![
            PathBuf::from("/games/Cyberpunk 2077/REDprelauncher.exe"),
            PathBuf::from("/games/Cyberpunk 2077/bin/x64/Cyberpunk2077.exe"),
        ];
        assert!(game_running(dir, &both, &launchers));
        assert!(is_launcher(
            Path::new("C:/x/redlauncher.EXE"),
            &["REDlauncher.exe".into()]
        ));
    }

    fn install(id: &str) -> PendingOp {
        PendingOp::Install {
            mod_id: id.into(),
            source: None,
        }
    }
    fn toggle(id: &str, enabled: bool) -> PendingOp {
        PendingOp::SetEnabled {
            mod_id: id.into(),
            enabled,
        }
    }
    fn uninstall(id: &str) -> PendingOp {
        PendingOp::Uninstall { mod_id: id.into() }
    }

    #[test]
    fn opposite_changes_cancel_out() {
        let q = enqueue(&[], toggle("a-A", false));
        assert!(enqueue(&q, toggle("a-A", true)).is_empty());

        let q = enqueue(&[], install("a-A"));
        assert!(enqueue(&q, uninstall("a-A")).is_empty());
    }

    #[test]
    fn the_last_change_wins_and_mods_are_independent() {
        let q = enqueue(&[], toggle("a-A", false));
        let q = enqueue(&q, install("b-B"));
        let q = enqueue(&q, uninstall("a-A"));
        assert_eq!(q, [install("b-B"), uninstall("a-A")]);

        // Toggling a mod whose install is pending: the install stays.
        assert_eq!(
            enqueue(&q, toggle("b-B", false)),
            [uninstall("a-A"), install("b-B")]
        );
    }

    #[test]
    fn a_game_runs_when_a_process_comes_from_its_folder() {
        let game = Path::new("/games/Hollow Knight Silksong");
        let exes = |list: &[&str]| list.iter().map(PathBuf::from).collect::<Vec<_>>();

        assert!(is_running(
            game,
            &exes(&[
                "/usr/bin/zsh",
                "/games/Hollow Knight Silksong/Hollow Knight Silksong.exe"
            ])
        ));
        assert!(is_running(
            game,
            &exes(&["/games/Hollow Knight Silksong/bin/game"])
        ));
        assert!(!is_running(game, &exes(&["/usr/bin/zsh"])));
        // A sibling folder with a similar name doesn't count.
        assert!(!is_running(
            game,
            &exes(&["/games/Hollow Knight Silksong Demo/game.exe"])
        ));
        assert!(!is_running(game, &exes(&[])));
    }

    #[cfg(windows)]
    #[test]
    fn windows_paths_ignore_case_and_separators() {
        let game =
            Path::new(r"C:\Program Files (x86)\Steam\steamapps\common\Hollow Knight Silksong");
        let exe = PathBuf::from(
            r"c:\program files (x86)\steam\steamapps\common\hollow knight silksong\Hollow Knight Silksong.exe",
        );
        assert!(is_running(game, &[exe]));
    }

    #[test]
    fn launch_targets() {
        assert_eq!(
            steam_target(1030300),
            LaunchTarget::Steam("steam://rungameid/1030300".into())
        );

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Game.exe"), "").unwrap();
        let names = vec!["Missing.exe".to_string(), "Game.exe".to_string()];
        assert_eq!(
            executable_target(dir.path(), &names),
            Some(LaunchTarget::Executable(dir.path().join("Game.exe")))
        );
        assert_eq!(
            executable_target(dir.path(), &["Nope.exe".to_string()]),
            None
        );
    }

    #[test]
    fn pending_ops_serialize_for_the_interface() {
        let json = serde_json::to_value(toggle("a-A", false)).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "type": "setEnabled", "modId": "a-A", "enabled": false })
        );
    }
}

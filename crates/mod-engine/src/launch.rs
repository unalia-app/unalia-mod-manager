//! Brique **launch** : jouer, savoir si le jeu tourne, et mettre les
//! changements en attente tant qu'il tourne (SPEC §3.6).
//!
//! Un jeu lancé tient ses fichiers ouverts : sous Windows, une DLL chargée
//! ne peut pas être retirée, et un mod ajouté en cours de partie ne serait
//! de toute façon chargé qu'au prochain lancement. Pendant une partie, les
//! changements vont donc dans une file, appliquée à la fermeture du jeu.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

/// Changement demandé pendant une partie.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum PendingOp {
    #[serde(rename_all = "camelCase")]
    Install { mod_id: String },
    #[serde(rename_all = "camelCase")]
    SetEnabled { mod_id: String, enabled: bool },
    #[serde(rename_all = "camelCase")]
    Uninstall { mod_id: String },
}

impl PendingOp {
    pub fn mod_id(&self) -> &str {
        match self {
            Self::Install { mod_id }
            | Self::SetEnabled { mod_id, .. }
            | Self::Uninstall { mod_id } => mod_id,
        }
    }
}

/// Ajoute `op` à la file en la simplifiant : un seul changement par mod, et
/// deux changements qui s'annulent disparaissent.
///
/// - désactiver puis réactiver : plus rien à faire ;
/// - installer puis retirer un mod pas encore installé : plus rien à faire ;
/// - activer ou désactiver un mod dont l'installation est en attente :
///   l'installation reste, le basculement est ignoré ;
/// - sinon, le dernier changement demandé remplace le précédent.
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

/// Le jeu tourne-t-il ? Vrai si un processus a été lancé depuis son dossier.
/// Marche pour les jeux Steam comme pour ceux ajoutés à la main, quel que
/// soit le nom de l'exécutable.
pub fn is_running(game_dir: &Path, running_exes: &[PathBuf]) -> bool {
    let dir = normalize(game_dir);
    running_exes.iter().any(|exe| {
        let exe = normalize(exe);
        exe.len() > dir.len() && exe.starts_with(&dir) && exe[dir.len()..].starts_with('/')
    })
}

/// Comparaison de chemins tolérante : séparateurs unifiés, et casse ignorée
/// sous Windows (système de fichiers insensible à la casse).
fn normalize(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    let s = s.trim_end_matches('/');
    if cfg!(windows) {
        s.to_lowercase()
    } else {
        s.to_owned()
    }
}

/// Liste des exécutables en cours, rafraîchie à la demande.
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

/// Comment lancer un jeu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchTarget {
    /// Par Steam (`steam://rungameid/<id>`) : Steam gère les mises à jour,
    /// le Steam Overlay et les éventuelles options de lancement.
    Steam(String),
    /// Exécutable du jeu, pour un jeu ajouté à la main. Jamais un fichier
    /// venu d'une archive de mod (SPEC §2.3) : uniquement les noms
    /// d'exécutables connus du catalogue, dans le dossier du jeu.
    Executable(PathBuf),
}

pub fn steam_target(app_id: u32) -> LaunchTarget {
    LaunchTarget::Steam(format!("steam://rungameid/{app_id}"))
}

/// Premier exécutable connu du profil présent dans le dossier du jeu. Sous
/// Windows on préfère un `.exe`, sous macOS un `.app`.
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

    fn install(id: &str) -> PendingOp {
        PendingOp::Install { mod_id: id.into() }
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

        // Basculer un mod dont l'installation attend : l'installation reste.
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
        // Un dossier voisin au nom proche ne compte pas.
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

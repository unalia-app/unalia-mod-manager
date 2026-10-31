//! Games recognized **automatically**: a game without a profile (neither in
//! the Thunderstore schema nor in `games/*.toml`) whose engine has an
//! adapter, and whose name exists on Nexus Mods. Its profile is derived
//! from the disk and from Nexus, with nothing written by hand.
//!
//! Today: Unreal Engine (project folder found by `find_project`). The name
//! must match exactly (case, ™ and ® aside): we don't guess. A hand-written
//! profile still wins.
//!
//! Nexus answers are kept on disk (`NexusGames`): the game scan doesn't
//! repeat the request every time.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{GameProfile, NexusGame};
use crate::loaders::framework::Framework;
use crate::loaders::rules::InstallRules;
use crate::loaders::unreal::find_project;
use crate::sources::nexus::FoundGame;

/// Prefix of automatic profiles: `auto-<Nexus domain>`.
const PREFIX: &str = "auto-";

/// A game found on Nexus is checked again after a month, a missing game
/// after three days (its page may open in the meantime).
const FOUND_FOR_SECS: i64 = 30 * 24 * 3600;
const MISSING_FOR_SECS: i64 = 3 * 24 * 3600;

/// Comparable name: without ™, ® or ©, collapsed spaces, lowercase.
pub fn normalize(name: &str) -> String {
    name.chars()
        .filter(|c| !matches!(c, '™' | '®' | '©'))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Nexus domain of an automatic profile.
pub fn domain_of(profile_id: &str) -> Option<&str> {
    profile_id.strip_prefix(PREFIX)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Template {
    frameworks: Vec<Framework>,
}

/// Profile of an Unreal game found on Nexus. `None` if the folder doesn't
/// hold a single Unreal project.
pub fn unreal_profile(found: &FoundGame, game_dir: &Path) -> Option<GameProfile> {
    let project = find_project(game_dir)?;
    // Embedded and tested: an error here is a Modalia bug.
    let template: Template = toml::from_str(include_str!("../../../../games/auto/unreal.toml"))
        .expect("invalid Unreal template");
    Some(GameProfile {
        id: format!("{PREFIX}{}", found.domain),
        name: found.name.clone(),
        community: format!("nexus:{}", found.domain),
        steam_app_ids: Vec::new(),
        loader: "unreal".into(),
        supported: true,
        exe_names: unreal_exes(game_dir, &project),
        data_folder_name: None,
        cover_url: None,
        hero_url: None,
        nexus: Some(NexusGame {
            domain: found.domain.clone(),
            game_id: found.game_id,
        }),
        install_rules: InstallRules::default(),
        frameworks: template.frameworks,
        mirrors: Vec::new(),
        launchers: Vec::new(),
        versions: Vec::new(),
        plugins: None,
        modded: None,
    })
}

/// Executables of an Unreal game: the root one (which launches the game),
/// then the project's.
fn unreal_exes(game_dir: &Path, project: &str) -> Vec<String> {
    const NOT_THE_GAME: [&str; 4] = ["unins", "crash", "setup", "redist"];
    let mut exes: Vec<String> = fs::read_dir(game_dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| {
            let lower = name.to_lowercase();
            lower.ends_with(".exe") && !NOT_THE_GAME.iter().any(|n| lower.contains(n))
        })
        .collect();
    exes.sort();
    let shipping = format!("{project}/Binaries/Win64/{project}-Win64-Shipping.exe");
    if game_dir.join(&shipping).is_file() {
        exes.push(shipping);
    }
    exes
}

/// Names to look up on Nexus for a manually added game: its folder, then
/// its root executables.
pub fn candidate_names(game_dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = game_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .into_iter()
        .collect();
    if let Some(project) = find_project(game_dir) {
        for exe in unreal_exes(game_dir, &project) {
            if let Some(stem) = exe.strip_suffix(".exe").filter(|s| !s.contains('/')) {
                names.push(stem.to_owned());
            }
        }
    }
    let mut seen = Vec::new();
    names.retain(|n| {
        let key = normalize(n);
        !key.is_empty() && !seen.contains(&key) && {
            seen.push(key);
            true
        }
    });
    names
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    found: Option<FoundGame>,
    /// Seconds since the Unix epoch.
    checked: i64,
}

/// Nexus answers, by name (`normalize`), kept on disk.
#[derive(Debug, Default)]
pub struct NexusGames {
    path: PathBuf,
    entries: HashMap<String, Entry>,
}

impl NexusGames {
    /// Unreadable or missing file: start from scratch.
    pub fn load(path: &Path) -> Self {
        let entries = fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Self {
            path: path.to_owned(),
            entries,
        }
    }

    /// Answer still valid for this name: `Some(None)` if Nexus doesn't
    /// know it, `None` if we need to ask again.
    pub fn fresh(&self, name: &str, now: i64) -> Option<Option<&FoundGame>> {
        let entry = self.entries.get(&normalize(name))?;
        let ttl = if entry.found.is_some() {
            FOUND_FOR_SECS
        } else {
            MISSING_FOR_SECS
        };
        (now - entry.checked < ttl).then_some(entry.found.as_ref())
    }

    /// Last known answer, even an old one (Nexus unreachable).
    pub fn last_known(&self, name: &str) -> Option<&FoundGame> {
        self.entries.get(&normalize(name))?.found.as_ref()
    }

    /// Game already found, by its domain (profile of a manually added game).
    pub fn by_domain(&self, domain: &str) -> Option<&FoundGame> {
        self.entries
            .values()
            .filter_map(|e| e.found.as_ref())
            .find(|f| f.domain == domain)
    }

    pub fn record(&mut self, name: &str, found: Option<FoundGame>, now: i64) {
        self.entries.insert(
            normalize(name),
            Entry {
                found,
                checked: now,
            },
        );
    }

    pub fn save(&self) -> io::Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string(&self.entries).map_err(io::Error::other)?;
        let tmp = self.path.with_extension("tmp");
        fs::write(&tmp, text)?;
        fs::rename(tmp, &self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hogwarts() -> FoundGame {
        FoundGame {
            name: "Hogwarts Legacy".into(),
            domain: "hogwartslegacy".into(),
            game_id: 5113,
        }
    }

    #[test]
    fn names_compare_without_case_or_trademarks() {
        assert_eq!(normalize("ELDEN RING™"), "elden ring");
        assert_eq!(
            normalize("  Tom Clancy's Rainbow Six®  Siege "),
            "tom clancy's rainbow six siege"
        );
    }

    #[test]
    fn answers_expire_and_survive_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nexus-games.json");
        let mut games = NexusGames::load(&path);
        games.record("Hogwarts Legacy", Some(hogwarts()), 1_000);
        games.record("Unknown Game", None, 1_000);
        games.save().unwrap();

        let games = NexusGames::load(&path);
        assert_eq!(
            games.fresh("hogwarts legacy™", 2_000),
            Some(Some(&hogwarts()))
        );
        assert_eq!(games.fresh("Unknown Game", 2_000), Some(None));
        assert_eq!(games.fresh("Unknown Game", 1_000 + MISSING_FOR_SECS), None);
        assert_eq!(games.fresh("Hogwarts Legacy", 1_000 + FOUND_FOR_SECS), None);
        assert_eq!(games.last_known("Hogwarts Legacy"), Some(&hogwarts()));
        assert_eq!(games.by_domain("hogwartslegacy"), Some(&hogwarts()));
        assert_eq!(games.fresh("Never Asked", 2_000), None);
    }

    #[test]
    fn an_unreal_game_gets_a_nexus_profile_with_ue4ss() {
        let dir = tempfile::tempdir().unwrap();
        let game = dir.path().join("Hogwarts Legacy");
        fs::create_dir_all(game.join("Phoenix/Content/Paks")).unwrap();
        fs::create_dir_all(game.join("Phoenix/Binaries/Win64")).unwrap();
        fs::create_dir_all(game.join("Engine")).unwrap();
        fs::write(game.join("HogwartsLegacy.exe"), "").unwrap();
        fs::write(game.join("unins000.exe"), "").unwrap();
        fs::write(
            game.join("Phoenix/Binaries/Win64/Phoenix-Win64-Shipping.exe"),
            "",
        )
        .unwrap();

        let profile = unreal_profile(&hogwarts(), &game).unwrap();
        assert_eq!(profile.id, "auto-hogwartslegacy");
        assert_eq!(domain_of(&profile.id), Some("hogwartslegacy"));
        assert_eq!(profile.community, "nexus:hogwartslegacy");
        assert_eq!(profile.loader, "unreal");
        assert!(profile.supported);
        assert_eq!(
            profile.exe_names,
            [
                "HogwartsLegacy.exe",
                "Phoenix/Binaries/Win64/Phoenix-Win64-Shipping.exe"
            ]
        );
        assert_eq!(
            profile.frameworks[0]
                .resolve(Some("Phoenix"), Some("Phoenix/Binaries/Win64"))
                .marker,
            "Phoenix/Binaries/Win64/ue4ss/UE4SS.dll"
        );
        assert_eq!(
            candidate_names(&game),
            ["Hogwarts Legacy", "HogwartsLegacy"]
        );

        // No Unreal project: no profile.
        assert!(unreal_profile(&hogwarts(), dir.path()).is_none());
    }
}

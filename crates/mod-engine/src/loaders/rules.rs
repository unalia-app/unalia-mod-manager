//! Generic adapter driven by the `installRules` of the Thunderstore schema
//! (the ones from r2modman's legacy installer). A game describes its mod
//! folders there; no per-game code.
//!
//! For each file of a package, the destination is decided this way:
//! 1. a folder of the package has the name of a route (`plugins/`, `Mods/`,
//!    `UserData/`…): its content goes into that route;
//! 2. else, the extension matches a route (`.dll` to `Mods`); the longest
//!    wins (`.lib.dll` before `.dll`);
//! 3. else, the game's default route.
//!
//! Then the route's tracking mode says how to store it:
//! - `subdir`: `route/<mod>/<path>` (each mod in its own folder);
//! - `subdir-no-flatten`: same, keeping the full package path;
//! - `none` and `state`: `route/<path>`, without a subfolder (settings,
//!   shared libraries). Modalia tracks every placed file anyway, and a file
//!   already there is never overwritten.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{Loader, PackageKind, Placement};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tracking {
    Subdir,
    SubdirNoFlatten,
    None,
    State,
    /// The archive is placed as is (Godot). Not handled yet.
    PackageZip,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallRule {
    /// Folder relative to the game, e.g. `BepInEx/plugins`.
    pub route: String,
    /// Extensions sent here by default, e.g. `.dll`, `.lib.dll`.
    pub extensions: Vec<String>,
    pub tracking: Tracking,
    /// Route for the files no other rule claims.
    pub default: bool,
}

/// A game's rules, ready to use.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallRules {
    pub rules: Vec<InstallRule>,
    /// Package files never to install (package root).
    pub exclusions: Vec<String>,
}

/// Thunderstore metadata at a package root: never installed.
const METADATA: [&str; 7] = [
    "manifest.json",
    "icon.png",
    "readme.md",
    "changelog.md",
    "license",
    "licence",
    "license.md",
];

impl InstallRules {
    /// Are all routes handled by Modalia?
    pub fn installable(&self) -> bool {
        !self.rules.is_empty()
            && self.rules.iter().any(|r| r.default)
            && self
                .rules
                .iter()
                .all(|r| !matches!(r.tracking, Tracking::PackageZip | Tracking::Unknown))
    }

    /// MelonLoader conventions, for "recursive-melonloader" games whose
    /// schema doesn't detail the rules.
    pub fn melonloader_defaults() -> Self {
        let rule = |route: &str, extensions: &[&str], tracking, default| InstallRule {
            route: route.to_owned(),
            extensions: extensions.iter().map(|e| e.to_string()).collect(),
            tracking,
            default,
        };
        Self {
            rules: vec![
                rule("Mods", &[".dll"], Tracking::State, false),
                rule("Plugins", &[".plugin.dll"], Tracking::State, false),
                rule("UserLibs", &[".lib.dll"], Tracking::State, false),
                rule("UserData", &[], Tracking::State, false),
                rule("UserData/ModManager", &[], Tracking::SubdirNoFlatten, true),
            ],
            exclusions: Vec::new(),
        }
    }

    fn is_excluded(&self, name: &str) -> bool {
        let lower = name.to_ascii_lowercase();
        METADATA.contains(&lower.as_str())
            || self.exclusions.iter().any(|e| e.eq_ignore_ascii_case(name))
    }

    /// Route claimed by a package folder: the first folder (from the root)
    /// that ends a route. A long route (`Renderer/BepInEx/plugins`) wins if
    /// the path mirrors it.
    fn folder_match(&self, folders: &[&str]) -> Option<(&InstallRule, usize)> {
        for (i, folder) in folders.iter().enumerate() {
            let best = self
                .rules
                .iter()
                .filter_map(|rule| {
                    let route: Vec<&str> = rule.route.split('/').collect();
                    let last = route.last()?;
                    if !last.eq_ignore_ascii_case(folder) {
                        return None;
                    }
                    // Number of route segments that the path mirrors.
                    let depth = route
                        .iter()
                        .rev()
                        .zip(folders[..=i].iter().rev())
                        .take_while(|(r, f)| r.eq_ignore_ascii_case(f))
                        .count();
                    Some((rule, depth))
                })
                // On a tie, the game's first rule wins (`rev`: `max_by_key`
                // keeps the last of equal ones).
                .rev()
                .max_by_key(|(_, depth)| *depth)
                .map(|(rule, _)| rule);
            if let Some(rule) = best {
                // The content after this folder goes into the route.
                return Some((rule, i + 1));
            }
        }
        None
    }

    fn extension_match(&self, name: &str) -> Option<&InstallRule> {
        let lower = name.to_ascii_lowercase();
        self.rules
            .iter()
            .flat_map(|rule| rule.extensions.iter().map(move |ext| (rule, ext)))
            .filter(|(_, ext)| lower.ends_with(&ext.to_ascii_lowercase()))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .max_by_key(|(_, ext)| ext.len())
            .map(|(rule, _)| rule)
    }

    fn place_file(&self, package_id: &str, source: &str) -> Option<String> {
        let parts: Vec<&str> = source.split('/').collect();
        let (name, folders) = parts.split_last()?;
        if folders.is_empty() && self.is_excluded(name) {
            return None;
        }

        // `rest`: path under the route; `whole`: full package path.
        let (rule, rest) = match self.folder_match(folders) {
            Some((rule, consumed)) => (rule, parts[consumed..].join("/")),
            None => match self.extension_match(name) {
                Some(rule) => (rule, parts.join("/")),
                None => (self.rules.iter().find(|r| r.default)?, parts.join("/")),
            },
        };
        if rest.is_empty() {
            return None;
        }
        Some(match rule.tracking {
            Tracking::Subdir => format!("{}/{package_id}/{rest}", rule.route),
            Tracking::SubdirNoFlatten => format!("{}/{package_id}/{}", rule.route, parts.join("/")),
            Tracking::None | Tracking::State => format!("{}/{rest}", rule.route),
            Tracking::PackageZip | Tracking::Unknown => return None,
        })
    }
}

/// A game's generic loader: its rules, and the expected loader (to spot a
/// manual install).
pub struct RulesLoader<'a> {
    pub loader: &'a str,
    pub rules: &'a InstallRules,
}

impl Loader for RulesLoader<'_> {
    fn id(&self) -> &'static str {
        "rules"
    }

    fn installed_manually(&self, game_dir: &Path) -> bool {
        match self.loader {
            "bepinex" | "bepisloader" => game_dir.join("BepInEx").join("core").is_dir(),
            "melonloader" | "recursive-melonloader" => game_dir.join("MelonLoader").is_dir(),
            _ => false,
        }
    }

    fn place(&self, package_id: &str, kind: &PackageKind, files: &[String]) -> Vec<Placement> {
        files
            .iter()
            .filter_map(|source| {
                let target = match kind {
                    PackageKind::Loader { root_folder } => {
                        loader_target(self.rules, source, root_folder)?
                    }
                    PackageKind::Mod => self.rules.place_file(package_id, source)?,
                };
                Some(Placement {
                    source: source.clone(),
                    target,
                })
            })
            .collect()
    }
}

/// Loader package: the content of its root folder (or the whole package if
/// the root is empty, like MelonLoader) goes to the game root.
pub(super) fn loader_target(
    rules: &InstallRules,
    source: &str,
    root_folder: &str,
) -> Option<String> {
    if root_folder.is_empty() {
        let top_level = !source.contains('/');
        return (!(top_level && rules.is_excluded(source))).then(|| source.to_owned());
    }
    let (first, rest) = source.split_once('/')?;
    (first.eq_ignore_ascii_case(root_folder) && !rest.is_empty()).then(|| rest.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_zip_rules_are_not_installable() {
        let rules = InstallRules {
            rules: vec![InstallRule {
                route: "mods".into(),
                extensions: vec![],
                tracking: Tracking::PackageZip,
                default: true,
            }],
            exclusions: vec![],
        };
        assert!(!rules.installable());
        assert!(!InstallRules::default().installable());
        assert!(InstallRules::melonloader_defaults().installable());
    }

    #[test]
    fn unknown_tracking_methods_deserialize_without_failing() {
        let t: Tracking = serde_json::from_str("\"something-new\"").unwrap();
        assert_eq!(t, Tracking::Unknown);
    }
}

//! Adaptateur générique piloté par les `installRules` du schéma Thunderstore
//! (ceux de l'installeur historique d'r2modman). Un jeu y décrit ses
//! dossiers de mods ; aucun code par jeu.
//!
//! Pour chaque fichier d'un paquet, la destination se décide ainsi :
//! 1. un dossier du paquet porte le nom d'une route (`plugins/`, `Mods/`,
//!    `UserData/`…) : son contenu va dans cette route ;
//! 2. sinon, l'extension correspond à une route (`.dll` → `Mods`) ; la plus
//!    longue gagne (`.lib.dll` avant `.dll`) ;
//! 3. sinon, la route par défaut du jeu.
//!
//! Puis le mode de suivi de la route dit comment ranger :
//! - `subdir` : `route/<mod>/<chemin>` (chaque mod dans son dossier) ;
//! - `subdir-no-flatten` : pareil, en gardant le chemin complet du paquet ;
//! - `none` et `state` : `route/<chemin>`, sans sous-dossier (réglages,
//!   bibliothèques partagées). Modalia suit de toute façon chaque fichier
//!   posé, et un fichier déjà présent n'est jamais écrasé.

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
    /// L'archive est posée telle quelle (Godot). Pas encore géré.
    PackageZip,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallRule {
    /// Dossier relatif au jeu, ex. `BepInEx/plugins`.
    pub route: String,
    /// Extensions envoyées ici par défaut, ex. `.dll`, `.lib.dll`.
    pub extensions: Vec<String>,
    pub tracking: Tracking,
    /// Route des fichiers qu'aucune autre règle ne réclame.
    pub default: bool,
}

/// Règles d'un jeu, prêtes à servir.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallRules {
    pub rules: Vec<InstallRule>,
    /// Fichiers du paquet à ne jamais installer (racine du paquet).
    pub exclusions: Vec<String>,
}

/// Métadonnées Thunderstore à la racine d'un paquet : jamais installées.
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
    /// Toutes les routes sont-elles gérées par Modalia ?
    pub fn installable(&self) -> bool {
        !self.rules.is_empty()
            && self.rules.iter().any(|r| r.default)
            && self
                .rules
                .iter()
                .all(|r| !matches!(r.tracking, Tracking::PackageZip | Tracking::Unknown))
    }

    /// Conventions de MelonLoader, pour les jeux « recursive-melonloader »
    /// dont le schéma ne détaille pas les règles.
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

    /// Route réclamée par un dossier du paquet : le premier dossier (en
    /// partant de la racine) qui termine une route. Une route longue
    /// (`Renderer/BepInEx/plugins`) l'emporte si le chemin la reproduit.
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
                    // Nombre de segments de la route que le chemin reproduit.
                    let depth = route
                        .iter()
                        .rev()
                        .zip(folders[..=i].iter().rev())
                        .take_while(|(r, f)| r.eq_ignore_ascii_case(f))
                        .count();
                    Some((rule, depth))
                })
                // À égalité, la première règle du jeu gagne (`rev` : `max_by_key`
                // garde le dernier des ex æquo).
                .rev()
                .max_by_key(|(_, depth)| *depth)
                .map(|(rule, _)| rule);
            if let Some(rule) = best {
                // Le contenu qui suit ce dossier va dans la route.
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

        // `rest` : chemin sous la route ; `whole` : chemin complet du paquet.
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

/// Le loader générique d'un jeu : ses règles, et le loader attendu (pour
/// repérer une installation manuelle).
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

/// Paquet loader : le contenu de son dossier racine (ou tout le paquet si
/// la racine est vide, comme MelonLoader) va à la racine du jeu.
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

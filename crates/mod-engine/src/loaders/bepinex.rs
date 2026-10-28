//! BepInEx (Unity games), following the rules of the legacy Thunderstore /
//! r2modman installer:
//!
//! - the loader (`BepInExPack`): the content of its root folder goes to the
//!   game root (`winhttp.dll`, `doorstop_config.ini`, `BepInEx/`…);
//! - a mod: `plugins/`, `patchers/`, `core/`, `monomod/` go into the matching
//!   BepInEx folder, under the mod name; `config/` goes into
//!   `BepInEx/config/` without a subfolder (mods read their settings there
//!   by name); everything else goes into `BepInEx/plugins/<mod>/`;
//! - a `BepInEx/` prefix in the package is tolerated;
//! - the Thunderstore metadata at the root (manifest, icon, README…) is not
//!   installed.

use std::path::Path;

use super::{Loader, PackageKind, Placement};

pub struct BepInEx;

/// BepInEx folders where each mod gets its own subfolder.
const NAMESPACED: [&str; 4] = ["plugins", "patchers", "core", "monomod"];

const METADATA: [&str; 6] = [
    "manifest.json",
    "icon.png",
    "readme.md",
    "changelog.md",
    "license",
    "license.md",
];

impl Loader for BepInEx {
    fn id(&self) -> &'static str {
        "bepinex"
    }

    fn installed_manually(&self, game_dir: &Path) -> bool {
        game_dir.join("BepInEx").join("core").is_dir()
    }

    fn place(&self, package_id: &str, kind: &PackageKind, files: &[String]) -> Vec<Placement> {
        files
            .iter()
            .filter_map(|source| {
                let target = match kind {
                    PackageKind::Loader { root_folder } => loader_target(source, root_folder)?,
                    PackageKind::Mod => mod_target(source, package_id)?,
                };
                Some(Placement {
                    source: source.clone(),
                    target,
                })
            })
            .collect()
    }
}

fn loader_target(source: &str, root_folder: &str) -> Option<String> {
    let (first, rest) = source.split_once('/')?;
    (first.eq_ignore_ascii_case(root_folder) && !rest.is_empty()).then(|| rest.to_owned())
}

fn mod_target(source: &str, package_id: &str) -> Option<String> {
    let parts: Vec<&str> = source.split('/').collect();
    if parts.len() == 1 && METADATA.contains(&parts[0].to_ascii_lowercase().as_str()) {
        return None;
    }
    // `BepInEx/plugins/...` in the package: strip the prefix.
    let parts = match parts.as_slice() {
        [first, rest @ ..] if first.eq_ignore_ascii_case("BepInEx") && !rest.is_empty() => rest,
        all => all,
    };

    let (folder, rest) = match parts {
        [first, rest @ ..] if !rest.is_empty() => (first.to_ascii_lowercase(), rest.join("/")),
        _ => (String::new(), parts.join("/")),
    };
    Some(if NAMESPACED.contains(&folder.as_str()) {
        format!("BepInEx/{folder}/{package_id}/{rest}")
    } else if folder == "config" {
        format!("BepInEx/config/{rest}")
    } else {
        format!("BepInEx/plugins/{package_id}/{}", parts.join("/"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(kind: PackageKind, files: &[&str]) -> Vec<(String, String)> {
        let files: Vec<String> = files.iter().map(|f| f.to_string()).collect();
        BepInEx
            .place("Author-Cool_Mod", &kind, &files)
            .into_iter()
            .map(|p| (p.source, p.target))
            .collect()
    }

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    #[test]
    fn the_loader_pack_goes_to_the_game_root() {
        let placed = place(
            PackageKind::Loader {
                root_folder: "BepInExPack".into(),
            },
            &[
                "BepInExPack/winhttp.dll",
                "BepInExPack/doorstop_config.ini",
                "BepInExPack/BepInEx/core/BepInEx.dll",
                "icon.png",
                "manifest.json",
                "README.md",
            ],
        );
        assert_eq!(
            placed,
            pairs(&[
                ("BepInExPack/winhttp.dll", "winhttp.dll"),
                ("BepInExPack/doorstop_config.ini", "doorstop_config.ini"),
                (
                    "BepInExPack/BepInEx/core/BepInEx.dll",
                    "BepInEx/core/BepInEx.dll"
                ),
            ])
        );
    }

    #[test]
    fn mod_folders_are_namespaced_except_config() {
        let placed = place(
            PackageKind::Mod,
            &[
                "plugins/CoolMod.dll",
                "plugins/assets/sprite.png",
                "patchers/CoolPatcher.dll",
                "config/com.author.cool.cfg",
                "Loose.dll",
                "data/levels.json",
                "manifest.json",
                "icon.png",
                "README.md",
                "CHANGELOG.md",
            ],
        );
        assert_eq!(
            placed,
            pairs(&[
                (
                    "plugins/CoolMod.dll",
                    "BepInEx/plugins/Author-Cool_Mod/CoolMod.dll"
                ),
                (
                    "plugins/assets/sprite.png",
                    "BepInEx/plugins/Author-Cool_Mod/assets/sprite.png"
                ),
                (
                    "patchers/CoolPatcher.dll",
                    "BepInEx/patchers/Author-Cool_Mod/CoolPatcher.dll"
                ),
                (
                    "config/com.author.cool.cfg",
                    "BepInEx/config/com.author.cool.cfg"
                ),
                ("Loose.dll", "BepInEx/plugins/Author-Cool_Mod/Loose.dll"),
                (
                    "data/levels.json",
                    "BepInEx/plugins/Author-Cool_Mod/data/levels.json"
                ),
            ])
        );
    }

    #[test]
    fn tolerates_a_bepinex_prefix_and_any_case() {
        let placed = place(
            PackageKind::Mod,
            &["BepInEx/plugins/CoolMod.dll", "Plugins/Other.dll"],
        );
        assert_eq!(
            placed,
            pairs(&[
                (
                    "BepInEx/plugins/CoolMod.dll",
                    "BepInEx/plugins/Author-Cool_Mod/CoolMod.dll"
                ),
                (
                    "Plugins/Other.dll",
                    "BepInEx/plugins/Author-Cool_Mod/Other.dll"
                ),
            ])
        );
    }
}

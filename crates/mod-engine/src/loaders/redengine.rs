//! Adaptateur **REDengine** (Cyberpunk 2077).
//!
//! La plupart des archives reproduisent l'arborescence du jeu (`archive/`,
//! `bin/`, `r6/`, `red4ext/`…) : elles se posent telles quelles. Les autres
//! sont rangées d'après leur contenu, sans jamais deviner au hasard :
//! - `.archive` et `.xl` → `archive/pc/mod/` ;
//! - dossier avec un `init.lua` → mod Cyber Engine Tweaks ;
//! - dossier avec un `info.json` → mod REDmod (`mods/`) ;
//! - `.reds` → `r6/scripts/<mod>/` ; `.yaml`, `.yml`, `.tweak` →
//!   `r6/tweaks/<mod>/`.
//!
//! Le reste (readme, captures d'écran) n'est pas installé.
//!
//! Les frameworks (RED4ext, Cyber Engine Tweaks, redscript…) sont décrits
//! par le profil du jeu (`games/cyberpunk-2077.toml`, voir `framework`) : de
//! la donnée, pas du code.

use std::collections::HashSet;
use std::path::Path;

pub use super::framework::{Framework, needed_frameworks};
use super::{Loader, PackageKind, Placement};

/// Dossiers racine du jeu qu'une archive peut reproduire.
const ROOTS: [&str; 6] = ["archive", "bin", "engine", "mods", "r6", "red4ext"];

const CET_MODS: &str = "bin/x64/plugins/cyber_engine_tweaks/mods";

pub struct RedEngine;

impl Loader for RedEngine {
    fn id(&self) -> &'static str {
        "redengine"
    }

    fn installed_manually(&self, _game_dir: &Path) -> bool {
        // Pas de loader unique : chaque framework se vérifie à part.
        false
    }

    fn place(&self, package_id: &str, _kind: &PackageKind, files: &[String]) -> Vec<Placement> {
        let skip = wrapper_depth(files);
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
                    target: target(parts, &all, &folder)?,
                })
            })
            .collect()
    }
}

/// Dossiers d'emballage à ignorer : `Mon Mod v1.2/archive/pc/mod/…`. On
/// descend tant que tout le paquet est dans un seul dossier qui n'est ni une
/// racine du jeu, ni un mod CET ou REDmod (dont le nom compte).
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
            if parts.len() == 2 && is_mod_manifest(parts[1]) {
                return depth;
            }
        }
        match first {
            Some(f) if !is_root(f) => depth += 1,
            _ => return depth,
        }
    }
}

fn is_root(segment: &str) -> bool {
    ROOTS.iter().any(|r| r.eq_ignore_ascii_case(segment))
}

fn is_mod_manifest(name: &str) -> bool {
    name.eq_ignore_ascii_case("init.lua") || name.eq_ignore_ascii_case("info.json")
}

fn target(parts: &[&str], all: &HashSet<String>, folder: &str) -> Option<String> {
    let (name, dirs) = parts.split_last()?;
    let lower = name.to_ascii_lowercase();

    // 1. Arborescence du jeu.
    if let Some(first) = dirs.first()
        && is_root(first)
    {
        return Some(format!(
            "{}/{}",
            first.to_ascii_lowercase(),
            parts[1..].join("/")
        ));
    }

    // 2. Mod CET ou REDmod : le dossier le plus proche qui contient le
    //    manifeste. Manifeste à la racine du paquet : dossier au nom du mod.
    for depth in (0..=dirs.len()).rev() {
        let base = dirs[..depth].join("/");
        let has = |manifest: &str| {
            let path = if base.is_empty() {
                manifest.to_owned()
            } else {
                format!("{base}/{manifest}")
            };
            all.contains(&path.to_ascii_lowercase())
        };
        let mod_folder = dirs
            .get(depth.wrapping_sub(1))
            .copied()
            .filter(|_| depth > 0);
        let rest = parts[depth..].join("/");
        if has("init.lua") {
            return Some(format!(
                "{CET_MODS}/{}/{rest}",
                mod_folder.unwrap_or(folder)
            ));
        }
        if has("info.json") {
            return Some(format!("mods/{}/{rest}", mod_folder.unwrap_or(folder)));
        }
    }

    // 3. Par extension.
    let rest = parts.join("/");
    if lower.ends_with(".archive") || lower.ends_with(".xl") {
        return Some(format!("archive/pc/mod/{name}"));
    }
    if lower.ends_with(".reds") {
        return Some(format!("r6/scripts/{folder}/{rest}"));
    }
    if [".yaml", ".yml", ".tweak"]
        .iter()
        .any(|e| lower.ends_with(e))
    {
        return Some(format!("r6/tweaks/{folder}/{rest}"));
    }
    None
}

/// Nom de dossier sûr et lisible tiré du nom du mod.
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

    fn place(files: &[&str]) -> Vec<(String, String)> {
        let files: Vec<String> = files.iter().map(|f| f.to_string()).collect();
        let mut out: Vec<_> = RedEngine
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
    fn game_layouts_are_kept() {
        assert_eq!(
            place(&[
                "archive/pc/mod/cool.archive",
                "R6/scripts/Cool/cool.reds",
                "red4ext/plugins/Cool/Cool.dll",
                "readme.txt",
            ]),
            pairs(&[
                ("archive/pc/mod/cool.archive", "archive/pc/mod/cool.archive"),
                ("R6/scripts/Cool/cool.reds", "r6/scripts/Cool/cool.reds"),
                (
                    "red4ext/plugins/Cool/Cool.dll",
                    "red4ext/plugins/Cool/Cool.dll"
                ),
            ])
        );
    }

    #[test]
    fn wrapper_folders_are_skipped() {
        assert_eq!(
            place(&[
                "Cool Mod v1.2/archive/pc/mod/cool.archive",
                "Cool Mod v1.2/readme.md"
            ]),
            pairs(&[(
                "Cool Mod v1.2/archive/pc/mod/cool.archive",
                "archive/pc/mod/cool.archive"
            )])
        );
    }

    #[test]
    fn loose_files_are_sorted_by_kind() {
        assert_eq!(
            place(&[
                "cool.archive",
                "cool.archive.xl",
                "Cool.reds",
                "cool.yaml",
                "preview.png"
            ]),
            pairs(&[
                ("cool.archive", "archive/pc/mod/cool.archive"),
                ("cool.archive.xl", "archive/pc/mod/cool.archive.xl"),
                ("Cool.reds", "r6/scripts/Cool Mod/Cool.reds"),
                ("cool.yaml", "r6/tweaks/Cool Mod/cool.yaml"),
            ])
        );
    }

    #[test]
    fn cet_and_redmod_folders_keep_their_name() {
        assert_eq!(
            place(&[
                "BetterHud/init.lua",
                "BetterHud/modules/ui.lua",
                "BetterHud/data.json"
            ]),
            pairs(&[
                (
                    "BetterHud/init.lua",
                    &format!("{CET_MODS}/BetterHud/init.lua")
                ),
                (
                    "BetterHud/modules/ui.lua",
                    &format!("{CET_MODS}/BetterHud/modules/ui.lua")
                ),
                (
                    "BetterHud/data.json",
                    &format!("{CET_MODS}/BetterHud/data.json")
                ),
            ])
        );
        assert_eq!(
            place(&["init.lua", "lib/x.lua"]),
            pairs(&[
                ("init.lua", &format!("{CET_MODS}/Cool Mod/init.lua")),
                ("lib/x.lua", &format!("{CET_MODS}/Cool Mod/lib/x.lua")),
            ])
        );
        assert_eq!(
            place(&["MyRedmod/info.json", "MyRedmod/archives/a.archive"]),
            pairs(&[
                ("MyRedmod/info.json", "mods/MyRedmod/info.json"),
                (
                    "MyRedmod/archives/a.archive",
                    "mods/MyRedmod/archives/a.archive"
                ),
            ])
        );
    }

    #[test]
    fn frameworks_are_needed_by_their_files_but_not_by_themselves() {
        let archive_xl = Framework {
            name: "ArchiveXL".into(),
            nexus_id: Some(4198),
            release: None,
            github: "psiberx/cp2077-archive-xl".into(),
            asset: "ArchiveXL-{version}.zip".into(),
            marker: "red4ext/plugins/ArchiveXL/ArchiveXL.dll".into(),
            needed_by_prefix: vec![],
            needed_by_ext: vec![".xl".into()],
        };
        let frameworks = [archive_xl];
        let placed = |targets: &[&str]| -> Vec<Placement> {
            targets
                .iter()
                .map(|t| Placement {
                    source: t.to_string(),
                    target: t.to_string(),
                })
                .collect()
        };
        assert_eq!(
            needed_frameworks(&frameworks, &placed(&["archive/pc/mod/a.archive.xl"])).len(),
            1
        );
        assert!(needed_frameworks(&frameworks, &placed(&["archive/pc/mod/a.archive"])).is_empty());
        assert!(
            needed_frameworks(
                &frameworks,
                &placed(&[
                    "red4ext/plugins/ArchiveXL/ArchiveXL.dll",
                    "red4ext/plugins/ArchiveXL/Bundle/Migration.xl",
                ])
            )
            .is_empty()
        );
    }
}

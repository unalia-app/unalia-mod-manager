//! Adaptateur **Unreal Engine** (4 et 5), pour les jeux dont les mods
//! viennent de Nexus : Phantom Blade Zero, Hogwarts Legacy…
//!
//! Un jeu Unreal range ses fichiers dans le dossier de son projet,
//! `<Projet>/Content/Paks` et `<Projet>/Binaries/Win64`. Son nom n'est connu
//! qu'une fois le jeu installé : `find_project` le trouve. Puis, d'après le
//! contenu de l'archive, sans deviner au hasard :
//! - `.pak`, `.utoc`, `.ucas` (et `.sig`) → `<Projet>/Content/Paks/~mods/` ;
//! - `LogicMods/…` (mods Blueprint) → `<Projet>/Content/Paks/LogicMods/` ;
//! - dossier avec `Scripts/main.lua` → mod UE4SS,
//!   `<Projet>/Binaries/Win64/ue4ss/Mods/<mod>/` ;
//! - `dwmapi.dll` et `ue4ss/` (UE4SS lui-même) → `<Projet>/Binaries/Win64/` ;
//! - une archive qui reproduit le jeu (`<Projet>/…`) ou le projet
//!   (`Content/…`, `Binaries/…`) se pose telle quelle.
//!
//! Le reste (readme, captures) n'est pas installé.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use super::{Loader, PackageKind, Placement};

/// Extensions des paquets de contenu Unreal (IoStore compris).
const PAK_FILES: [&str; 4] = [".pak", ".utoc", ".ucas", ".sig"];

/// Dossier du projet Unreal : celui qui contient à la fois `Content/Paks` et
/// `Binaries/Win64` (pas `Engine`, présent dans tous les jeux). Plusieurs
/// candidats : on ne devine pas.
pub fn find_project(game_dir: &Path) -> Option<String> {
    let mut found = fs::read_dir(game_dir).ok()?.flatten().filter_map(|entry| {
        let name = entry.file_name().to_string_lossy().into_owned();
        let dir = entry.path();
        (!name.eq_ignore_ascii_case("engine")
            && dir.join("Content").join("Paks").is_dir()
            && dir.join("Binaries").join("Win64").is_dir())
        .then_some(name)
    });
    let first = found.next()?;
    found.next().is_none().then_some(first)
}

pub struct Unreal {
    /// Nom du dossier du projet, ex. `PhantomBladeZero`.
    pub project: String,
}

impl Loader for Unreal {
    fn id(&self) -> &'static str {
        "unreal"
    }

    fn installed_manually(&self, _game_dir: &Path) -> bool {
        // Pas de loader unique : UE4SS se vérifie comme un framework.
        false
    }

    fn place(&self, package_id: &str, _kind: &PackageKind, files: &[String]) -> Vec<Placement> {
        let skip = wrapper_depth(files, &self.project);
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
                    target: self.target(parts, &all, &folder)?,
                })
            })
            .collect()
    }
}

impl Unreal {
    fn target(&self, parts: &[&str], all: &HashSet<String>, folder: &str) -> Option<String> {
        let project = &self.project;
        let win64 = format!("{project}/Binaries/Win64");
        let (name, dirs) = parts.split_last()?;
        let lower = name.to_ascii_lowercase();
        let first = dirs.first().map(|d| d.to_ascii_lowercase());

        // 1. Arborescence du jeu, puis du projet.
        if dirs
            .first()
            .is_some_and(|d| d.eq_ignore_ascii_case(project))
        {
            return Some(format!("{project}/{}", parts[1..].join("/")));
        }
        if matches!(first.as_deref(), Some("content" | "binaries")) {
            return Some(format!("{project}/{}", parts.join("/")));
        }

        // 2. UE4SS lui-même.
        if dirs.is_empty() && lower == "dwmapi.dll" {
            return Some(format!("{win64}/{name}"));
        }
        if first.as_deref() == Some("ue4ss") {
            return Some(format!("{win64}/{}", parts.join("/")));
        }

        // 3. Mod UE4SS : le dossier le plus proche qui contient
        //    `Scripts/main.lua`. À la racine du paquet : dossier au nom du mod.
        for depth in (0..=dirs.len()).rev() {
            let base = dirs[..depth].join("/");
            let main = if base.is_empty() {
                "scripts/main.lua".to_owned()
            } else {
                format!("{}/scripts/main.lua", base.to_ascii_lowercase())
            };
            if all.contains(&main) {
                let mod_folder = if depth > 0 { dirs[depth - 1] } else { folder };
                return Some(format!(
                    "{win64}/ue4ss/Mods/{mod_folder}/{}",
                    parts[depth..].join("/")
                ));
            }
        }

        // 4. Mods Blueprint, puis paquets de contenu.
        if let Some(at) = dirs
            .iter()
            .position(|d| d.eq_ignore_ascii_case("logicmods"))
        {
            return Some(format!(
                "{project}/Content/Paks/LogicMods/{}",
                parts[at + 1..].join("/")
            ));
        }
        if PAK_FILES.iter().any(|e| lower.ends_with(e)) {
            return Some(format!("{project}/Content/Paks/~mods/{name}"));
        }
        None
    }
}

/// Dossiers d'emballage à ignorer (`Mon Mod v1.2/…`), comme pour REDengine :
/// on descend tant que tout le paquet est dans un seul dossier qui n'a pas de
/// sens pour le jeu.
fn wrapper_depth(files: &[String], project: &str) -> usize {
    let meaningful = |segment: &str| {
        segment.eq_ignore_ascii_case(project)
            || [
                "content",
                "binaries",
                "ue4ss",
                "logicmods",
                "~mods",
                "scripts",
            ]
            .iter()
            .any(|s| segment.eq_ignore_ascii_case(s))
    };
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
            // `Mod/Scripts/main.lua` : `Mod` est le nom du mod UE4SS, on le garde.
            if parts.len() >= 3 && parts[1].eq_ignore_ascii_case("scripts") {
                return depth;
            }
        }
        match first {
            Some(f) if !meaningful(f) => depth += 1,
            _ => return depth,
        }
    }
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

    const P: &str = "PhantomBladeZero";

    fn place(files: &[&str]) -> Vec<(String, String)> {
        let files: Vec<String> = files.iter().map(|f| f.to_string()).collect();
        let mut out: Vec<_> = Unreal { project: P.into() }
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
    fn pak_mods_go_to_tilde_mods() {
        assert_eq!(
            place(&[
                "Better Swords/BetterSwords_P.pak",
                "Better Swords/BetterSwords_P.utoc",
                "Better Swords/BetterSwords_P.ucas",
                "Better Swords/readme.txt",
            ]),
            pairs(&[
                (
                    "Better Swords/BetterSwords_P.pak",
                    "PhantomBladeZero/Content/Paks/~mods/BetterSwords_P.pak"
                ),
                (
                    "Better Swords/BetterSwords_P.utoc",
                    "PhantomBladeZero/Content/Paks/~mods/BetterSwords_P.utoc"
                ),
                (
                    "Better Swords/BetterSwords_P.ucas",
                    "PhantomBladeZero/Content/Paks/~mods/BetterSwords_P.ucas"
                ),
            ])
        );
    }

    #[test]
    fn game_and_project_layouts_are_kept() {
        assert_eq!(
            place(&[
                "PhantomBladeZero/Content/Paks/~mods/A_P.pak",
                "Content/Paks/LogicMods/B.pak",
            ]),
            pairs(&[
                (
                    "PhantomBladeZero/Content/Paks/~mods/A_P.pak",
                    "PhantomBladeZero/Content/Paks/~mods/A_P.pak"
                ),
                (
                    "Content/Paks/LogicMods/B.pak",
                    "PhantomBladeZero/Content/Paks/LogicMods/B.pak"
                ),
            ])
        );
    }

    #[test]
    fn ue4ss_and_its_lua_mods_go_to_binaries() {
        assert_eq!(
            place(&["dwmapi.dll", "ue4ss/UE4SS.dll", "ue4ss/Mods/mods.txt"]),
            pairs(&[
                ("dwmapi.dll", "PhantomBladeZero/Binaries/Win64/dwmapi.dll"),
                (
                    "ue4ss/UE4SS.dll",
                    "PhantomBladeZero/Binaries/Win64/ue4ss/UE4SS.dll"
                ),
                (
                    "ue4ss/Mods/mods.txt",
                    "PhantomBladeZero/Binaries/Win64/ue4ss/Mods/mods.txt"
                ),
            ])
        );
        assert_eq!(
            place(&["NoStamina/Scripts/main.lua", "NoStamina/enabled.txt"]),
            pairs(&[
                (
                    "NoStamina/Scripts/main.lua",
                    "PhantomBladeZero/Binaries/Win64/ue4ss/Mods/NoStamina/Scripts/main.lua"
                ),
                (
                    "NoStamina/enabled.txt",
                    "PhantomBladeZero/Binaries/Win64/ue4ss/Mods/NoStamina/enabled.txt"
                ),
            ])
        );
        // Lua à la racine du paquet : dossier au nom du mod.
        assert_eq!(
            place(&["Scripts/main.lua"]),
            pairs(&[(
                "Scripts/main.lua",
                "PhantomBladeZero/Binaries/Win64/ue4ss/Mods/Cool Mod/Scripts/main.lua"
            )])
        );
    }

    #[test]
    fn logic_mods_keep_their_folder() {
        assert_eq!(
            place(&["MyBP v2/LogicMods/MyBP.pak"]),
            pairs(&[(
                "MyBP v2/LogicMods/MyBP.pak",
                "PhantomBladeZero/Content/Paks/LogicMods/MyBP.pak"
            )])
        );
    }

    #[test]
    fn the_project_folder_is_found_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let game = dir.path();
        std::fs::create_dir_all(game.join("Engine/Binaries/Win64")).unwrap();
        std::fs::create_dir_all(game.join("Engine/Content/Paks")).unwrap();
        assert_eq!(find_project(game), None);
        std::fs::create_dir_all(game.join(P).join("Content/Paks")).unwrap();
        std::fs::create_dir_all(game.join(P).join("Binaries/Win64")).unwrap();
        assert_eq!(find_project(game).as_deref(), Some(P));
    }
}

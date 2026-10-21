//! Placement par les `installRules` du schéma Thunderstore, sur les vraies
//! règles des jeux de `fixtures/thunderstore/schema.json`.

use std::path::Path;

use mod_engine::games::GameCatalog;
use mod_engine::loaders::rules::RulesLoader;
use mod_engine::loaders::{Loader, PackageKind};

fn catalog() -> GameCatalog {
    let schema = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/thunderstore/schema.json"),
    )
    .unwrap();
    GameCatalog::from_schema_json(&schema).unwrap()
}

fn place(game: &str, kind: PackageKind, files: &[&str]) -> Vec<(String, String)> {
    let catalog = catalog();
    let profile = catalog.by_id(game).unwrap();
    let loader = RulesLoader {
        loader: &profile.loader,
        rules: &profile.install_rules,
    };
    let files: Vec<String> = files.iter().map(|f| f.to_string()).collect();
    let mut placed: Vec<_> = loader
        .place("Author-Cool_Mod", &kind, &files)
        .into_iter()
        .map(|p| (p.source, p.target))
        .collect();
    placed.sort();
    placed
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
fn bepinex_games_behave_like_the_hand_written_adapter() {
    let placed = place(
        "hollow-knight-silksong",
        PackageKind::Mod,
        &[
            "plugins/CoolMod.dll",
            "plugins/assets/sprite.png",
            "patchers/CoolPatcher.dll",
            "config/com.author.cool.cfg",
            "Loose.dll",
            "data/levels.json",
            "BepInEx/plugins/Nested.dll",
            "manifest.json",
            "icon.png",
            "README.md",
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
            (
                "BepInEx/plugins/Nested.dll",
                "BepInEx/plugins/Author-Cool_Mod/Nested.dll"
            ),
        ])
    );
}

#[test]
fn melonloader_games_follow_their_rules() {
    let placed = place(
        "backpack-hero",
        PackageKind::Mod,
        &[
            "Mods/CoolMod.dll",
            "Loose.dll",
            "Shared.lib.dll",
            "UserLibs/Other.lib.dll",
            "MelonLoader/Managed/Patch.dll",
            "notes/readme.txt",
            "LICENCE",
            "manifest.json",
        ],
    );
    assert_eq!(
        placed,
        pairs(&[
            ("Mods/CoolMod.dll", "Mods/CoolMod.dll"),
            ("Loose.dll", "Mods/Loose.dll"),
            // L'extension la plus longue gagne : `.lib.dll` avant `.dll`.
            ("Shared.lib.dll", "UserLibs/Shared.lib.dll"),
            ("UserLibs/Other.lib.dll", "UserLibs/Other.lib.dll"),
            // Sous-route aplatie.
            (
                "MelonLoader/Managed/Patch.dll",
                "MelonLoader/Managed/Patch.dll"
            ),
            // Route par défaut en subdir-no-flatten : chemin complet gardé.
            (
                "notes/readme.txt",
                "UserData/ModManager/Author-Cool_Mod/notes/readme.txt"
            ),
        ])
    );
}

#[test]
fn recursive_melonloader_games_use_melonloader_conventions() {
    let placed = place(
        "dale-dawson-stationery-supplies",
        PackageKind::Mod,
        &["Mods/A.dll", "B.dll", "UserData/settings.cfg", "C.lib.dll"],
    );
    assert_eq!(
        placed,
        pairs(&[
            ("Mods/A.dll", "Mods/A.dll"),
            ("B.dll", "Mods/B.dll"),
            ("UserData/settings.cfg", "UserData/settings.cfg"),
            ("C.lib.dll", "UserLibs/C.lib.dll"),
        ])
    );
}

#[test]
fn unreal_shimloader_games_sort_paks_and_configs() {
    let placed = place(
        "palworld",
        PackageKind::Mod,
        &["pak/CoolMod_P.pak", "cfg/cool.ini", "lua/main.lua"],
    );
    assert_eq!(
        placed,
        pairs(&[
            (
                "pak/CoolMod_P.pak",
                "shimloader/pak/Author-Cool_Mod/CoolMod_P.pak"
            ),
            ("cfg/cool.ini", "shimloader/cfg/cool.ini"),
            (
                "lua/main.lua",
                "shimloader/mod/Author-Cool_Mod/lua/main.lua"
            ),
        ])
    );
}

#[test]
fn loader_packages_without_root_folder_go_to_the_game_root() {
    // MelonLoader : tout le paquet, sauf les métadonnées, à la racine du jeu.
    let placed = place(
        "backpack-hero",
        PackageKind::Loader {
            root_folder: String::new(),
        },
        &[
            "version.dll",
            "MelonLoader/net6/MelonLoader.dll",
            "manifest.json",
            "icon.png",
        ],
    );
    assert_eq!(
        placed,
        pairs(&[
            ("version.dll", "version.dll"),
            (
                "MelonLoader/net6/MelonLoader.dll",
                "MelonLoader/net6/MelonLoader.dll"
            ),
        ])
    );
}

#[test]
fn a_manual_melonloader_install_is_detected() {
    let catalog = catalog();
    let profile = catalog.by_id("backpack-hero").unwrap();
    let loader = RulesLoader {
        loader: &profile.loader,
        rules: &profile.install_rules,
    };
    let dir = tempfile::tempdir().unwrap();
    assert!(!loader.installed_manually(dir.path()));
    std::fs::create_dir(dir.path().join("MelonLoader")).unwrap();
    assert!(loader.installed_manually(dir.path()));
}

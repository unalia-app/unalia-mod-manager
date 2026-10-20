//! Tests de la brique detect sur les faux dossiers Steam de `fixtures/steam`.

use std::fs;
use std::path::{Path, PathBuf};

use mod_engine::detect::{DetectError, Engine, SteamGame, library_folders, scan_steam};

/// Copie `fixtures/steam` dans un dossier temporaire et remplace `{{FIXTURE}}`
/// dans les `.vdf` par le chemin de cette copie, échappé comme le fait Steam.
fn fixture() -> tempfile::TempDir {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/steam");
    let dir = tempfile::tempdir().unwrap();
    let escaped = dir.path().to_str().unwrap().replace('\\', "\\\\");
    copy_dir(&source, dir.path(), &escaped);
    dir
}

fn copy_dir(from: &Path, to: &Path, fixture_path: &str) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target, fixture_path);
        } else if entry.path().extension().is_some_and(|e| e == "vdf") {
            let text = fs::read_to_string(entry.path()).unwrap();
            fs::write(target, text.replace("{{FIXTURE}}", fixture_path)).unwrap();
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn find(games: &[SteamGame], app_id: u32) -> &SteamGame {
    games
        .iter()
        .find(|g| g.app_id == app_id)
        .unwrap_or_else(|| panic!("jeu {app_id} absent de {games:#?}"))
}

fn ids(games: &[SteamGame]) -> Vec<u32> {
    games.iter().map(|g| g.app_id).collect()
}

#[test]
fn lists_games_of_every_library_sorted_by_name() {
    let fx = fixture();
    let games = scan_steam(&fx.path().join("root")).unwrap();

    let names: Vec<_> = games.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Faux Jeu IL2CPP",
            "Faux Jeu Unreal",
            "Hollow Knight: Silksong",
            "Steamworks Common Redistributables",
        ]
    );
}

#[test]
fn resolves_install_dir_and_library() {
    let fx = fixture();
    let root = fx.path().join("root");
    let games = scan_steam(&root).unwrap();

    let silksong = find(&games, 1030300);
    assert_eq!(silksong.library, root);
    assert_eq!(
        silksong.install_dir,
        root.join("steamapps/common/Hollow Knight Silksong")
    );

    let unreal = find(&games, 2000001);
    assert_eq!(
        fs::canonicalize(&unreal.install_dir).unwrap(),
        fs::canonicalize(fx.path().join("library-d/steamapps/common/FauxJeuUnreal")).unwrap()
    );
}

#[test]
fn identifies_engines() {
    let fx = fixture();
    let games = scan_steam(&fx.path().join("root")).unwrap();

    assert_eq!(find(&games, 1030300).engine, Engine::Unity);
    assert_eq!(find(&games, 2000002).engine, Engine::Unity);
    assert_eq!(find(&games, 2000001).engine, Engine::Unreal);
    assert_eq!(find(&games, 228980).engine, Engine::Unknown);
}

#[test]
fn skips_uninstalled_corrupt_and_trapped_manifests() {
    let fx = fixture();
    let games = scan_steam(&fx.path().join("root")).unwrap();
    let ids = ids(&games);

    assert!(!ids.contains(&440), "jeu désinstallé listé");
    assert!(!ids.contains(&999), "manifeste corrompu listé");
    assert!(!ids.contains(&666), "installdir hors de common/ accepté");
    assert!(
        !games.iter().any(|g| g.name.contains("Orphelin")),
        "dossier sans manifeste listé"
    );
}

#[test]
fn a_game_present_in_two_libraries_is_listed_once_from_the_first() {
    let fx = fixture();
    let root = fx.path().join("root");
    let games = scan_steam(&root).unwrap();

    assert_eq!(ids(&games).iter().filter(|&&id| id == 1030300).count(), 1);
    assert_eq!(find(&games, 1030300).library, root);
}

#[test]
fn library_folders_dedupes_root_and_drops_missing_drives() {
    let fx = fixture();
    let root = fx.path().join("root");
    let libraries = library_folders(&root);

    assert_eq!(libraries.len(), 2, "{libraries:#?}");
    assert_eq!(libraries[0], root);
    assert_eq!(
        fs::canonicalize(&libraries[1]).unwrap(),
        fs::canonicalize(fx.path().join("library-d")).unwrap()
    );
}

#[test]
fn reads_legacy_libraryfolders_format() {
    let fx = fixture();
    let games = scan_steam(&fx.path().join("legacy")).unwrap();

    assert_eq!(ids(&games), [2000002, 2000001, 1030300]);
    // Dans cette config, Silksong n'est que dans library-d.
    let silksong = find(&games, 1030300);
    assert_eq!(silksong.engine, Engine::Unity);
    assert!(silksong.library.ends_with("library-d"));
}

#[test]
fn works_without_libraryfolders_vdf() {
    let fx = fixture();
    let root = fx.path().join("root");
    fs::remove_file(root.join("steamapps/libraryfolders.vdf")).unwrap();

    let games = scan_steam(&root).unwrap();
    assert_eq!(ids(&games), [1030300, 228980]);
}

#[test]
fn corrupt_libraryfolders_falls_back_to_root_library() {
    let fx = fixture();
    let root = fx.path().join("root");
    fs::write(
        root.join("steamapps/libraryfolders.vdf"),
        "\"libraryfolders\" {",
    )
    .unwrap();

    assert_eq!(library_folders(&root), [root]);
}

#[test]
fn missing_steam_is_an_error() {
    let fx = fixture();
    let nowhere: PathBuf = fx.path().join("pas-de-steam");

    match scan_steam(&nowhere) {
        Err(DetectError::SteamNotFound(path)) => assert_eq!(path, nowhere),
        other => panic!("attendu SteamNotFound, obtenu {other:?}"),
    }
}

#[test]
fn serializes_for_the_interface_in_camel_case() {
    let fx = fixture();
    let games = scan_steam(&fx.path().join("root")).unwrap();
    let json = serde_json::to_value(find(&games, 1030300)).unwrap();

    assert_eq!(json["appId"], 1030300);
    assert_eq!(json["engine"], "unity");
    assert!(json["installDir"].is_string());
}

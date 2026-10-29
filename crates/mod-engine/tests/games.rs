//! Tests of the game catalog on a real excerpt of the Thunderstore schema
//! (`fixtures/thunderstore/schema.json`).

use std::fs;
use std::path::Path;
use std::time::Duration;

use mockito::Server;
use mod_engine::games::{CatalogError, GameCatalog, load_catalog};

fn schema() -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/thunderstore/schema.json"),
    )
    .unwrap()
}

fn catalog() -> GameCatalog {
    GameCatalog::from_schema_json(&schema()).unwrap()
}

#[test]
fn maps_steam_app_ids_to_profiles() {
    let catalog = catalog();
    let silksong = catalog.by_steam_app_id(1030300).unwrap();

    assert_eq!(silksong.id, "hollow-knight-silksong");
    assert_eq!(silksong.name, "Hollow Knight: Silksong");
    assert_eq!(silksong.community, "hollow-knight-silksong");
    assert_eq!(silksong.loader, "bepinex");
    assert!(silksong.supported);
    assert_eq!(
        silksong.data_folder_name.as_deref(),
        Some("Hollow Knight Silksong_Data")
    );
    assert!(
        silksong
            .exe_names
            .iter()
            .any(|e| e == "Hollow Knight Silksong.exe")
    );
    assert_eq!(
        silksong.cover_url.as_deref(),
        Some(
            "https://gcdn.thunderstore.io/live/community/hollow-knight-silksong/hollow-knight-silksong-cover-360x480.webp"
        )
    );
    assert!(
        silksong
            .hero_url
            .as_deref()
            .unwrap()
            .ends_with("-bg-1920x620.webp")
    );

    assert_eq!(
        catalog.by_steam_app_id(1966720).unwrap().id,
        "lethal-company"
    );
    assert_eq!(catalog.by_steam_app_id(892970).unwrap().id, "valheim");
    assert!(catalog.by_steam_app_id(730).is_none());
}

#[test]
fn loaders_with_install_rules_are_supported() {
    let catalog = catalog();
    for (id, loader) in [
        ("backpack-hero", "melonloader"),
        ("palworld", "shimloader"),
        // No rules in the schema: MelonLoader conventions by default.
        ("dale-dawson-stationery-supplies", "recursive-melonloader"),
    ] {
        let game = catalog.by_id(id).unwrap();
        assert_eq!(game.loader, loader);
        assert!(game.supported, "{id} should be supported");
        assert!(game.install_rules.installable());
    }
}

#[test]
fn games_whose_mods_ship_as_archives_are_not_supported_yet() {
    // Godot: the archive is placed as is (package-zip).
    let brotato = catalog().by_id("brotato").unwrap().clone();
    assert_eq!(brotato.loader, "godotml");
    assert!(!brotato.supported);
}

#[test]
fn games_without_an_installable_entry_are_skipped() {
    let catalog = catalog();
    assert!(catalog.by_id("107zxz-inscryption-multiplayer").is_none());
    // 7 games from the schema, plus Cyberpunk 2077, Phantom Blade Zero,
    // Skyrim SE and Fallout 4 (hand-written profiles).
    assert_eq!(catalog.games().len(), 11);
}

#[test]
fn cyberpunk_comes_from_its_handwritten_profile() {
    let catalog = catalog();
    let cyberpunk = catalog.by_steam_app_id(1091500).unwrap();
    assert_eq!(cyberpunk.id, "cyberpunk-2077");
    assert_eq!(cyberpunk.loader, "redengine");
    assert!(cyberpunk.supported);
    assert_eq!(cyberpunk.community, "nexus:cyberpunk2077");
    assert_eq!(cyberpunk.nexus.as_ref().unwrap().game_id, 3333);
    let names: Vec<_> = cyberpunk
        .frameworks
        .iter()
        .map(|f| f.name.as_str())
        .collect();
    assert!(
        names.contains(&"RED4ext") && names.contains(&"redscript"),
        "{names:?}"
    );

    // GOG, Epic: recognized by its executable, stored in `bin/x64`.
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("bin/x64")).unwrap();
    assert!(catalog.identify_folder(dir.path()).is_none());
    std::fs::write(dir.path().join("bin/x64/Cyberpunk2077.exe"), b"exe").unwrap();
    assert_eq!(
        catalog.identify_folder(dir.path()).unwrap().id,
        "cyberpunk-2077"
    );
}

#[test]
fn sub_routes_are_flattened() {
    let backpack = catalog().by_id("backpack-hero").unwrap().clone();
    let routes: Vec<_> = backpack
        .install_rules
        .rules
        .iter()
        .map(|r| r.route.as_str())
        .collect();
    assert!(routes.contains(&"MelonLoader/Managed"), "{routes:?}");
    assert!(routes.contains(&"MelonLoader/Libs"));
}

#[test]
fn knows_which_packages_are_loaders() {
    let catalog = catalog();
    let pack = catalog
        .loader_package("silksong_modding-BepInExPack_Silksong")
        .unwrap();
    assert_eq!(pack.loader, "bepinex");
    assert_eq!(pack.root_folder, "BepInExPack");
    assert!(
        catalog
            .loader_package("shownyoung-KnightInSilksong")
            .is_none()
    );
}

#[test]
fn identifies_a_folder_added_by_hand() {
    let catalog = catalog();
    let dir = tempfile::tempdir().unwrap();

    // Unity game outside Steam: the data folder is enough.
    fs::create_dir(dir.path().join("Hollow Knight Silksong_Data")).unwrap();
    fs::write(dir.path().join("Hollow Knight Silksong.exe"), "").unwrap();
    assert_eq!(
        catalog.identify_folder(dir.path()).unwrap().id,
        "hollow-knight-silksong"
    );

    // By the executable alone, case ignored.
    let lc = tempfile::tempdir().unwrap();
    fs::write(lc.path().join("lethal company.EXE"), "").unwrap();
    assert_eq!(
        catalog.identify_folder(lc.path()).unwrap().id,
        "lethal-company"
    );

    let unknown = tempfile::tempdir().unwrap();
    fs::write(unknown.path().join("Game.exe"), "").unwrap();
    assert!(catalog.identify_folder(unknown.path()).is_none());
}

#[tokio::test]
async fn downloads_then_reuses_the_cache() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/schema")
        .with_body(schema())
        .expect(1)
        .create_async()
        .await;
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("cache").join("schema.json");
    let client = reqwest::Client::new();
    let url = format!("{}/schema", server.url());

    let first = load_catalog(&client, &url, &cache, Duration::from_secs(3600))
        .await
        .unwrap();
    assert!(cache.is_file());
    let second = load_catalog(&client, &url, &cache, Duration::from_secs(3600))
        .await
        .unwrap();

    mock.assert_async().await;
    assert_eq!(first.games(), second.games());
}

#[tokio::test]
async fn offline_falls_back_to_a_stale_cache() {
    let mut server = Server::new_async().await;
    server
        .mock("GET", "/schema")
        .with_status(503)
        .create_async()
        .await;
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("schema.json");
    fs::write(&cache, schema()).unwrap();

    // Zero max_age: the cache is stale, the download fails, we keep it.
    let catalog = load_catalog(
        &reqwest::Client::new(),
        &format!("{}/schema", server.url()),
        &cache,
        Duration::ZERO,
    )
    .await
    .unwrap();
    assert!(catalog.by_steam_app_id(1030300).is_some());
}

#[tokio::test]
async fn a_broken_download_never_overwrites_a_good_cache() {
    let mut server = Server::new_async().await;
    server
        .mock("GET", "/schema")
        .with_body("{ not json")
        .create_async()
        .await;
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("schema.json");
    fs::write(&cache, schema()).unwrap();

    let result = load_catalog(
        &reqwest::Client::new(),
        &format!("{}/schema", server.url()),
        &cache,
        Duration::ZERO,
    )
    .await;
    assert!(matches!(result, Err(CatalogError::Schema(_))), "{result:?}");
    assert_eq!(fs::read_to_string(&cache).unwrap(), schema());
}

#[tokio::test]
async fn no_cache_and_no_network_is_an_error() {
    let mut server = Server::new_async().await;
    server
        .mock("GET", "/schema")
        .with_status(500)
        .create_async()
        .await;
    let dir = tempfile::tempdir().unwrap();

    let result = load_catalog(
        &reqwest::Client::new(),
        &format!("{}/schema", server.url()),
        &dir.path().join("schema.json"),
        Duration::from_secs(3600),
    )
    .await;
    assert!(
        matches!(result, Err(CatalogError::Download(_))),
        "{result:?}"
    );
}

#[test]
fn manual_games_reuse_the_steam_id_or_get_a_stable_one() {
    use mod_engine::games::manual_app_id;
    let catalog = catalog();
    assert_eq!(
        manual_app_id(catalog.by_id("hollow-knight-silksong").unwrap()),
        1030300
    );

    let mut no_steam = catalog.by_id("valheim").unwrap().clone();
    no_steam.steam_app_ids.clear();
    let id = manual_app_id(&no_steam);
    assert_eq!(id, manual_app_id(&no_steam), "must be stable");
    assert!(
        id & 0x8000_0000 != 0,
        "must never collide with a Steam id"
    );
}

#[test]
fn manual_games_are_persisted() {
    use mod_engine::state::{ManualGame, State};
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("modalia.db");
    let game = ManualGame {
        app_id: 1030300,
        profile_id: "hollow-knight-silksong".into(),
        path: "D:/Games/Silksong".into(),
        added_at: 1,
    };
    let mut state = State::open(&db).unwrap();
    state.save_manual_game(&game).unwrap();
    // The same game moved: replaced, not duplicated.
    let moved = ManualGame {
        path: "E:/Silksong".into(),
        ..game.clone()
    };
    state.save_manual_game(&moved).unwrap();
    drop(state);

    let mut state = State::open(&db).unwrap();
    assert_eq!(state.manual_games().unwrap(), [moved]);
    state.remove_manual_game(1030300).unwrap();
    assert!(state.manual_games().unwrap().is_empty());
}

#[test]
fn store_paths_can_be_rebased_after_a_move() {
    use mod_engine::deploy::Deployment;
    use mod_engine::state::{InstalledMod, State};
    let mut state = State::open_in_memory().unwrap();
    let record = |id: &str, dir: &str| InstalledMod {
        game: "steam:1".into(),
        id: id.into(),
        name: id.into(),
        author: "a".into(),
        version: "1.0.0".into(),
        icon_url: None,
        loader_root: None,
        enabled: true,
        explicit: true,
        sha256: String::new(),
        installed_at: 0,
        package: String::new(),
        legacy_store_dir: dir.into(),
        files: Vec::new(),
        deployment: Deployment::default(),
        placements: Vec::new(),
        edits: Default::default(),
        previous_versions: Vec::new(),
    };
    state
        .save_all(&[
            record("a-One", "C:/Roaming/mods/steam-1/a-One/1.0.0"),
            record("b-Two", "D:/Elsewhere/b-Two"),
        ])
        .unwrap();

    state
        .rebase_store(Path::new("C:/Roaming/mods"), Path::new("C:/Local/mods"))
        .unwrap();
    assert_eq!(
        state.get("steam:1", "a-One").unwrap().unwrap().legacy_store_dir,
        "C:/Local/mods/steam-1/a-One/1.0.0"
    );
    assert_eq!(
        state.get("steam:1", "b-Two").unwrap().unwrap().legacy_store_dir,
        "D:/Elsewhere/b-Two",
        "a path outside the old storage doesn't move"
    );
}

#[test]
fn locating_a_game_tolerates_its_parent_folder() {
    let catalog = catalog();
    let games = tempfile::tempdir().unwrap();
    let silksong = games.path().join("Silksong");
    fs::create_dir_all(silksong.join("Hollow Knight Silksong_Data")).unwrap();
    fs::create_dir_all(games.path().join("Photos")).unwrap();

    let (profile, dir) = catalog.locate_game(games.path()).unwrap();
    assert_eq!(profile.id, "hollow-knight-silksong");
    assert_eq!(dir, silksong);
    // The right folder directly: unchanged.
    assert_eq!(catalog.locate_game(&silksong).unwrap().1, silksong);

    // Two games in the parent: ambiguous, we don't guess.
    fs::create_dir_all(games.path().join("LC")).unwrap();
    fs::write(games.path().join("LC/Lethal Company.exe"), "").unwrap();
    assert!(catalog.locate_game(games.path()).is_none());
}

#[test]
fn pending_changes_survive_a_restart() {
    use mod_engine::launch::PendingOp;
    use mod_engine::state::State;
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("modalia.db");
    let ops = vec![
        PendingOp::Install {
            mod_id: "a-A".into(),
        },
        PendingOp::SetEnabled {
            mod_id: "b-B".into(),
            enabled: false,
        },
    ];
    State::open(&db)
        .unwrap()
        .set_pending("steam:1", &ops)
        .unwrap();

    let mut state = State::open(&db).unwrap();
    assert_eq!(state.pending("steam:1").unwrap(), ops);
    assert!(state.pending("steam:2").unwrap().is_empty());
    state.set_pending("steam:1", &[]).unwrap();
    assert!(state.pending("steam:1").unwrap().is_empty());
}

#[test]
fn detects_a_real_running_process() {
    use mod_engine::launch::{ProcessWatcher, is_running};
    // This test's binary runs from its folder: it must be seen.
    let exe = std::env::current_exe().unwrap();
    let dir = exe.parent().unwrap();
    let running = ProcessWatcher::default().running_exes();
    assert!(
        is_running(dir, &running),
        "{} missing from the process list",
        exe.display()
    );
    assert!(!is_running(
        Path::new("/nonexistent/modalia-game"),
        &running
    ));
}

#[test]
fn phantom_blade_zero_is_ready_for_launch() {
    let catalog = catalog();
    let game = catalog.by_steam_app_id(4115450).unwrap();
    assert_eq!(game.id, "phantom-blade-zero");
    assert_eq!(game.loader, "unreal");
    assert!(game.supported);
    assert_eq!(game.community, "nexus:phantombladezero");
    assert_eq!(game.exe_names, ["PBZ.exe"]);
    let ue4ss = &game.frameworks[0];
    assert_eq!(ue4ss.key(), "nexus-13");
    let ue4ss = ue4ss.resolve(Some("ProjectPBZ"), Some("platform/steam"));
    assert_eq!(ue4ss.marker, "platform/steam/ue4ss/UE4SS.dll");
    assert!(ue4ss.needed_by("platform/steam/ue4ss/Mods/NoStamina/Scripts/main.lua"));
    assert!(ue4ss.needed_by("ProjectPBZ/Content/Paks/LogicMods/MyBP.pak"));
    assert!(!ue4ss.needed_by("ProjectPBZ/Content/Paks/~mods/A_P.pak"));
}

#[test]
fn bethesda_games_come_from_their_handwritten_profiles() {
    let catalog = catalog();
    for (app_id, id, domain, plugins) in [
        (489830, "skyrim-special-edition", "skyrimspecialedition", "Skyrim Special Edition"),
        (377160, "fallout-4", "fallout4", "Fallout4"),
    ] {
        let game = catalog.by_steam_app_id(app_id).unwrap();
        assert_eq!(game.id, id);
        assert_eq!(game.loader, "bethesda");
        assert!(game.supported);
        assert_eq!(game.community, format!("nexus:{domain}"));
        assert_eq!(game.plugins.as_deref(), Some(plugins));
    }
}

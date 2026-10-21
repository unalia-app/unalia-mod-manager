//! Tests du catalogue de jeux sur un extrait réel du schéma Thunderstore
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
        // Pas de règles dans le schéma : conventions MelonLoader par défaut.
        ("dale-dawson-stationery-supplies", "recursive-melonloader"),
    ] {
        let game = catalog.by_id(id).unwrap();
        assert_eq!(game.loader, loader);
        assert!(game.supported, "{id} devrait être pris en charge");
        assert!(game.install_rules.installable());
    }
}

#[test]
fn games_whose_mods_ship_as_archives_are_not_supported_yet() {
    // Godot : l'archive est posée telle quelle (package-zip).
    let brotato = catalog().by_id("brotato").unwrap().clone();
    assert_eq!(brotato.loader, "godotml");
    assert!(!brotato.supported);
}

#[test]
fn games_without_an_installable_entry_are_skipped() {
    let catalog = catalog();
    assert!(catalog.by_id("107zxz-inscryption-multiplayer").is_none());
    // 7 jeux du schéma, plus Cyberpunk 2077 (profil écrit à la main).
    assert_eq!(catalog.games().len(), 8);
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

    // GOG, Epic : reconnu par son exécutable, rangé dans `bin/x64`.
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

    // Jeu Unity hors Steam : le dossier de données suffit.
    fs::create_dir(dir.path().join("Hollow Knight Silksong_Data")).unwrap();
    fs::write(dir.path().join("Hollow Knight Silksong.exe"), "").unwrap();
    assert_eq!(
        catalog.identify_folder(dir.path()).unwrap().id,
        "hollow-knight-silksong"
    );

    // Par l'exécutable seul, casse ignorée.
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

    // max_age nul : le cache est périmé, le téléchargement échoue, on le garde.
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
        .with_body("{ pas du json")
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
    assert_eq!(id, manual_app_id(&no_steam), "doit être stable");
    assert!(
        id & 0x8000_0000 != 0,
        "ne doit pas pouvoir croiser un identifiant Steam"
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
    // Le même jeu déplacé : remplacé, pas dupliqué.
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
        store_dir: dir.into(),
        deployment: Deployment::default(),
        placements: Vec::new(),
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
        state.get("steam:1", "a-One").unwrap().unwrap().store_dir,
        "C:/Local/mods/steam-1/a-One/1.0.0"
    );
    assert_eq!(
        state.get("steam:1", "b-Two").unwrap().unwrap().store_dir,
        "D:/Elsewhere/b-Two",
        "un chemin hors de l'ancien stockage ne bouge pas"
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
    // Le bon dossier directement : inchangé.
    assert_eq!(catalog.locate_game(&silksong).unwrap().1, silksong);

    // Deux jeux dans le parent : ambigu, on ne devine pas.
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
    // Le binaire de ce test tourne depuis son dossier : il doit être vu.
    let exe = std::env::current_exe().unwrap();
    let dir = exe.parent().unwrap();
    let running = ProcessWatcher::default().running_exes();
    assert!(
        is_running(dir, &running),
        "{} absent de la liste des processus",
        exe.display()
    );
    assert!(!is_running(
        Path::new("/nonexistent/modalia-game"),
        &running
    ));
}

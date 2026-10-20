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
fn other_loaders_are_known_but_not_supported() {
    let catalog = catalog();
    let backpack = catalog.by_id("backpack-hero").unwrap();
    assert_eq!(backpack.loader, "melonloader");
    assert!(!backpack.supported);
}

#[test]
fn games_without_an_installable_entry_are_skipped() {
    let catalog = catalog();
    assert!(catalog.by_id("107zxz-inscryption-multiplayer").is_none());
    assert_eq!(catalog.games().len(), 4);
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

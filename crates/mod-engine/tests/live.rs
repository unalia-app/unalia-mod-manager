//! Against the real APIs (Nexus, GitHub): checks that the requests still
//! match their schemas. Outside the normal suite:
//! `cargo test -p mod-engine --test live -- --ignored`. The Nexus account is
//! tested if `NEXUS_API_KEY` is set.

use std::path::Path;

use mod_engine::games::GameCatalog;
use mod_engine::sources::github::GitHub;
use mod_engine::sources::nexus::Nexus;
use mod_engine::sources::{ModSource, SearchQuery, Sort};

const COMMUNITY: &str = "nexus:cyberpunk2077";

fn catalog() -> GameCatalog {
    let schema = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/thunderstore/schema.json"),
    )
    .unwrap();
    GameCatalog::from_schema_json(&schema).unwrap()
}

#[tokio::test]
#[ignore]
async fn nexus_queries_match_the_real_schema() {
    let nexus = Nexus::new(mod_engine::http_client());
    for sort in [Sort::Popular, Sort::Downloads, Sort::Newest, Sort::Updated] {
        let page = nexus
            .search(
                COMMUNITY,
                &SearchQuery {
                    sort,
                    page: 2,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(page.items.len(), 24, "{sort:?}");
        assert!(page.has_more);
        // Libraries are left out (Modalia installs them by itself).
        assert!(
            page.items
                .iter()
                .all(|m| m.categories != ["Modders Resources"])
        );
    }
    let found = nexus
        .search(
            COMMUNITY,
            &SearchQuery {
                text: "hud".into(),
                page: 1,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(!found.items.is_empty());

    let archive_xl = nexus.get(COMMUNITY, "nexus-4198").await.unwrap();
    assert_eq!(archive_xl.name, "ArchiveXL");
    assert!(!archive_xl.versions.is_empty());
    assert!(
        archive_xl.versions[0]
            .dependencies
            .iter()
            .any(|d| d.id == "nexus-2380")
    );
    let description = nexus
        .description(COMMUNITY, "nexus-4198", "")
        .await
        .unwrap();
    assert!(description.contains("ArchiveXL") || description.contains("ARCHIVE-XL"));
    assert!(!description.contains("<br"));

    let info = nexus.mod_info("cyberpunk2077", 4198).await.unwrap();
    assert!(info.main_file().is_some());
    let latest = nexus
        .latest_versions("cyberpunk2077", &[107, 4198])
        .await
        .unwrap();
    assert_eq!(latest.len(), 2);
}

#[tokio::test]
#[ignore]
async fn nexus_account_and_free_download_rules() {
    let Ok(key) = std::env::var("NEXUS_API_KEY") else {
        eprintln!("NEXUS_API_KEY not set: skipped");
        return;
    };
    let nexus = Nexus::new(mod_engine::http_client());
    nexus.set_api_key(Some(key));
    let account = nexus.account().await.unwrap();
    let info = nexus.mod_info("cyberpunk2077", 4198).await.unwrap();
    let file = info.main_file().unwrap().file_id;
    let link = nexus.download_url("cyberpunk2077", 4198, file, None).await;
    if account.premium {
        assert!(link.unwrap().starts_with("https://"));
    } else {
        assert!(matches!(
            link,
            Err(mod_engine::sources::SourceError::WebsiteOnly { .. })
        ));
    }
}

#[tokio::test]
#[ignore]
async fn every_framework_has_a_github_release_asset() {
    let catalog = catalog();
    let github = GitHub::new(mod_engine::http_client());
    // Every hand-written profile (Cyberpunk, Phantom Blade Zero…).
    let frameworks = catalog.games().iter().flat_map(|g| g.frameworks.iter());
    for framework in frameworks {
        let release = github
            .release(&framework.github, framework.release.as_deref())
            .await
            .unwrap();
        assert!(
            release.asset_and_version(&framework.asset).is_some(),
            "{}: no {} in {:?}",
            framework.name,
            framework.asset,
            release.assets.iter().map(|a| &a.name).collect::<Vec<_>>()
        );
    }
}

#[tokio::test]
#[ignore]
async fn real_framework_archives_land_in_the_right_place() {
    use mod_engine::installer::{Installer, NexusFile, Paths, Target};
    use mod_engine::state::State;

    let catalog = catalog();
    let cyberpunk = catalog.by_steam_app_id(1091500).unwrap().clone();
    let tmp = tempfile::tempdir().unwrap();
    let game = tmp.path().join("Cyberpunk 2077");
    std::fs::create_dir_all(game.join("bin/x64")).unwrap();
    let client = mod_engine::http_client();
    let nexus = Nexus::new(client.clone());
    let github = GitHub::new(client.clone());
    let paths = Paths {
        store: tmp.path().join("store"),
        downloads: tmp.path().join("downloads"),
    };
    let mut state = State::open_in_memory().unwrap();
    let installer = Installer {
        source: &nexus,
        catalog: None,
        client: &client,
        paths: &paths,
        cancel: None,
    };
    let target = Target {
        game: "steam:1091500",
        game_dir: &game,
        community: COMMUNITY,
    };
    // Codeware: its `.reds` scripts need redscript, its `red4ext/plugins`
    // folder needs RED4ext.
    let done = installer
        .install_nexus(
            &mut state,
            &target,
            &cyberpunk,
            &github,
            7780,
            NexusFile::Main,
            None,
            |_| {},
        )
        .await
        .unwrap();
    let mut ids: Vec<_> = done.mods.iter().map(|m| m.id.as_str()).collect();
    ids.sort();
    assert_eq!(ids, ["nexus-1511", "nexus-2380", "nexus-7780"]);
    for file in [
        "red4ext/plugins/Codeware/Codeware.dll",
        "red4ext/RED4ext.dll",
        "bin/x64/winmm.dll",
        "engine/tools/scc.exe",
        "r6/config/cybercmd/scc.toml",
    ] {
        assert!(game.join(file).is_file(), "{file}");
    }
}

/// Source ladder, for real: Mod Settings (profile list) and Input Loader
/// (repo mentioned by its description) install from GitHub with a free
/// account, without a site link. Needs `NEXUS_API_KEY` (the hash is
/// checked against Nexus).
#[tokio::test]
#[ignore]
async fn verified_github_copies_spare_the_website() {
    use mod_engine::installer::{Installer, NexusFile, Paths, Target};
    use mod_engine::state::State;

    let Ok(key) = std::env::var("NEXUS_API_KEY") else {
        eprintln!("NEXUS_API_KEY not set: skipped");
        return;
    };
    let catalog = catalog();
    let cyberpunk = catalog.by_steam_app_id(1091500).unwrap().clone();
    let client = mod_engine::http_client();
    let nexus = Nexus::new(client.clone());
    nexus.set_api_key(Some(key));
    let github = GitHub::new(client.clone());
    for mod_id in [4885, 4575] {
        let tmp = tempfile::tempdir().unwrap();
        let game = tmp.path().join("Cyberpunk 2077");
        std::fs::create_dir_all(game.join("bin/x64")).unwrap();
        let paths = Paths {
            store: tmp.path().join("store"),
            downloads: tmp.path().join("downloads"),
        };
        let mut state = State::open_in_memory().unwrap();
        let installer = Installer {
            source: &nexus,
            catalog: None,
            client: &client,
            paths: &paths,
            cancel: None,
        };
        let target = Target {
            game: "steam:1091500",
            game_dir: &game,
            community: COMMUNITY,
        };
        let done = installer
            .install_nexus(
                &mut state,
                &target,
                &cyberpunk,
                &github,
                mod_id,
                NexusFile::Main,
                None,
                |_| {},
            )
            .await
            .unwrap_or_else(|e| panic!("{mod_id}: {e:?}"));
        assert!(
            done.mods
                .iter()
                .any(|m| m.id == format!("nexus-{mod_id}") && m.explicit)
        );
    }
}

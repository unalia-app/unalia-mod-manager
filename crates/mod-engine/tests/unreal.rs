//! Phantom Blade Zero (Unreal Engine 5) end to end, on its real Steam
//! layout: `.pak` mods and UE4SS mods from a fake Nexus, UE4SS (the build
//! made for the game) installed by itself from a fake GitHub, next to the
//! real executable `platform/steam/PBZ.exe`.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use mockito::{Matcher, ServerGuard};
use mod_engine::games::{GameCatalog, GameProfile};
use mod_engine::installer::{
    InstallError, Installer, NexusFile, NexusInstall, Paths, Target, available_updates_nexus,
};
use mod_engine::sources::github::GitHub;
use mod_engine::sources::nexus::{Nexus, NxmLink};
use mod_engine::state::State;
use serde_json::json;
use zip::write::SimpleFileOptions;

const GAME: &str = "steam:4115450";
const COMMUNITY: &str = "nexus:phantombladezero";
/// Project folder of the game, found on disk.
const PROJECT: &str = "ProjectPBZ";
/// Folder of the real executable, where UE4SS goes.
const BINARIES: &str = "platform/steam";
const UE4SS_PATH: &str = "/repos/KintaroModding/ue4ss/releases/latest";
/// UE4SS's id: its Nexus page for the game.
const UE4SS_ID: &str = "nexus-13";

struct World {
    _tmp: tempfile::TempDir,
    server: ServerGuard,
    game: PathBuf,
    paths: Paths,
    profile: GameProfile,
    nexus: Nexus,
    client: reqwest::Client,
    state: State,
}

impl World {
    async fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let game = tmp.path().join("Phantom Blade Zero");
        for dir in [
            "Engine/Binaries/Win64",
            "Engine/Content/Paks",
            "ProjectPBZ/Binaries/Win64",
            "ProjectPBZ/Content/Paks",
            "steam",
            "platform/steam",
        ] {
            fs::create_dir_all(game.join(dir)).unwrap();
        }
        // The launcher, then the game (sparse: its size, not its bytes).
        fs::write(game.join("steam/PBZ.exe"), b"launcher").unwrap();
        fs::File::create(game.join("platform/steam/PBZ.exe"))
            .unwrap()
            .set_len(500 * 1024 * 1024)
            .unwrap();
        fs::write(
            game.join("ProjectPBZ/Content/Paks/ProjectPBZ-Windows.pak"),
            b"game",
        )
        .unwrap();
        let schema = fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/thunderstore/schema.json"),
        )
        .unwrap();
        let catalog = GameCatalog::from_schema_json(&schema).unwrap();
        let server = mockito::Server::new_async().await;
        let client = reqwest::Client::new();
        let nexus = Nexus::with_urls(
            client.clone(),
            &format!("{}/graphql", server.url()),
            &format!("{}/v1", server.url()),
        );
        nexus.set_api_key(Some("test-key".into()));
        let mut world = Self {
            nexus,
            paths: Paths {
                store: tmp.path().join("store"),
                downloads: tmp.path().join("downloads"),
            },
            profile: catalog.by_steam_app_id(4115450).unwrap().clone(),
            state: State::open(&tmp.path().join("modalia.db")).unwrap(),
            client,
            server,
            game,
            _tmp: tmp,
        };
        world.publish_ue4ss("3.0.1-1183-gcdbc0b66").await;
        world
    }

    /// The UE4SS build made for the game, with the layout of its real
    /// archive, next to the builds for other games.
    async fn publish_ue4ss(&mut self, build: &str) {
        let asset = format!("phantombladezero_ue4ss_v{build}.zip");
        let download = format!("/assets/{asset}");
        self.server
            .mock("GET", UE4SS_PATH)
            .with_body(
                json!({
                    "tag_name": format!("v{build}"),
                    "assets": [
                        { "name": format!("UE4SS_v{build}.zip"), "browser_download_url": "https://example.invalid/stock", "size": 1 },
                        { "name": format!("expedition33_ue4ss_v{build}.zip"), "browser_download_url": "https://example.invalid/e33", "size": 1 },
                        { "name": asset, "browser_download_url": format!("{}{download}", self.server.url()), "size": 1 }
                    ]
                })
                .to_string(),
            )
            .create_async()
            .await;
        self.server
            .mock("GET", download.as_str())
            .with_body(zip_bytes(&[
                ("dwmapi.dll", b"proxy"),
                ("ue4ss/UE4SS.dll", b"ue4ss"),
                ("ue4ss/UE4SS-settings.ini", b"[General]"),
                ("ue4ss/MemberVariableLayout.ini", b"[UObjectBase]"),
                ("ue4ss/UE4SS_Signatures/GUObjectArray.lua", b"-- signature"),
                ("ue4ss/Mods/mods.txt", b"BPModLoaderMod : 1"),
            ]))
            .create_async()
            .await;
    }

    async fn publish(&mut self, mod_id: u32, name: &str, files: &[(&str, &[u8])]) {
        let file_id = mod_id * 10;
        let archive = zip_bytes(files);
        let graphql = |body: Vec<Matcher>, data: serde_json::Value| (body, data);
        for (body, data) in [
            graphql(
                vec![
                    Matcher::Regex("modRequirements".into()),
                    Matcher::Regex(format!(r#""modId":{mod_id}\b"#)),
                ],
                json!({ "legacyModsByDomain": { "nodes": [{
                    "modId": mod_id, "name": name, "author": "Someone", "version": "1.0",
                    "gameId": 10614, "status": "published", "description": "",
                    "modRequirements": { "nexusRequirements": { "nodes": [] } }
                }] } }),
            ),
            graphql(
                vec![
                    Matcher::Regex("modFiles".into()),
                    Matcher::Regex(format!(r#""mod":"{mod_id}""#)),
                ],
                json!({ "modFiles": [{ "fileId": file_id, "name": name, "version": "1.0",
                    "category": "MAIN", "primary": 1,
                    "sizeInBytes": archive.len().to_string(), "date": 1_793_000_000 }] }),
            ),
        ] {
            self.server
                .mock("POST", "/graphql")
                .match_body(Matcher::AllOf(body))
                .with_body(json!({ "data": data }).to_string())
                .create_async()
                .await;
        }
        let download = format!("/cdn/{mod_id}.zip");
        self.server
            .mock(
                "GET",
                format!(
                    "/v1/games/phantombladezero/mods/{mod_id}/files/{file_id}/download_link.json"
                )
                .as_str(),
            )
            .match_query(Matcher::Any)
            .with_body(
                json!([{ "name": "CDN", "short_name": "CDN",
                         "URI": format!("{}{download}", self.server.url()) }])
                .to_string(),
            )
            .create_async()
            .await;
        self.server
            .mock("GET", download.as_str())
            .with_body(archive)
            .create_async()
            .await;
    }

    async fn install(&mut self, mod_id: u32) -> Result<NexusInstall, InstallError> {
        self.install_variant(mod_id, None).await
    }

    async fn install_variant(&mut self, mod_id: u32, variant: Option<&str>) -> Result<NexusInstall, InstallError> {
        let github = GitHub::with_url(self.client.clone(), &self.server.url());
        self.install_with(mod_id, &github, variant).await
    }

    async fn install_with(
        &mut self,
        mod_id: u32,
        github: &GitHub,
        variant: Option<&str>,
    ) -> Result<NexusInstall, InstallError> {
        let link = NxmLink::parse(&format!(
            "nxm://phantombladezero/mods/{mod_id}/files/{}?key=k&expires=1",
            mod_id * 10
        ))
        .unwrap();
        let installer = Installer {
            source: &self.nexus,
            catalog: None,
            client: &self.client,
            paths: &self.paths,
            cancel: None,
        };
        let target = Target {
            game: GAME,
            game_dir: &self.game,
            community: COMMUNITY,
        };
        installer
            .install_nexus(
                &mut self.state,
                &target,
                &self.profile,
                github,
                mod_id,
                NexusFile::Link(&link),
                None,
                variant,
                |_| {},
            )
            .await
    }
}

fn zip_bytes(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, data) in files {
        w.start_file(*name, SimpleFileOptions::default()).unwrap();
        w.write_all(data).unwrap();
    }
    w.finish().unwrap().into_inner()
}

#[tokio::test]
async fn a_lua_mod_brings_ue4ss_next_to_the_real_executable() {
    let mut w = World::new().await;
    w.publish(
        8000,
        "No Stamina",
        &[
            ("NoStamina/Scripts/main.lua", b"print('hi')"),
            ("NoStamina/enabled.txt", b""),
            ("README.md", b"readme"),
        ],
    )
    .await;

    let done = w.install(8000).await.unwrap();

    let binaries = w.game.join(BINARIES);
    assert!(binaries.join("dwmapi.dll").is_file());
    assert!(binaries.join("ue4ss/UE4SS.dll").is_file());
    assert!(binaries.join("ue4ss/MemberVariableLayout.ini").is_file());
    assert!(
        binaries
            .join("ue4ss/Mods/NoStamina/Scripts/main.lua")
            .is_file()
    );
    // Not next to the launcher, not in the project.
    assert!(!w.game.join("steam/dwmapi.dll").exists());
    assert!(!w.game.join(PROJECT).join("Binaries/Win64/dwmapi.dll").exists());
    assert!(!w.game.join("README.md").exists());

    let mut ids: Vec<_> = done
        .mods
        .iter()
        .map(|m| (m.id.as_str(), m.version.as_str(), m.explicit))
        .collect();
    ids.sort();
    assert_eq!(
        ids,
        [
            ("nexus-13", "3.0.1-1183-gcdbc0b66", false),
            ("nexus-8000", "1.0", true),
        ]
    );
}

/// The real UE4SS build for the game, from GitHub: its layout is still the
/// one the install expects. Outside the normal suite (network):
/// `cargo test -p mod-engine --test unreal -- --ignored`.
#[tokio::test]
#[ignore]
async fn the_real_ue4ss_build_lands_in_the_right_place() {
    let mut w = World::new().await;
    w.publish(8000, "No Stamina", &[("NoStamina/Scripts/main.lua", b"print('hi')")])
        .await;

    let github = GitHub::new(mod_engine::http_client());
    let done = w.install_with(8000, &github, None).await.unwrap();

    let binaries = w.game.join(BINARIES);
    for file in [
        "dwmapi.dll",
        "ue4ss/UE4SS.dll",
        "ue4ss/UE4SS-settings.ini",
        "ue4ss/MemberVariableLayout.ini",
        "ue4ss/Mods/mods.txt",
        "ue4ss/Mods/NoStamina/Scripts/main.lua",
    ] {
        assert!(binaries.join(file).is_file(), "{file}");
    }
    let ue4ss = done.mods.iter().find(|m| m.id == UE4SS_ID).unwrap();
    assert!(ue4ss.version.starts_with("3."), "{}", ue4ss.version);
}

#[tokio::test]
async fn a_pak_mod_goes_to_tilde_mods_without_ue4ss() {
    let mut w = World::new().await;
    w.publish(
        8001,
        "Better Swords",
        &[
            ("Better Swords/BetterSwords_P.pak", b"pak"),
            ("Better Swords/BetterSwords_P.utoc", b"utoc"),
            ("Better Swords/BetterSwords_P.ucas", b"ucas"),
        ],
    )
    .await;

    let done = w.install(8001).await.unwrap();

    let mods = w.game.join(PROJECT).join("Content/Paks/~mods");
    for file in [
        "BetterSwords_P.pak",
        "BetterSwords_P.utoc",
        "BetterSwords_P.ucas",
    ] {
        assert!(mods.join(file).is_file(), "{file}");
    }
    assert!(!w.game.join(BINARIES).join("dwmapi.dll").exists());
    let ids: Vec<_> = done.mods.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, ["nexus-8001"]);
}

/// Skip Intro 1.1.0: one folder per variant, the same `.pak` in each.
#[tokio::test]
async fn an_archive_with_variants_asks_which_one_then_keeps_it() {
    let mut w = World::new().await;
    let pak = "ProjectPBZ/Content/Paks/~mods/zzz_PBZSkipIntro_P.pak";
    w.publish(
        8002,
        "Skip Intro",
        &[
            ("README.txt", b"readme"),
            (&format!("Skip Intro/{pak}"), b"plain"),
            (&format!("Skip Intro + Instant Menu/{pak}"), b"instant"),
        ],
    )
    .await;

    let err = w.install(8002).await.unwrap_err();
    let InstallError::ChooseVariant(names) = err else {
        panic!("{err:?}");
    };
    assert_eq!(names, ["Skip Intro", "Skip Intro + Instant Menu"]);
    // Nothing placed while the player chooses.
    assert!(!w.game.join(pak).exists());

    w.install_variant(8002, Some("Skip Intro + Instant Menu"))
        .await
        .unwrap();
    assert_eq!(fs::read(w.game.join(pak)).unwrap(), b"instant");

    // Installed again (an update): the same variant, without asking.
    w.install(8002).await.unwrap();
    assert_eq!(fs::read(w.game.join(pak)).unwrap(), b"instant");
}

#[tokio::test]
async fn a_new_ue4ss_build_is_an_update() {
    let mut w = World::new().await;
    w.publish(
        8000,
        "No Stamina",
        &[("NoStamina/Scripts/main.lua", b"lua")],
    )
    .await;
    w.install(8000).await.unwrap();

    // Same number, new build: it is an update.
    let mut newer = mockito::Server::new_async().await;
    newer
        .mock("GET", UE4SS_PATH)
        .with_body(
            json!({ "tag_name": "v3.0.1-1200-gabcdef12", "assets": [
                { "name": "phantombladezero_ue4ss_v3.0.1-1200-gabcdef12.zip",
                  "browser_download_url": "http://example.test/u.zip", "size": 1 }
            ] })
            .to_string(),
        )
        .create_async()
        .await;
    let github = GitHub::with_url(w.client.clone(), &newer.url());
    let installed = w.state.list(GAME).unwrap();
    let updates = available_updates_nexus(&w.nexus, &github, &w.profile, &installed, None).await;
    let ue4ss = updates
        .iter()
        .find(|u| u.mod_id == UE4SS_ID)
        .unwrap();
    assert_eq!(ue4ss.latest, "3.0.1-1200-gabcdef12");
}

#[tokio::test]
async fn a_game_without_its_unreal_folders_is_refused_clearly() {
    let mut w = World::new().await;
    fs::remove_dir_all(w.game.join(PROJECT)).unwrap();
    w.publish(8001, "Better Swords", &[("BetterSwords_P.pak", b"pak")])
        .await;
    let err = w.install(8001).await.unwrap_err();
    assert!(matches!(err, InstallError::NoGameFiles), "{err:?}");
}

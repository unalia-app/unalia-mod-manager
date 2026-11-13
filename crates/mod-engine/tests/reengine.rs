//! RE Engine end to end, on a fake Monster Hunter Wilds recognized from
//! its `re_chunk_000.pak` and its name on Nexus: loose files, scripts and
//! `.pak` mods from a fake Nexus, REFramework installed by itself from a
//! fake GitHub, and its loose file loader turned on while a mod needs it.

use std::fs;
use std::io::Write;
use std::path::PathBuf;

use mockito::{Matcher, ServerGuard};
use mod_engine::games::GameProfile;
use mod_engine::games::auto;
use mod_engine::installer::{self, InstallError, Installer, NexusFile, NexusInstall, Paths, Target};
use mod_engine::loaders::reengine::CONFIG;
use mod_engine::sources::github::GitHub;
use mod_engine::sources::nexus::{FoundGame, Nexus, NxmLink};
use mod_engine::state::State;
use serde_json::json;
use zip::write::SimpleFileOptions;

const GAME: &str = "steam:2246340";
const REFRAMEWORK_PATH: &str = "/repos/praydog/REFramework-nightly/releases/latest";

struct World {
    _tmp: tempfile::TempDir,
    server: ServerGuard,
    game: PathBuf,
    paths: Paths,
    profile: GameProfile,
    domain: &'static str,
    nexus: Nexus,
    client: reqwest::Client,
    state: State,
}

impl World {
    /// Monster Hunter Wilds, where REFramework loads `.pak` mods.
    async fn wilds() -> Self {
        Self::new("Monster Hunter Wilds", "monsterhunterwilds", 6993, "MonsterHunterWilds.exe").await
    }

    async fn new(name: &str, domain: &'static str, game_id: u32, exe: &str) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let game = tmp.path().join(name);
        fs::create_dir_all(&game).unwrap();
        for file in [exe, "re_chunk_000.pak", "re_chunk_000.pak.patch_001.pak"] {
            fs::write(game.join(file), b"game").unwrap();
        }
        let found = FoundGame {
            name: name.into(),
            domain: domain.into(),
            game_id,
        };
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
            profile: auto::profile(&found, &game).unwrap(),
            domain,
            state: State::open(&tmp.path().join("modalia.db")).unwrap(),
            client,
            server,
            game,
            _tmp: tmp,
        };
        world.publish_reframework().await;
        world
    }

    /// REFramework as its author publishes it: one `dinput8.dll` for every
    /// game, in `REFramework.zip`, next to the VR and C# builds.
    async fn publish_reframework(&mut self) {
        let download = "/assets/REFramework.zip";
        self.server
            .mock("GET", REFRAMEWORK_PATH)
            .with_body(
                json!({
                    "tag_name": "nightly-01439-c38d1fe",
                    "assets": [
                        { "name": "csharp-api.zip", "browser_download_url": "https://example.invalid/csharp", "size": 1 },
                        { "name": "REFramework.zip", "browser_download_url": format!("{}{download}", self.server.url()), "size": 1 },
                        { "name": "VR.zip", "browser_download_url": "https://example.invalid/vr", "size": 1 }
                    ]
                })
                .to_string(),
            )
            .create_async()
            .await;
        self.server
            .mock("GET", download)
            .with_body(zip_bytes(&[
                ("dinput8.dll", b"reframework"),
                ("reframework_revision.txt", b"c38d1fe"),
            ]))
            .create_async()
            .await;
    }

    async fn publish(&mut self, mod_id: u32, name: &str, requires: &[u32], files: &[(&str, &[u8])]) {
        let file_id = mod_id * 10;
        let archive = zip_bytes(files);
        let requirements: Vec<_> = requires
            .iter()
            .map(|id| {
                json!({ "modId": id.to_string(), "modName": "REFramework", "gameId": "6993",
                        "url": "", "notes": "", "externalRequirement": false })
            })
            .collect();
        for (body, data) in [
            (
                vec![
                    Matcher::Regex("modRequirements".into()),
                    Matcher::Regex(format!(r#""modId":{mod_id}\b"#)),
                ],
                json!({ "legacyModsByDomain": { "nodes": [{
                    "modId": mod_id, "name": name, "author": "Someone", "version": "1.0",
                    "gameId": 6993, "status": "published", "description": "",
                    "modRequirements": { "nexusRequirements": { "nodes": requirements } }
                }] } }),
            ),
            (
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
                format!("/v1/games/{}/mods/{mod_id}/files/{file_id}/download_link.json", self.domain).as_str(),
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
        let github = GitHub::with_url(self.client.clone(), &self.server.url());
        let link = NxmLink::parse(&format!(
            "nxm://{}/mods/{mod_id}/files/{}?key=k&expires=1",
            self.domain,
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
        let community = format!("nexus:{}", self.domain);
        let target = Target {
            game: GAME,
            game_dir: &self.game,
            community: &community,
        };
        let result = installer
            .install_nexus(
                &mut self.state,
                &target,
                &self.profile,
                &github,
                mod_id,
                NexusFile::Link(&link),
                None,
                None,
                |_| {},
            )
            .await;
        // As the app does after each change.
        installer::sync_loose_files(&mut self.state, GAME, &self.game).unwrap();
        result
    }

    fn uninstall(&mut self, mod_id: u32) {
        installer::uninstall(&mut self.state, &self.paths, GAME, &self.game, &format!("nexus-{mod_id}")).unwrap();
        installer::sync_loose_files(&mut self.state, GAME, &self.game).unwrap();
    }

    fn config(&self) -> Option<String> {
        fs::read_to_string(self.game.join(CONFIG)).ok()
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
async fn a_loose_file_mod_brings_reframework_and_its_loose_file_loader() {
    let mut w = World::wilds().await;
    w.publish(
        1001,
        "Ver.R Body",
        &[],
        &[
            ("Ver.R Body/modinfo.ini", b"name=Ver.R Body"),
            ("Ver.R Body/natives/STM/Art/body.mesh.2109148288", b"mesh"),
            ("Ver.R Body/screenshot.png", b"png"),
        ],
    )
    .await;

    let done = w.install(1001).await.unwrap();

    assert_eq!(fs::read(w.game.join("dinput8.dll")).unwrap(), b"reframework");
    assert!(w.game.join("natives/STM/Art/body.mesh.2109148288").is_file());
    assert!(!w.game.join("modinfo.ini").exists() && !w.game.join("reframework_revision.txt").exists());
    let mut ids: Vec<_> = done.mods.iter().map(|m| (m.id.as_str(), m.version.as_str(), m.explicit)).collect();
    ids.sort();
    // REFramework under its Nexus page in this game.
    assert_eq!(ids, [("nexus-1001", "1.0", true), ("nexus-93", "nightly-01439-c38d1fe", false)]);
    assert_eq!(w.config().as_deref(), Some("LooseFileLoader_Enabled=true\n"));

    // REFramework writes all its settings at the first launch: only our
    // key goes when no mod needs it, as it was before.
    fs::write(
        w.game.join(CONFIG),
        "REFrameworkConfig_MenuKey=45\nLooseFileLoader_Enabled=true\n",
    )
    .unwrap();
    w.uninstall(1001);
    assert!(!w.game.join("natives").exists());
    assert_eq!(w.config().as_deref(), Some("REFrameworkConfig_MenuKey=45\n"));

    // Turned off by the player while a mod needs it: their choice stays.
    w.install(1001).await.unwrap();
    assert_eq!(w.config().as_deref(), Some("REFrameworkConfig_MenuKey=45\nLooseFileLoader_Enabled=true\n"));
    fs::write(w.game.join(CONFIG), "LooseFileLoader_Enabled=false\n").unwrap();
    assert!(!installer::sync_loose_files(&mut w.state, GAME, &w.game).unwrap());
    w.uninstall(1001);
    assert_eq!(w.config().as_deref(), Some("LooseFileLoader_Enabled=false\n"));
}

#[tokio::test]
async fn scripts_and_paks_each_go_where_reframework_loads_them() {
    let mut w = World::wilds().await;
    w.publish(
        1002,
        "Auto Collect",
        &[93],
        &[("Auto Collect/autorun/AutoCollect.lua", b"-- collect"), ("Auto Collect/README.txt", b"readme")],
    )
    .await;
    w.publish(
        1003,
        "Lala Barina Ver.R",
        &[93],
        &[("Lala Barina/re_chunk_000.pak.sub_000.pak.patch_005.pak", b"pak")],
    )
    .await;

    let done = w.install(1002).await.unwrap();
    // Declared as a requirement, REFramework comes from GitHub: nothing
    // left to get from Nexus.
    assert!(done.missing.is_empty());
    assert!(w.game.join("reframework/autorun/AutoCollect.lua").is_file());
    w.install(1003).await.unwrap();
    assert!(
        w.game
            .join("pak_mods/Lala Barina Ver.R/re_chunk_000.pak.sub_000.pak.patch_005.pak")
            .is_file()
    );
    // Neither needs the loose file loader.
    assert_eq!(w.config(), None);

    w.uninstall(1003);
    w.uninstall(1002);
    assert!(!w.game.join("pak_mods").exists());
    assert!(!w.game.join("reframework/autorun/AutoCollect.lua").exists());
    // The game's own files are untouched.
    assert_eq!(fs::read(w.game.join("re_chunk_000.pak.patch_001.pak")).unwrap(), b"game");
}

#[tokio::test]
async fn a_pak_where_reframework_cant_load_it_is_refused_with_a_reason() {
    let mut w = World::new("Resident Evil 4", "residentevil42023", 5195, "re4.exe").await;
    w.publish(2001, "Leon Outfit", &[], &[("re_chunk_000.pak.patch_004.pak", b"pak")]).await;

    let error = w.install(2001).await.unwrap_err();
    assert!(matches!(error, InstallError::PatchPak(ref name) if name == "Leon Outfit"));
    // Nothing placed, not even REFramework.
    assert!(!w.game.join("dinput8.dll").exists());
    assert!(!w.game.join("pak_mods").exists());
}

/// The real REFramework from GitHub: its layout is still the one the
/// install expects. Outside the normal suite (network):
/// `cargo test -p mod-engine --test reengine -- --ignored`.
#[tokio::test]
#[ignore]
async fn the_real_reframework_lands_at_the_game_root() {
    let mut w = World::wilds().await;
    w.publish(1001, "Ver.R Body", &[], &[("natives/STM/Art/body.mesh.2109148288", b"mesh")])
        .await;
    let github = GitHub::new(mod_engine::http_client());
    let link = NxmLink::parse("nxm://monsterhunterwilds/mods/1001/files/10010?key=k&expires=1").unwrap();
    let installer = Installer {
        source: &w.nexus,
        catalog: None,
        client: &w.client,
        paths: &w.paths,
        cancel: None,
    };
    let target = Target {
        game: GAME,
        game_dir: &w.game,
        community: "nexus:monsterhunterwilds",
    };
    let done = installer
        .install_nexus(&mut w.state, &target, &w.profile, &github, 1001, NexusFile::Link(&link), None, None, |_| {})
        .await
        .unwrap();

    assert!(fs::metadata(w.game.join("dinput8.dll")).unwrap().len() > 1_000_000);
    let reframework = done.mods.iter().find(|m| m.id == "nexus-93").unwrap();
    assert!(reframework.version.starts_with("nightly-"), "{}", reframework.version);
    assert_eq!(reframework.deployment.files, ["dinput8.dll"]);
}

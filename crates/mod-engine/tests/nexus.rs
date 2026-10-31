//! Cyberpunk 2077 end to end: a Nexus mod (`nxm://` link), its frameworks
//! installed by themselves from GitHub, conflicts, against a fake Nexus and
//! a fake GitHub that serve real archives.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use mockito::{Matcher, ServerGuard};
use mod_engine::games::{GameCatalog, GameProfile};
use mod_engine::installer;
use mod_engine::installer::{
    InstallError, Installer, NexusFile, NexusInstall, NotAMod, Paths, Target, available_updates_nexus,
};
use mod_engine::sources::github::GitHub;
use mod_engine::sources::nexus::{Nexus, NxmLink};
use mod_engine::state::State;
use serde_json::json;
use zip::write::SimpleFileOptions;

/// Files of an archive: path and content.
type Files<'a> = Vec<(&'a str, &'a [u8])>;

const GAME: &str = "steam:1091500";
const COMMUNITY: &str = "nexus:cyberpunk2077";
const COOL_HUD: u32 = 5000;
const MOD_SETTINGS: u32 = 4885;

struct World {
    _tmp: tempfile::TempDir,
    server: ServerGuard,
    game: PathBuf,
    paths: Paths,
    profile: GameProfile,
    nexus: Nexus,
    github: GitHub,
    client: reqwest::Client,
    state: State,
    /// Description of the next published mods (GitHub repos are mentioned there).
    description: String,
    /// Game that stayed on an old version: mods published before this date.
    before: Option<String>,
}

impl World {
    async fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let game = tmp.path().join("Cyberpunk 2077");
        fs::create_dir_all(game.join("bin/x64")).unwrap();
        fs::write(game.join("bin/x64/Cyberpunk2077.exe"), b"exe").unwrap();
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
            github: GitHub::with_url(client.clone(), &server.url()),
            nexus,
            paths: Paths {
                store: tmp.path().join("store"),
                downloads: tmp.path().join("downloads"),
            },
            profile: catalog.by_steam_app_id(1091500).unwrap().clone(),
            state: State::open(&tmp.path().join("modalia.db")).unwrap(),
            client,
            server,
            game,
            description: String::new(),
            before: None,
            _tmp: tmp,
        };
        world.publish_frameworks().await;
        world
    }

    /// The frameworks on the fake GitHub, with the layout of their real
    /// archives.
    async fn publish_frameworks(&mut self) {
        let frameworks: [(&str, &str, &str, Files); 4] = [
            (
                "wopss/RED4ext",
                "v1.30.0",
                "red4ext-1.30.0.zip",
                vec![
                    ("bin/x64/winmm.dll", b"winmm"),
                    ("red4ext/RED4ext.dll", b"r4e"),
                ],
            ),
            (
                "jac3km4/redscript",
                "v0.5.31",
                "redscript-v0.5.31-windows.zip",
                vec![
                    ("engine/config/base/scripts.ini", b"ini"),
                    ("engine/tools/scc.exe", b"scc"),
                ],
            ),
            (
                "psiberx/cp2077-archive-xl",
                "v1.27.5",
                "ArchiveXL-1.27.5.zip",
                vec![
                    ("red4ext/plugins/ArchiveXL/ArchiveXL.dll", b"axl"),
                    ("red4ext/plugins/ArchiveXL/Bundle/Migration.xl", b"xl"),
                ],
            ),
            (
                "psiberx/cp2077-codeware",
                "v1.20.6",
                "Codeware-1.20.6.zip",
                vec![
                    ("red4ext/plugins/Codeware/Codeware.dll", b"cw"),
                    ("red4ext/plugins/Codeware/Scripts/Codeware.reds", b"reds"),
                ],
            ),
        ];
        for (repo, tag, asset, files) in frameworks {
            let download = format!("/assets/{asset}");
            self.server
                .mock("GET", format!("/repos/{repo}/releases/latest").as_str())
                .with_body(
                    json!({
                        "tag_name": tag,
                        "published_at": "2026-10-19T17:00:00Z",
                        "assets": [
                            { "name": format!("{asset}-pdb.zip"), "browser_download_url": "https://example.invalid/pdb", "size": 1 },
                            { "name": asset, "browser_download_url": format!("{}{download}", self.server.url()), "size": 1 }
                        ]
                    })
                    .to_string(),
                )
                .create_async()
                .await;
            self.server
                .mock("GET", download.as_str())
                .with_body(zip_bytes(&files))
                .create_async()
                .await;
        }
    }

    /// A mod on the fake Nexus: page, files, requirements, archive.
    async fn publish(
        &mut self,
        mod_id: u32,
        name: &str,
        version: &str,
        requirements: &[(u32, &str)],
        files: &[(&str, &[u8])],
    ) {
        self.publish_with(mod_id, name, version, requirements, files, None).await;
    }

    /// With an optional file next to the main one: its id, name, version
    /// and content.
    async fn publish_with(
        &mut self,
        mod_id: u32,
        name: &str,
        version: &str,
        requirements: &[(u32, &str)],
        files: &[(&str, &[u8])],
        optional: Option<(u32, &str, &str, Files<'_>)>,
    ) {
        let file_id = mod_id * 10;
        let archive = zip_bytes(files);
        let requirements: Vec<_> = requirements
            .iter()
            .map(|(id, name)| {
                // `Name|note`: what the author says about it.
                let (name, notes) = name.split_once('|').unwrap_or((name, ""));
                json!({ "modId": id.to_string(), "modName": name, "gameId": "3333",
                        "externalRequirement": false, "url": "", "notes": notes })
            })
            .collect();
        self.graphql(
            vec![
                Matcher::Regex("modRequirements".into()),
                mod_id_matcher(mod_id),
            ],
            json!({ "legacyModsByDomain": { "nodes": [{
                "modId": mod_id, "name": name, "summary": "", "author": "Someone",
                "thumbnailUrl": null, "pictureUrl": null, "downloads": 1,
                "updatedAt": "2026-10-01T00:00:00Z", "createdAt": "2026-01-01T00:00:00Z",
                "version": version, "adultContent": false, "status": "published",
                "description": self.description,
                "modCategory": { "name": "Gameplay" }, "gameId": 3333,
                "modRequirements": { "nexusRequirements": { "nodes": requirements } }
            }] } }),
        )
        .await;
        let mut listed = vec![
            json!({ "fileId": file_id - 1, "name": name, "version": "0.9", "category": "OLD_VERSION",
                    "primary": 0, "sizeInBytes": "10", "date": 1_700_000_000 }),
            json!({ "fileId": file_id, "name": name, "version": version, "category": "MAIN",
                    "primary": 1, "sizeInBytes": archive.len().to_string(), "date": 1_790_000_000 }),
        ];
        if let Some((optional_id, optional_name, optional_version, content)) = optional {
            let bytes = zip_bytes(&content);
            listed.push(json!({ "fileId": optional_id, "name": optional_name, "version": optional_version,
                                "category": "OPTIONAL", "primary": 0,
                                "sizeInBytes": bytes.len().to_string(), "date": 1_790_000_100 }));
            self.serve(mod_id, optional_id, bytes).await;
        }
        self.graphql(
            vec![
                Matcher::Regex("modFiles".into()),
                Matcher::Regex(format!(r#""mod":"{mod_id}""#)),
            ],
            json!({ "modFiles": listed }),
        )
        .await;
        self.serve(mod_id, file_id, archive).await;
    }

    /// A Nexus file's download: only with the key of an nxm:// link.
    async fn serve(&mut self, mod_id: u32, file_id: u32, archive: Vec<u8>) {
        let download = format!("/cdn/{file_id}.zip");
        // Free account: the link is only given with the key of the nxm:// link.
        self.server
            .mock(
                "GET",
                format!("/v1/games/cyberpunk2077/mods/{mod_id}/files/{file_id}/download_link.json")
                    .as_str(),
            )
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("key".into(), "site-key".into()),
                Matcher::UrlEncoded("expires".into(), "1792500000".into()),
            ]))
            .with_body(
                json!([{ "name": "Nexus CDN", "short_name": "CDN",
                         "URI": format!("{}{download}", self.server.url()) }])
                .to_string(),
            )
            .create_async()
            .await;
        self.server
            .mock(
                "GET",
                format!("/v1/games/cyberpunk2077/mods/{mod_id}/files/{file_id}/download_link.json")
                    .as_str(),
            )
            .match_query(Matcher::Missing)
            .with_status(403)
            .with_body(r#"{"code":403,"message":"premium users only"}"#)
            .create_async()
            .await;
        self.server
            .mock("GET", download.as_str())
            .with_body(archive)
            .create_async()
            .await;
    }

    async fn graphql(&mut self, body: Vec<Matcher>, data: serde_json::Value) {
        self.server
            .mock("POST", "/graphql")
            .match_body(Matcher::AllOf(body))
            .with_body(json!({ "data": data }).to_string())
            .create_async()
            .await;
    }

    async fn install(
        &mut self,
        mod_id: u32,
        file: NexusFile<'_>,
    ) -> Result<NexusInstall, InstallError> {
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
                &self.github,
                mod_id,
                file,
                self.before.as_deref(),
                None,
                |_| {},
            )
            .await
    }
}

fn mod_id_matcher(mod_id: u32) -> Matcher {
    Matcher::Regex(format!(r#""modId":{mod_id}\b"#))
}

fn link(mod_id: u32) -> NxmLink {
    NxmLink::parse(&format!(
        "nxm://cyberpunk2077/mods/{mod_id}/files/{}?key=site-key&expires=1792500000&user_id=1",
        mod_id * 10
    ))
    .unwrap()
}

fn zip_bytes(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, data) in files {
        w.start_file(*name, SimpleFileOptions::default()).unwrap();
        w.write_all(data).unwrap();
    }
    w.finish().unwrap().into_inner()
}

fn snapshot(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let rel = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.insert(rel, fs::read(&path).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

async fn publish_cool_hud(w: &mut World) {
    w.publish(
        COOL_HUD,
        "Cool HUD",
        "1.2.0",
        &[(7780, "Codeware"), (MOD_SETTINGS, "Mod Settings")],
        &[
            ("Cool HUD v1.2/r6/scripts/CoolHud/hud.reds", b"hud"),
            ("Cool HUD v1.2/archive/pc/mod/coolhud.archive", b"archive"),
            ("Cool HUD v1.2/archive/pc/mod/coolhud.archive.xl", b"xl"),
            ("Cool HUD v1.2/README.txt", b"readme"),
        ],
    )
    .await;
}

// ---------------------------------------------------------------- tests

#[tokio::test]
async fn a_nexus_mod_brings_its_frameworks_from_github() {
    let mut w = World::new().await;
    publish_cool_hud(&mut w).await;
    let link = link(COOL_HUD);

    let done = w.install(COOL_HUD, NexusFile::Link(&link)).await.unwrap();

    let game = &w.game;
    // The mod, stored despite its wrapper folder, without its readme.
    assert!(game.join("r6/scripts/CoolHud/hud.reds").is_file());
    assert!(game.join("archive/pc/mod/coolhud.archive.xl").is_file());
    assert!(!game.join("README.txt").exists());
    // redscript (.reds), ArchiveXL (.xl), Codeware (Nexus requirement), and
    // RED4ext, which ArchiveXL and Codeware need in turn.
    assert!(game.join("engine/tools/scc.exe").is_file());
    assert!(
        game.join("red4ext/plugins/ArchiveXL/ArchiveXL.dll")
            .is_file()
    );
    assert!(game.join("red4ext/plugins/Codeware/Codeware.dll").is_file());
    assert!(game.join("red4ext/RED4ext.dll").is_file());

    let mut ids: Vec<_> = done
        .mods
        .iter()
        .map(|m| (m.id.as_str(), m.explicit))
        .collect();
    ids.sort();
    assert_eq!(
        ids,
        [
            ("nexus-1511", false),
            ("nexus-2380", false),
            ("nexus-4198", false),
            ("nexus-5000", true),
            ("nexus-7780", false),
        ]
    );
    let hud = done.mods.iter().find(|m| m.id == "nexus-5000").unwrap();
    assert_eq!(
        (hud.version.as_str(), hud.author.as_str()),
        ("1.2.0", "Someone")
    );
    let red4ext = done.mods.iter().find(|m| m.id == "nexus-2380").unwrap();
    assert_eq!(red4ext.version, "1.30.0");

    // Mod Settings is not a framework: the user has to go through the site
    // (free account).
    let missing: Vec<_> = done.missing.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(missing, ["Mod Settings"]);
}

#[tokio::test]
async fn frameworks_installed_by_hand_are_not_installed_again() {
    let mut w = World::new().await;
    publish_cool_hud(&mut w).await;
    for marker in [
        "red4ext/RED4ext.dll",
        "engine/tools/scc.exe",
        "red4ext/plugins/ArchiveXL/ArchiveXL.dll",
        "red4ext/plugins/Codeware/Codeware.dll",
    ] {
        let path = w.game.join(marker);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"manual").unwrap();
    }

    let done = w
        .install(COOL_HUD, NexusFile::Link(&link(COOL_HUD)))
        .await
        .unwrap();
    let ids: Vec<_> = done.mods.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, ["nexus-5000"]);
}

#[tokio::test]
async fn a_free_account_is_sent_to_the_website() {
    let mut w = World::new().await;
    publish_cool_hud(&mut w).await;
    let before = snapshot(&w.game);

    let err = w.install(COOL_HUD, NexusFile::Main).await.unwrap_err();
    assert!(
        matches!(err, InstallError::NexusWebsite { file_id: 50000 }),
        "{err:?}"
    );
    assert_eq!(snapshot(&w.game), before);
    assert!(w.state.list(GAME).unwrap().is_empty());
}

#[tokio::test]
async fn frameworks_install_from_github_without_the_website() {
    let mut w = World::new().await;
    // ArchiveXL requested directly: no need for Nexus or Premium.
    let done = w.install(4198, NexusFile::Main).await.unwrap();
    let mut ids: Vec<_> = done
        .mods
        .iter()
        .map(|m| (m.id.as_str(), m.explicit))
        .collect();
    ids.sort();
    assert_eq!(ids, [("nexus-2380", false), ("nexus-4198", true)]);
}

const HUD: &str = "r6/scripts/CoolHud/hud.reds";
const REMIX: &str = "nexus-6000";

/// Cool HUD, then a patch that replaces one of its files.
async fn hud_and_its_patch() -> World {
    let mut w = World::new().await;
    publish_cool_hud(&mut w).await;
    w.install(COOL_HUD, NexusFile::Link(&link(COOL_HUD)))
        .await
        .unwrap();
    w.publish(6000, "HUD Remix", "2.0", &[], &[(HUD, b"remix")]).await;
    w.install(6000, NexusFile::Link(&link(6000))).await.unwrap();
    w
}

fn hud_file(w: &World) -> Vec<u8> {
    fs::read(w.game.join(HUD)).unwrap()
}

fn turn(w: &mut World, id: &str, enabled: bool) {
    installer::set_enabled(&mut w.state, &w.paths, GAME, &w.game, id, enabled).unwrap();
}

#[tokio::test]
async fn a_patch_replaces_the_file_of_its_mod_and_gives_it_back() {
    let mut w = World::new().await;
    publish_cool_hud(&mut w).await;
    w.install(COOL_HUD, NexusFile::Link(&link(COOL_HUD)))
        .await
        .unwrap();
    let before = snapshot(&w.game);
    w.publish(6000, "HUD Remix", "2.0", &[], &[(HUD, b"remix")]).await;

    w.install(6000, NexusFile::Link(&link(6000))).await.unwrap();
    assert_eq!(hud_file(&w), b"remix");
    let mut mods = w.state.list(GAME).unwrap();
    installer::mark_covered(&mut mods);
    let hud = mods.iter().find(|m| m.id == "nexus-5000").unwrap();
    assert_eq!(hud.covered_by, ["HUD Remix"]);
    assert!(!hud.deployment.files.iter().any(|f| f == HUD));
    assert!(installer::missing(&w.state, GAME, &w.game).unwrap().is_empty());

    // Off, its mod's file is back. On again, it wins again.
    turn(&mut w, REMIX, false);
    assert_eq!(snapshot(&w.game), before);
    turn(&mut w, REMIX, true);
    assert_eq!(hud_file(&w), b"remix");

    // Removed then undone: the same. Removed for good: nothing left of it.
    let removed = installer::uninstall(&mut w.state, &w.paths, GAME, &w.game, REMIX).unwrap();
    assert_eq!(snapshot(&w.game), before);
    installer::restore(&mut w.state, &w.paths, &w.game, removed).unwrap();
    assert_eq!(hud_file(&w), b"remix");
    installer::uninstall(&mut w.state, &w.paths, GAME, &w.game, REMIX).unwrap();
    assert_eq!(snapshot(&w.game), before);
}

#[tokio::test]
async fn the_mod_under_a_patch_stays_under_it() {
    let mut w = hud_and_its_patch().await;

    // Updated, turned off and on: it doesn't go back over the patch.
    w.install(COOL_HUD, NexusFile::Link(&link(COOL_HUD)))
        .await
        .unwrap();
    assert_eq!(hud_file(&w), b"remix");
    turn(&mut w, "nexus-5000", false);
    assert_eq!(hud_file(&w), b"remix");
    assert!(!w.game.join("archive/pc/mod/coolhud.archive").is_file());
    turn(&mut w, "nexus-5000", true);
    assert_eq!(hud_file(&w), b"remix");
    assert!(w.game.join("archive/pc/mod/coolhud.archive").is_file());

    // Files gone from the game: each mod gets its own back.
    fs::remove_file(w.game.join(HUD)).unwrap();
    let missing = installer::missing(&w.state, GAME, &w.game).unwrap();
    assert_eq!(missing.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), [REMIX]);
    installer::put_back(&mut w.state, &w.paths, GAME, &w.game, REMIX).unwrap();
    assert_eq!(hud_file(&w), b"remix");

    // Removing the mod leaves the patch's file.
    installer::uninstall(&mut w.state, &w.paths, GAME, &w.game, "nexus-5000").unwrap();
    assert_eq!(hud_file(&w), b"remix");
    installer::uninstall(&mut w.state, &w.paths, GAME, &w.game, REMIX).unwrap();
    assert!(!w.game.join(HUD).exists());
}

#[tokio::test]
async fn a_file_shared_by_three_mods_goes_down_the_order() {
    let mut w = hud_and_its_patch().await;
    w.publish(6001, "HUD Fix", "1.0", &[], &[(HUD, b"fix")]).await;
    w.install(6001, NexusFile::Link(&link(6001))).await.unwrap();
    assert_eq!(hud_file(&w), b"fix");

    installer::uninstall(&mut w.state, &w.paths, GAME, &w.game, "nexus-6001").unwrap();
    assert_eq!(hud_file(&w), b"remix");
    installer::uninstall(&mut w.state, &w.paths, GAME, &w.game, REMIX).unwrap();
    assert_eq!(hud_file(&w), b"hud");
}

#[tokio::test]
async fn a_mod_can_ship_a_file_of_the_framework_installed_with_it() {
    let mut w = World::new().await;
    w.publish_frameworks().await;
    w.publish(
        6002,
        "Bundled",
        "1.0",
        &[(2380, "RED4ext")],
        &[("red4ext/RED4ext.dll", b"bundled"), ("r6/scripts/Bundled/a.reds", b"a")],
    )
    .await;

    w.install(6002, NexusFile::Link(&link(6002))).await.unwrap();
    assert_eq!(fs::read(w.game.join("red4ext/RED4ext.dll")).unwrap(), b"bundled");

    installer::uninstall(&mut w.state, &w.paths, GAME, &w.game, "nexus-6002").unwrap();
    assert_eq!(fs::read(w.game.join("red4ext/RED4ext.dll")).unwrap(), b"r4e");
}

const PATCHED: u32 = 7000;
const PATCH_FILE: u32 = 70001;

/// A mod and an optional patch of its own, like Factions Evolved's.
async fn publish_patched(w: &mut World) {
    w.publish_with(
        PATCHED,
        "Base",
        "3.01",
        &[],
        &[("r6/scripts/Base/base.reds", b"base"), ("r6/scripts/Base/shared.reds", b"base")],
        Some((PATCH_FILE, "Base Patch", "1.5l", vec![("r6/scripts/Base/shared.reds", b"patch")])),
    )
    .await;
}

fn patch_link() -> NxmLink {
    NxmLink::parse(&format!(
        "nxm://cyberpunk2077/mods/{PATCHED}/files/{PATCH_FILE}?key=site-key&expires=1792500000&user_id=1"
    ))
    .unwrap()
}

fn assert_patched(w: &World) {
    assert_eq!(fs::read(w.game.join("r6/scripts/Base/base.reds")).unwrap(), b"base");
    assert_eq!(fs::read(w.game.join("r6/scripts/Base/shared.reds")).unwrap(), b"patch");
    let m = w.state.get(GAME, "nexus-7000").unwrap().unwrap();
    assert_eq!(m.files, [PATCHED * 10, PATCH_FILE]);
    assert_eq!(m.version, "3.01");
}

#[tokio::test]
async fn an_optional_file_joins_the_mod_instead_of_replacing_it() {
    let mut w = World::new().await;
    publish_patched(&mut w).await;
    w.install(PATCHED, NexusFile::Link(&link(PATCHED))).await.unwrap();
    w.install(PATCHED, NexusFile::Link(&patch_link())).await.unwrap();
    assert_patched(&w);

    // The same link again: still both.
    w.install(PATCHED, NexusFile::Link(&patch_link())).await.unwrap();
    assert_patched(&w);
}

#[tokio::test]
async fn the_main_file_keeps_an_optional_file_installed_first() {
    let mut w = World::new().await;
    publish_patched(&mut w).await;
    w.install(PATCHED, NexusFile::Link(&patch_link())).await.unwrap();
    w.install(PATCHED, NexusFile::Link(&link(PATCHED))).await.unwrap();
    assert_patched(&w);
}

#[tokio::test]
async fn a_mod_without_its_main_file_is_offered_no_update_it_would_not_install() {
    let mut w = World::new().await;
    publish_patched(&mut w).await;
    w.install(PATCHED, NexusFile::Link(&patch_link())).await.unwrap();
    w.graphql(
        vec![
            Matcher::Regex(r"nodes \{ modId version \}".into()),
            mod_id_matcher(PATCHED),
        ],
        json!({ "legacyModsByDomain": { "nodes": [{ "modId": PATCHED, "version": "3.01" }] } }),
    )
    .await;

    let installed = w.state.list(GAME).unwrap();
    assert_eq!(installed[0].version, "1.5l");
    let updates = available_updates_nexus(&w.nexus, &w.github, &w.profile, &installed, None).await;
    assert!(updates.is_empty(), "{updates:?}");
}

#[tokio::test]
async fn a_mod_remembers_what_it_needs_until_it_is_installed() {
    let mut w = World::new().await;
    w.publish(
        6005,
        "Needy",
        "1.0",
        &[
            (6006, "Helper|MANDATORY"),
            (6007, "Logs|OPTIONAL: (to view logs)"),
            (6008, "Addon|ADDON: modded vehicle list"),
            (6009, "Other|Optional - MANDATORY for the Other addon"),
            (6010, "Nice|Not required to function"),
        ],
        &[("r6/scripts/Needy/a.reds", b"a"), ("r6/scripts/Helper/shared.reds", b"needy")],
    )
    .await;
    let installed = w.install(6005, NexusFile::Link(&link(6005))).await.unwrap();
    assert_eq!(installed.missing.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["Helper"]);

    let needs = |w: &World| {
        let mut mods = w.state.list(GAME).unwrap();
        installer::drop_installed_needs(&mut mods);
        let needy = mods.into_iter().find(|m| m.id == "nexus-6005").unwrap();
        needy.needs.into_iter().map(|r| r.name).collect::<Vec<_>>()
    };
    assert_eq!(needs(&w), ["Helper"]);

    w.publish(6006, "Helper", "1.0", &[], &[("r6/scripts/Helper/shared.reds", b"helper")])
        .await;
    w.install(6006, NexusFile::Link(&link(6006))).await.unwrap();
    assert!(needs(&w).is_empty());

    // Installed after the mod that requires it, it still goes under it: the
    // mod's replacement of its file stays.
    assert_eq!(fs::read(w.game.join("r6/scripts/Helper/shared.reds")).unwrap(), b"needy");
    let needy = w.state.get(GAME, "nexus-6005").unwrap().unwrap();
    let helper = w.state.get(GAME, "nexus-6006").unwrap().unwrap();
    assert!(helper.priority < needy.priority);

    // Reinstalled, the mod remembers it, so a later reinstall of what it
    // requires goes under it again.
    w.install(6005, NexusFile::Link(&link(6005))).await.unwrap();
    installer::uninstall(&mut w.state, &w.paths, GAME, &w.game, "nexus-6006").unwrap();
    w.install(6006, NexusFile::Link(&link(6006))).await.unwrap();
    assert_eq!(fs::read(w.game.join("r6/scripts/Helper/shared.reds")).unwrap(), b"needy");
}

#[tokio::test]
async fn a_covered_config_keeps_the_players_edits() {
    let config = "r6/config/settings/remix.json";
    let mut w = World::new().await;
    w.publish(6003, "Base", "1.0", &[], &[(config, b"{\"a\":1}")]).await;
    w.install(6003, NexusFile::Link(&link(6003))).await.unwrap();
    fs::write(w.game.join(config), "{\"a\":2}").unwrap();

    w.publish(6004, "Patch", "1.0", &[], &[(config, b"{\"a\":3}")]).await;
    w.install(6004, NexusFile::Link(&link(6004))).await.unwrap();
    assert_eq!(fs::read(w.game.join(config)).unwrap(), b"{\"a\":3}");

    installer::uninstall(&mut w.state, &w.paths, GAME, &w.game, "nexus-6004").unwrap();
    assert_eq!(fs::read(w.game.join(config)).unwrap(), b"{\"a\":2}");
}

#[tokio::test]
async fn archives_with_nothing_to_install_are_refused() {
    let mut w = World::new().await;
    w.publish(
        6100,
        "Screenshots",
        "1.0",
        &[],
        &[("shot.png", b"png"), ("readme.md", b"md")],
    )
    .await;
    let err = w
        .install(6100, NexusFile::Link(&link(6100)))
        .await
        .unwrap_err();
    assert!(matches!(err, InstallError::NothingToInstall(_)), "{err:?}");
}

#[tokio::test]
async fn cheat_tables_programs_and_scripts_are_named_not_installed() {
    let mut w = World::new().await;
    w.publish(6101, "Cheat Table", "1.0", &[], &[("Game.CT", b"ct"), ("readme.txt", b"txt")])
        .await;
    w.publish(6102, "Trainer", "1.0", &[], &[("Trainer/Trainer.exe", b"exe")])
        .await;
    w.publish(
        6103,
        "Tweaks",
        "1.0",
        &[],
        &[("Tweaks/Install.bat", b"bat"), ("Tweaks/Tweaks.exe", b"exe")],
    )
    .await;
    for (id, kind) in [
        (6101, NotAMod::CheatTable),
        (6102, NotAMod::Program),
        (6103, NotAMod::Script),
    ] {
        let err = w.install(id, NexusFile::Link(&link(id))).await.unwrap_err();
        assert!(
            matches!(err, InstallError::NotAMod { kind: k, .. } if k == kind),
            "{err:?}"
        );
    }
}

#[tokio::test]
async fn updates_come_from_github_for_frameworks_and_nexus_for_mods() {
    let mut w = World::new().await;
    publish_cool_hud(&mut w).await;
    w.install(COOL_HUD, NexusFile::Link(&link(COOL_HUD)))
        .await
        .unwrap();
    // A more recent version of Cool HUD on Nexus.
    w.graphql(
        vec![
            Matcher::Regex(r"nodes \{ modId version \}".into()),
            mod_id_matcher(COOL_HUD),
        ],
        json!({ "legacyModsByDomain": { "nodes": [{ "modId": COOL_HUD, "version": "1.3.0" }] } }),
    )
    .await;

    let installed = w.state.list(GAME).unwrap();
    let updates = available_updates_nexus(&w.nexus, &w.github, &w.profile, &installed, None).await;
    let updates: Vec<_> = updates
        .iter()
        .map(|u| (u.mod_id.as_str(), u.latest.as_str()))
        .collect();
    // The frameworks were just installed at their latest version.
    assert_eq!(updates, [("nexus-5000", "1.3.0")]);
}

/// The same file published by the author on GitHub, and what Nexus answers
/// when asked who owns its hash.
async fn mirror_on_github(w: &mut World, bytes: Vec<u8>, owner_mod: u32, owner_file: u32) {
    w.server
        .mock("GET", "/repos/someone/cool-hud/releases/latest")
        .with_body(
            json!({
                "tag_name": "v1.2.0",
                "assets": [
                    { "name": "notes.txt", "browser_download_url": format!("{}/gh/notes.txt", w.server.url()), "size": 3 },
                    { "name": "CoolHud.zip", "browser_download_url": format!("{}/gh/CoolHud.zip", w.server.url()), "size": bytes.len() }
                ]
            })
            .to_string(),
        )
        .create_async()
        .await;
    w.server
        .mock("GET", "/gh/CoolHud.zip")
        .with_body(bytes)
        .create_async()
        .await;
    w.server
        .mock(
            "GET",
            Matcher::Regex(r"^/v1/games/cyberpunk2077/mods/md5_search/[0-9a-f]{32}\.json$".into()),
        )
        .with_body(
            json!([{ "mod": { "mod_id": owner_mod }, "file_details": { "file_id": owner_file } }])
                .to_string(),
        )
        .create_async()
        .await;
}

fn cool_hud_files() -> Vec<(&'static str, &'static [u8])> {
    vec![("r6/scripts/CoolHud/hud.reds", b"hud")]
}

#[tokio::test]
async fn a_verified_github_copy_spares_the_website() {
    let mut w = World::new().await;
    w.description = "Source code: [url=https://github.com/someone/cool-hud]GitHub[/url]. \
        Built with https://github.com/jac3km4/redscript"
        .into();
    let files = cool_hud_files();
    w.publish(COOL_HUD, "Cool HUD", "1.2.0", &[], &files).await;
    mirror_on_github(&mut w, zip_bytes(&files), COOL_HUD, COOL_HUD * 10).await;

    // Free account, no site link: Nexus would refuse, GitHub serves.
    let done = w.install(COOL_HUD, NexusFile::Main).await.unwrap();
    assert!(w.game.join("r6/scripts/CoolHud/hud.reds").is_file());
    let hud = done.mods.iter().find(|m| m.id == "nexus-5000").unwrap();
    assert_eq!(hud.version, "1.2.0");
}

#[tokio::test]
async fn a_github_file_that_nexus_attributes_elsewhere_is_not_used() {
    let mut w = World::new().await;
    w.description = "https://github.com/someone/cool-hud".into();
    let files = cool_hud_files();
    w.publish(COOL_HUD, "Cool HUD", "1.2.0", &[], &files).await;
    // Same bytes, but Nexus ties them to another mod (a reuploaded copy):
    // that doesn't prove it is this file.
    mirror_on_github(&mut w, zip_bytes(&files), 31005, 1).await;
    let before = snapshot(&w.game);

    let err = w.install(COOL_HUD, NexusFile::Main).await.unwrap_err();
    assert!(
        matches!(err, InstallError::NexusWebsite { file_id: 50000 }),
        "{err:?}"
    );
    assert_eq!(snapshot(&w.game), before);
}

#[tokio::test]
async fn a_game_kept_on_an_older_version_gets_the_files_of_its_time() {
    let mut w = World::new().await;
    w.before = w.profile.mods_before("2.31").map(str::to_owned);
    assert_eq!(w.before.as_deref(), Some("2026-10-22"));
    assert_eq!(w.profile.mods_before("2.5"), None);

    // RED4ext: a release for 2.5, a prerelease, and the one before.
    let old_red4ext = format!("{}/assets/red4ext-1.29.1.zip", w.server.url());
    w.server
        .mock("GET", "/repos/wopss/RED4ext/releases")
        .match_query(Matcher::Any)
        .with_body(
            json!([
                { "tag_name": "v1.31.0", "published_at": "2026-10-23T09:00:00Z",
                  "draft": false, "prerelease": false, "assets": [
                    { "name": "red4ext-1.31.0.zip", "browser_download_url": "https://example.invalid/new", "size": 1 }
                ] },
                { "tag_name": "v1.31.0-rc1", "published_at": "2026-10-21T09:00:00Z",
                  "draft": false, "prerelease": true, "assets": [] },
                { "tag_name": "v1.29.1", "published_at": "2025-09-12T09:00:00Z",
                  "draft": false, "prerelease": false, "assets": [
                    { "name": "red4ext-1.29.1.zip", "browser_download_url": old_red4ext, "size": 1 }
                ] }
            ])
            .to_string(),
        )
        .create_async()
        .await;
    w.server
        .mock("GET", "/assets/red4ext-1.29.1.zip")
        .with_body(zip_bytes(&[("red4ext/RED4ext.dll", b"old r4e")]))
        .create_async()
        .await;

    // The mod: its file for 2.5 (the main one) and the one before.
    let mod_id = 6000;
    w.graphql(
        vec![
            Matcher::Regex("modRequirements".into()),
            mod_id_matcher(mod_id),
        ],
        json!({ "legacyModsByDomain": { "nodes": [{
            "modId": mod_id, "name": "Cool Plugin", "summary": "", "author": "Someone",
            "thumbnailUrl": null, "pictureUrl": null, "downloads": 1,
            "updatedAt": "2026-10-23T00:00:00Z", "createdAt": "2026-01-01T00:00:00Z",
            "version": "2.0", "adultContent": false, "status": "published",
            "description": "", "modCategory": { "name": "Gameplay" }, "gameId": 3333,
            "modRequirements": { "nexusRequirements": { "nodes": [] } }
        }] } }),
    )
    .await;
    let archive = zip_bytes(&[("red4ext/plugins/Cool/Cool.dll", b"for 2.31")]);
    w.graphql(
        vec![
            Matcher::Regex("modFiles".into()),
            Matcher::Regex(format!(r#""mod":"{mod_id}""#)),
        ],
        json!({ "modFiles": [
            { "fileId": 60002, "name": "Cool Plugin", "version": "2.0", "category": "MAIN",
              "primary": 1, "sizeInBytes": "10", "date": 1_792_752_726 },
            { "fileId": 60001, "name": "Cool Plugin", "version": "1.4", "category": "OLD_VERSION",
              "primary": 0, "sizeInBytes": archive.len().to_string(), "date": 1_780_000_000 }
        ] }),
    )
    .await;
    // Only the old file can be downloaded: asking for the other fails.
    w.server
        .mock(
            "GET",
            "/v1/games/cyberpunk2077/mods/6000/files/60001/download_link.json",
        )
        .match_query(Matcher::Any)
        .with_body(
            json!([{ "name": "Nexus CDN", "short_name": "CDN",
                     "URI": format!("{}/cdn/6000-old.zip", w.server.url()) }])
            .to_string(),
        )
        .create_async()
        .await;
    w.server
        .mock("GET", "/cdn/6000-old.zip")
        .with_body(archive)
        .create_async()
        .await;

    let done = w.install(mod_id, NexusFile::Main).await.unwrap();

    assert_eq!(
        fs::read(w.game.join("red4ext/plugins/Cool/Cool.dll")).unwrap(),
        b"for 2.31"
    );
    assert_eq!(
        fs::read(w.game.join("red4ext/RED4ext.dll")).unwrap(),
        b"old r4e"
    );
    let versions: BTreeMap<_, _> = done
        .mods
        .iter()
        .map(|m| (m.id.as_str(), m.version.as_str()))
        .collect();
    assert_eq!(
        versions,
        BTreeMap::from([("nexus-2380", "1.29.1"), ("nexus-6000", "1.4")])
    );

    // Nothing more recent from before 2.5: no update offered.
    let installed = w.state.list(GAME).unwrap();
    let updates = available_updates_nexus(
        &w.nexus,
        &w.github,
        &w.profile,
        &installed,
        w.before.as_deref(),
    )
    .await;
    assert!(updates.is_empty(), "{updates:?}");

    // Installed for 2.5 before going back: the previous version is
    // offered.
    let mut newer = installed
        .iter()
        .find(|m| m.id == "nexus-6000")
        .unwrap()
        .clone();
    newer.version = "2.0".into();
    w.state.save_all(&[newer]).unwrap();
    let installed = w.state.list(GAME).unwrap();
    let updates = available_updates_nexus(
        &w.nexus,
        &w.github,
        &w.profile,
        &installed,
        w.before.as_deref(),
    )
    .await;
    let back: Vec<_> = updates
        .iter()
        .map(|u| (u.mod_id.as_str(), u.current.as_str(), u.latest.as_str()))
        .collect();
    assert_eq!(back, [("nexus-6000", "2.0", "1.4")]);
}

/// The Porsche 911 GT3 RS (mod 11180): a main file and color variants, all
/// "MAIN", none marked as primary. Each color replaces the main file's
/// archive.
async fn publish_gt3(w: &mut World) {
    let mod_id = 11180;
    w.graphql(
        vec![
            Matcher::Regex("modRequirements".into()),
            mod_id_matcher(mod_id),
        ],
        json!({ "legacyModsByDomain": { "nodes": [{
            "modId": mod_id, "name": "Porsche 911 GT3 RS", "summary": "", "author": "aalexbug",
            "thumbnailUrl": null, "pictureUrl": null, "downloads": 1,
            "updatedAt": "2023-12-24T00:00:00Z", "createdAt": "2023-11-23T00:00:00Z",
            "version": "1.2", "adultContent": false, "status": "published",
            "description": "", "modCategory": { "name": "Vehicles" }, "gameId": 3333,
            "modRequirements": { "nexusRequirements": { "nodes": [] } }
        }] } }),
    )
    .await;
    let main: Files = vec![
        ("archive/pc/mod/porsche_gt3_rs.archive", b"white"),
        ("archive/pc/mod/porsche_gt3_rs.xl", b"xl"),
        (
            "archive/pc/mod/porsche_gt3_rs_interior.archive",
            b"interior",
        ),
    ];
    let black: Files = vec![
        ("archive/pc/mod/porsche_gt3_rs.archive", b"black yellow"),
        ("archive/pc/mod/porsche_gt3_rs.xl", b"xl"),
    ];
    let file = |id: u32, name: &str, category: &str, date: i64| {
        json!({ "fileId": id, "name": name, "version": "1.2", "category": category,
                "primary": 0, "sizeInBytes": "10", "date": date,
                "description": "DRAG THE ARCHIVE FOLDER IN THE GAME MAIN FOLDER" })
    };
    w.graphql(
        vec![
            Matcher::Regex("modFiles".into()),
            Matcher::Regex(format!(r#""mod":"{mod_id}""#)),
        ],
        json!({ "modFiles": [
            file(59389, "Porsche 911 GT3 RS BLACK_YELLOW", "ARCHIVED", 1_701_369_886),
            file(62171, "Porsche 911 GT3 RS (Main File)you need this", "MAIN", 1_703_436_412),
            file(62172, "Porsche 911 GT3 RS Black_Yellow", "MAIN", 1_703_436_457),
            file(62175, "Porsche 911 GT3 RS White_Black", "MAIN", 1_703_436_525),
        ] }),
    )
    .await;
    for (file_id, files) in [(62171, main), (62172, black)] {
        let path =
            format!("/v1/games/cyberpunk2077/mods/{mod_id}/files/{file_id}/download_link.json");
        let download = format!("/cdn/{file_id}.zip");
        w.server
            .mock("GET", path.as_str())
            .match_query(Matcher::UrlEncoded("key".into(), "site-key".into()))
            .with_body(
                json!([{ "name": "Nexus CDN", "short_name": "CDN",
                         "URI": format!("{}{download}", w.server.url()) }])
                .to_string(),
            )
            .create_async()
            .await;
        w.server
            .mock("GET", path.as_str())
            .match_query(Matcher::Missing)
            .with_status(403)
            .create_async()
            .await;
        w.server
            .mock("GET", download.as_str())
            .with_body(zip_bytes(&files))
            .create_async()
            .await;
    }
}

fn file_link(mod_id: u32, file_id: u32) -> NxmLink {
    NxmLink::parse(&format!(
        "nxm://cyberpunk2077/mods/{mod_id}/files/{file_id}?key=site-key&expires=1792500000"
    ))
    .unwrap()
}

#[tokio::test]
async fn a_color_variant_goes_on_top_of_the_main_file() {
    let mut w = World::new().await;
    publish_gt3(&mut w).await;

    // Several files: the user picks, the main one first.
    let err = w.install(11180, NexusFile::Main).await.unwrap_err();
    let InstallError::ChooseFiles(files) = err else {
        panic!("{err:?}");
    };
    let ids: Vec<u32> = files.iter().map(|f| f.file_id).collect();
    assert_eq!(ids, [62171, 62175, 62172]);

    // Free account: the main one comes from the site, the color must follow.
    let chosen = [62171, 62172];
    let main = file_link(11180, 62171);
    let err = w
        .install(
            11180,
            NexusFile::Files {
                ids: &chosen,
                link: Some(&main),
            },
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, InstallError::NexusWebsite { file_id: 62172 }),
        "{err:?}"
    );
    assert!(!w.game.join("archive").exists());

    // The color arrives: the main one is already there, everything is placed.
    let color = file_link(11180, 62172);
    let done = w
        .install(
            11180,
            NexusFile::Files {
                ids: &chosen,
                link: Some(&color),
            },
        )
        .await
        .unwrap();
    let mod_dir = w.game.join("archive/pc/mod");
    assert_eq!(
        fs::read(mod_dir.join("porsche_gt3_rs.archive")).unwrap(),
        b"black yellow"
    );
    assert!(mod_dir.join("porsche_gt3_rs_interior.archive").is_file());
    let gt3 = done.mods.iter().find(|m| m.id == "nexus-11180").unwrap();
    assert_eq!(gt3.files, [62171, 62172]);
    assert_eq!(gt3.name, "Porsche 911 GT3 RS");

    // Reinstalling (update) keeps the same choice, asking for nothing.
    let done = w.install(11180, NexusFile::Main).await.unwrap();
    let gt3 = done.mods.iter().find(|m| m.id == "nexus-11180").unwrap();
    assert_eq!(gt3.files, [62171, 62172]);
    assert_eq!(
        fs::read(mod_dir.join("porsche_gt3_rs.archive")).unwrap(),
        b"black yellow"
    );
}

/// Automatically recognized games: one request for every name, exact name
/// only.
#[tokio::test]
async fn games_are_found_by_their_exact_name() {
    let mut server = mockito::Server::new_async().await;
    let nexus = Nexus::with_urls(
        reqwest::Client::new(),
        &format!("{}/graphql", server.url()),
        &format!("{}/v1", server.url()),
    );
    let mock = server
        .mock("POST", "/graphql")
        .match_body(Matcher::PartialJson(json!({
            "variables": { "n0": "hogwarts legacy", "n1": "nothing here", "n2": "skyrim" }
        })))
        .with_body(
            json!({ "data": {
                "g0": { "nodes": [{ "id": 5113, "name": "Hogwarts Legacy", "domainName": "hogwartslegacy" }] },
                "g1": { "nodes": [] },
                // Close but not identical name: refused.
                "g2": { "nodes": [{ "id": 1704, "name": "Skyrim Special Edition", "domainName": "skyrimspecialedition" }] }
            }})
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;

    let found = nexus
        .find_games(&["Hogwarts Legacy™".into(), "Nothing Here".into(), "Skyrim".into()])
        .await
        .unwrap();
    mock.assert_async().await;
    assert_eq!(found.len(), 3);
    let hogwarts = found[0].as_ref().unwrap();
    assert_eq!((hogwarts.domain.as_str(), hogwarts.game_id), ("hogwartslegacy", 5113));
    assert!(found[1].is_none());
    assert!(found[2].is_none());
}

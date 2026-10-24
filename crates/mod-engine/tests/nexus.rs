//! Cyberpunk 2077 de bout en bout : un mod Nexus (lien `nxm://`), ses
//! frameworks installés tout seuls depuis GitHub, les conflits, contre de
//! faux Nexus et GitHub qui servent de vraies archives.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use mockito::{Matcher, ServerGuard};
use mod_engine::games::{GameCatalog, GameProfile};
use mod_engine::installer::{
    InstallError, Installer, NexusFile, NexusInstall, Paths, Target, available_updates_nexus,
};
use mod_engine::sources::SourceError;
use mod_engine::sources::github::GitHub;
use mod_engine::sources::nexus::{Nexus, NxmLink};
use mod_engine::state::State;
use serde_json::json;
use zip::write::SimpleFileOptions;

/// Fichiers d'une archive : chemin et contenu.
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
    /// Description des prochains mods publiés (on y cite des dépôts GitHub).
    description: String,
    /// Jeu resté sur une ancienne version : mods publiés avant cette date.
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

    /// Les frameworks sur le faux GitHub, avec la disposition de leurs
    /// vraies archives.
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

    /// Un mod sur le faux Nexus : fiche, fichiers, prérequis, archive.
    async fn publish(
        &mut self,
        mod_id: u32,
        name: &str,
        version: &str,
        requirements: &[(u32, &str)],
        files: &[(&str, &[u8])],
    ) {
        let file_id = mod_id * 10;
        let archive = zip_bytes(files);
        let requirements: Vec<_> = requirements
            .iter()
            .map(|(id, name)| {
                json!({ "modId": id.to_string(), "modName": name, "gameId": "3333",
                        "externalRequirement": false, "url": "" })
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
        self.graphql(
            vec![
                Matcher::Regex("modFiles".into()),
                Matcher::Regex(format!(r#""mod":"{mod_id}""#)),
            ],
            json!({ "modFiles": [
                { "fileId": file_id - 1, "name": name, "version": "0.9", "category": "OLD_VERSION",
                  "primary": 0, "sizeInBytes": "10", "date": 1_700_000_000 },
                { "fileId": file_id, "name": name, "version": version, "category": "MAIN",
                  "primary": 1, "sizeInBytes": archive.len().to_string(), "date": 1_790_000_000 }
            ] }),
        )
        .await;
        let download = format!("/cdn/{mod_id}.zip");
        // Compte gratuit : le lien n'est donné qu'avec la clé du lien nxm://.
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
    // Le mod, rangé malgré son dossier d'emballage, sans son readme.
    assert!(game.join("r6/scripts/CoolHud/hud.reds").is_file());
    assert!(game.join("archive/pc/mod/coolhud.archive.xl").is_file());
    assert!(!game.join("README.txt").exists());
    // redscript (.reds), ArchiveXL (.xl), Codeware (prérequis Nexus), et
    // RED4ext dont ArchiveXL et Codeware ont besoin à leur tour.
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

    // Mod Settings n'est pas un framework : l'utilisateur doit passer par
    // le site (compte gratuit).
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
        matches!(err, InstallError::Source(SourceError::WebsiteOnly { .. })),
        "{err:?}"
    );
    assert_eq!(snapshot(&w.game), before);
    assert!(w.state.list(GAME).unwrap().is_empty());
}

#[tokio::test]
async fn frameworks_install_from_github_without_the_website() {
    let mut w = World::new().await;
    // ArchiveXL demandé directement : pas besoin de Nexus, ni de Premium.
    let done = w.install(4198, NexusFile::Main).await.unwrap();
    let mut ids: Vec<_> = done
        .mods
        .iter()
        .map(|m| (m.id.as_str(), m.explicit))
        .collect();
    ids.sort();
    assert_eq!(ids, [("nexus-2380", false), ("nexus-4198", true)]);
}

#[tokio::test]
async fn a_conflicting_mod_names_the_mod_it_collides_with() {
    let mut w = World::new().await;
    publish_cool_hud(&mut w).await;
    w.install(COOL_HUD, NexusFile::Link(&link(COOL_HUD)))
        .await
        .unwrap();
    let before = snapshot(&w.game);

    w.publish(
        6000,
        "HUD Remix",
        "2.0",
        &[],
        &[("r6/scripts/CoolHud/hud.reds", b"remix")],
    )
    .await;
    let err = w
        .install(6000, NexusFile::Link(&link(6000)))
        .await
        .unwrap_err();
    match err {
        InstallError::Conflict { path, other } => {
            assert_eq!(path, "r6/scripts/CoolHud/hud.reds");
            assert_eq!(other, "Cool HUD");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(snapshot(&w.game), before);
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
async fn updates_come_from_github_for_frameworks_and_nexus_for_mods() {
    let mut w = World::new().await;
    publish_cool_hud(&mut w).await;
    w.install(COOL_HUD, NexusFile::Link(&link(COOL_HUD)))
        .await
        .unwrap();
    // Une version plus récente de Cool HUD sur Nexus.
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
    // Les frameworks viennent d'être installés à leur dernière version.
    assert_eq!(updates, [("nexus-5000", "1.3.0")]);
}

/// Le même fichier publié par l'auteur sur GitHub, et ce que Nexus répond
/// quand on lui demande à qui appartient son empreinte.
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

    // Compte gratuit, sans lien du site : Nexus refuserait, GitHub sert.
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
    // Même octets, mais Nexus les rattache à un autre mod (une copie
    // republiée) : ce n'est pas la preuve qu'il s'agit de ce fichier.
    mirror_on_github(&mut w, zip_bytes(&files), 31005, 1).await;
    let before = snapshot(&w.game);

    let err = w.install(COOL_HUD, NexusFile::Main).await.unwrap_err();
    assert!(
        matches!(err, InstallError::Source(SourceError::WebsiteOnly { .. })),
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

    // RED4ext : une release pour la 2.5, une préversion, et celle d'avant.
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

    // Le mod : son fichier pour la 2.5 (le principal) et celui d'avant.
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
    // Seul l'ancien fichier peut être téléchargé : demander l'autre échoue.
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

    // Rien de plus récent d'avant la 2.5 : pas de mise à jour proposée.
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

    // Installé pour la 2.5 avant le retour en arrière : la version d'avant
    // est proposée.
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

//! Installation en un clic de bout en bout, contre un faux Thunderstore qui
//! sert de vraies archives, sur le faux Silksong de `fixtures/steam`.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use mockito::{Mock, ServerGuard};
use mod_engine::games::GameCatalog;
use mod_engine::installer::{self, InstallError, Installer, Paths, Progress, Target};
use mod_engine::sources::thunderstore::Thunderstore;
use mod_engine::state::State;
use zip::write::SimpleFileOptions;

const GAME: &str = "../../fixtures/steam/root/steamapps/common/Hollow Knight Silksong";
const COMMUNITY: &str = "hollow-knight-silksong";
const LOADER: &str = "silksong_modding-BepInExPack_Silksong";
const PREPATCHER: &str = "silksong_modding-SilksongPrepatcher";
const KNIGHT: &str = "shownyoung-KnightInSilksong";

struct World {
    _tmp: tempfile::TempDir,
    server: ServerGuard,
    game: PathBuf,
    paths: Paths,
    catalog: GameCatalog,
    source: Thunderstore,
    client: reqwest::Client,
    state: State,
    community: &'static str,
}

impl World {
    async fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let game = tmp.path().join("game");
        copy_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join(GAME), &game);
        let schema = fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/thunderstore/schema.json"),
        )
        .unwrap();
        let server = mockito::Server::new_async().await;
        let client = reqwest::Client::new();
        Self {
            source: Thunderstore::with_base_url(client.clone(), &server.url()),
            paths: Paths {
                store: tmp.path().join("store"),
                downloads: tmp.path().join("downloads"),
            },
            catalog: GameCatalog::from_schema_json(&schema).unwrap(),
            state: State::open(&tmp.path().join("modalia.db")).unwrap(),
            client,
            server,
            game,
            community: COMMUNITY,
            _tmp: tmp,
        }
    }

    /// Publie un paquet sur le faux Thunderstore. Renvoie la route de
    /// téléchargement, pour compter les appels.
    async fn publish(&mut self, id: &str, version: &str, deps: &[&str], archive: Vec<u8>) -> Mock {
        let (namespace, name) = id.split_once('-').unwrap();
        let download = format!("/dl/{id}-{version}.zip");
        let json = serde_json::json!({
            "namespace": namespace, "name": name, "full_name": id, "owner": namespace,
            "date_updated": "2026-01-01T00:00:00Z", "is_deprecated": false, "total_downloads": 10,
            "latest": {
                "version_number": version, "description": format!("{name} test"), "icon": null,
                "dependencies": deps, "download_url": format!("{}{download}", self.server.url()),
                "date_created": "2026-01-01T00:00:00Z", "website_url": ""
            },
            "community_listings": [{ "community": self.community, "categories": ["Mods"] }]
        });
        self.server
            .mock(
                "GET",
                format!("/api/experimental/package/{namespace}/{name}/").as_str(),
            )
            .with_body(json.to_string())
            .create_async()
            .await;
        self.server
            .mock("GET", download.as_str())
            .with_body(archive)
            .create_async()
            .await
    }

    async fn publish_knight(&mut self) -> Vec<Mock> {
        vec![
            self.publish(LOADER, "1.0.3", &[], loader_zip()).await,
            self.publish(
                PREPATCHER,
                "1.4.0",
                &["silksong_modding-BepInExPack_Silksong-1.0.0"],
                mod_zip("Prepatcher"),
            )
            .await,
            self.publish(
                KNIGHT,
                "0.6.7",
                &[
                    "silksong_modding-BepInExPack_Silksong-1.0.3",
                    "silksong_modding-SilksongPrepatcher-1.3.0",
                ],
                mod_zip("Knight"),
            )
            .await,
        ]
    }

    async fn install(
        &mut self,
        id: &str,
    ) -> (
        Result<Vec<mod_engine::state::InstalledMod>, InstallError>,
        Vec<Progress>,
    ) {
        let installer = Installer {
            source: &self.source,
            catalog: Some(&self.catalog),
            client: &self.client,
            paths: &self.paths,
            cancel: None,
        };
        let target = Target {
            game: "steam:1030300",
            game_dir: &self.game,
            community: self.community,
        };
        let mut events = Vec::new();
        let result = installer
            .install(&mut self.state, &target, id, |p| events.push(p))
            .await;
        (result, events)
    }
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn snapshot(dir: &Path) -> BTreeMap<String, Option<Vec<u8>>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Option<Vec<u8>>>) {
        for entry in fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            let rel = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if entry.file_type().unwrap().is_dir() {
                out.insert(rel, None);
                walk(root, &path, out);
            } else {
                out.insert(rel, Some(fs::read(&path).unwrap()));
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

fn zip_bytes(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, data) in files {
        w.start_file(*name, SimpleFileOptions::default()).unwrap();
        w.write_all(data).unwrap();
    }
    w.finish().unwrap().into_inner()
}

fn loader_zip() -> Vec<u8> {
    zip_bytes(&[
        ("BepInExPack/winhttp.dll", b"winhttp"),
        ("BepInExPack/doorstop_config.ini", b"[General]"),
        ("BepInExPack/BepInEx/core/BepInEx.dll", b"core"),
        ("manifest.json", b"{}"),
    ])
}

fn mod_zip(name: &str) -> Vec<u8> {
    zip_bytes(&[
        (&format!("plugins/{name}.dll"), name.as_bytes()),
        ("manifest.json", b"{}"),
        ("icon.png", b"png"),
    ])
}

fn plugin(game: &Path, id: &str, dll: &str) -> PathBuf {
    game.join("BepInEx/plugins").join(id).join(dll)
}

// ---------------------------------------------------------------- tests

#[tokio::test]
async fn one_click_installs_the_loader_and_dependencies_in_order() {
    let mut w = World::new().await;
    w.publish_knight().await;

    let (result, events) = w.install(KNIGHT).await;
    let installed = result.unwrap();

    assert!(w.game.join("winhttp.dll").is_file());
    assert!(w.game.join("BepInEx/core/BepInEx.dll").is_file());
    assert!(plugin(&w.game, PREPATCHER, "Prepatcher.dll").is_file());
    assert!(plugin(&w.game, KNIGHT, "Knight.dll").is_file());

    // Loader en tête de liste, puis par nom.
    let ids: Vec<_> = installed.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, [LOADER, KNIGHT, PREPATCHER]);
    let knight = installed.iter().find(|m| m.id == KNIGHT).unwrap();
    assert!(knight.explicit && knight.enabled);
    assert_eq!(knight.version, "0.6.7");
    assert_eq!(knight.sha256.len(), 64);
    assert!(
        !installed
            .iter()
            .find(|m| m.id == PREPATCHER)
            .unwrap()
            .explicit
    );
    assert!(
        installed
            .iter()
            .find(|m| m.id == LOADER)
            .unwrap()
            .is_loader()
    );

    // Progression : on résout, puis chaque paquet est téléchargé puis installé.
    assert_eq!(events.first(), Some(&Progress::Resolving));
    let installing: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Progress::Installing { index, total, .. } => Some((*index, *total)),
            _ => None,
        })
        .collect();
    assert_eq!(installing, [(0, 3), (1, 3), (2, 3)]);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Progress::Downloading { .. }))
    );
}

#[tokio::test]
async fn reinstalling_does_nothing_and_cached_archives_are_reused() {
    let mut w = World::new().await;
    let downloads = w.publish_knight().await;
    w.install(KNIGHT).await.0.unwrap();
    let before = snapshot(&w.game);

    // Déjà installé : aucun nouveau téléchargement, rien ne bouge.
    let (again, events) = w.install(KNIGHT).await;
    assert_eq!(again.unwrap().len(), 3);
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Progress::Downloading { .. }))
    );
    assert_eq!(snapshot(&w.game), before);
    for d in downloads {
        d.expect(1).assert_async().await;
    }
}

#[tokio::test]
async fn a_failing_dependency_rolls_back_the_whole_install() {
    let mut w = World::new().await;
    w.publish(LOADER, "1.0.3", &[], loader_zip()).await;
    // Archive abîmée pour la dépendance du milieu.
    w.publish(PREPATCHER, "1.4.0", &[], b"PK pas un zip".to_vec())
        .await;
    w.publish(
        KNIGHT,
        "0.6.7",
        &[
            "silksong_modding-BepInExPack_Silksong-1.0.3",
            "silksong_modding-SilksongPrepatcher-1.3.0",
        ],
        mod_zip("Knight"),
    )
    .await;
    let original = snapshot(&w.game);

    let (result, _) = w.install(KNIGHT).await;
    let err = result.unwrap_err();

    assert!(matches!(err, InstallError::Archive { .. }), "{err:?}");
    assert_eq!(err.to_string(), "couldn't unpack SilksongPrepatcher");
    assert_eq!(
        snapshot(&w.game),
        original,
        "le loader déjà posé doit avoir été retiré"
    );
    assert!(
        w.state.list("steam:1030300").unwrap().is_empty(),
        "rien ne doit être enregistré"
    );
}

#[tokio::test]
async fn toggling_and_uninstalling_everything_restores_the_game() {
    let mut w = World::new().await;
    w.publish_knight().await;
    let original = snapshot(&w.game);
    w.install(KNIGHT).await.0.unwrap();
    let game = "steam:1030300";
    let knight_dll = plugin(&w.game, KNIGHT, "Knight.dll");

    // Désactiver / réactiver (SPEC §9, scénario 2).
    let off = installer::set_enabled(&mut w.state, &w.paths, game, &w.game, KNIGHT, false).unwrap();
    assert!(!off.enabled && !knight_dll.exists());
    installer::set_enabled(&mut w.state, &w.paths, game, &w.game, KNIGHT, true).unwrap();
    assert!(knight_dll.is_file());
    assert!(matches!(
        installer::set_enabled(&mut w.state, &w.paths, game, &w.game, LOADER, false),
        Err(InstallError::LoaderToggle)
    ));

    // Le loader part en dernier.
    assert!(matches!(
        installer::uninstall(&mut w.state, &w.paths, game, &w.game, LOADER),
        Err(InstallError::LoaderInUse)
    ));

    // Désinstaller, puis annuler (toast « Undo »).
    let removed = installer::uninstall(&mut w.state, &w.paths, game, &w.game, KNIGHT).unwrap();
    assert!(!knight_dll.exists());
    installer::restore(&mut w.state, &w.paths, &w.game, removed).unwrap();
    assert!(knight_dll.is_file());
    assert!(w.state.get(game, KNIGHT).unwrap().unwrap().explicit);

    // BepInEx a tourné et écrit ses journaux.
    fs::write(w.game.join("BepInEx/LogOutput.log"), "log").unwrap();

    // Tout retirer (SPEC §9, scénario 4) : retour exact à l'état d'origine.
    for id in [KNIGHT, PREPATCHER, LOADER] {
        installer::uninstall(&mut w.state, &w.paths, game, &w.game, id).unwrap();
    }
    assert_eq!(snapshot(&w.game), original);
    assert!(w.state.list(game).unwrap().is_empty());
}

#[tokio::test]
async fn removing_every_mod_at_once_restores_the_game() {
    let mut w = World::new().await;
    w.publish_knight().await;
    let original = snapshot(&w.game);
    w.install(KNIGHT).await.0.unwrap();
    let game = "steam:1030300";
    installer::set_enabled(&mut w.state, &w.paths, game, &w.game, KNIGHT, false).unwrap();
    fs::write(w.game.join("BepInEx/LogOutput.log"), "log").unwrap();
    assert_eq!(w.state.games().unwrap(), [game]);

    assert_eq!(installer::uninstall_all(&mut w.state, game, &w.game).unwrap(), 3);
    assert_eq!(snapshot(&w.game), original);
    assert!(w.state.games().unwrap().is_empty());
}

#[tokio::test]
async fn mods_wiped_by_a_game_reinstall_are_put_back_from_storage() {
    let mut w = World::new().await;
    w.publish_knight().await;
    w.install(KNIGHT).await.0.unwrap();
    let game = "steam:1030300";
    let installed = snapshot(&w.game);
    assert!(installer::missing(&w.state, game, &w.game).unwrap().is_empty());

    // Steam désinstalle puis réinstalle le jeu : un dossier neuf.
    fs::remove_dir_all(&w.game).unwrap();
    copy_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join(GAME), &w.game);
    let missing: Vec<_> = installer::missing(&w.state, game, &w.game)
        .unwrap()
        .into_iter()
        .map(|m| m.id)
        .collect();
    assert_eq!(missing.len(), 3);
    assert_eq!(missing[0], LOADER);

    for id in &missing {
        installer::put_back(&mut w.state, &w.paths, game, &w.game, id).unwrap();
    }
    assert!(installer::missing(&w.state, game, &w.game).unwrap().is_empty());
    assert_eq!(snapshot(&w.game), installed);
}

#[tokio::test]
async fn forgetting_an_uninstalled_game_frees_its_storage() {
    let mut w = World::new().await;
    w.publish_knight().await;
    w.install(KNIGHT).await.0.unwrap();
    let game = "steam:1030300";
    assert!(installer::storage_size(&w.paths, game) > 0);

    fs::remove_dir_all(&w.game).unwrap();
    assert_eq!(installer::forget(&mut w.state, &w.paths, game).unwrap(), 3);
    assert!(w.state.games().unwrap().is_empty());
    assert!(!w.paths.store.join("packages/steam-1030300").exists());
    assert_eq!(installer::storage_size(&w.paths, game), 0);
    assert!(names(&w.paths.store.join("files")).is_empty());
    assert!(!names(&w.paths.downloads).iter().any(|n| n.ends_with(".zip")));
}

#[tokio::test]
async fn refuses_to_install_over_a_manual_bepinex() {
    let mut w = World::new().await;
    w.publish_knight().await;
    fs::create_dir_all(w.game.join("BepInEx/core")).unwrap();
    let before = snapshot(&w.game);

    let (result, _) = w.install(KNIGHT).await;
    assert!(
        matches!(result, Err(InstallError::LoaderPresent)),
        "{result:?}"
    );
    assert_eq!(snapshot(&w.game), before);
}

#[tokio::test]
async fn state_survives_a_restart() {
    let mut w = World::new().await;
    w.publish_knight().await;
    w.install(KNIGHT).await.0.unwrap();
    let db = w.paths.store.parent().unwrap().join("modalia.db");

    let reopened = State::open(&db).unwrap();
    let knight = reopened.get("steam:1030300", KNIGHT).unwrap().unwrap();
    assert!(
        knight
            .deployment
            .files
            .iter()
            .any(|f| f.ends_with("Knight.dll"))
    );
}

#[tokio::test]
async fn updates_are_detected_then_applied_in_one_click() {
    let mut w = World::new().await;
    w.publish_knight().await;
    w.install(KNIGHT).await.0.unwrap();
    let game = "steam:1030300";
    let old_dll = plugin(&w.game, KNIGHT, "Knight.dll");
    assert!(old_dll.is_file());

    // Rien de neuf : aucune mise à jour.
    let installed = w.state.list(game).unwrap();
    assert!(
        installer::available_updates(&w.source, COMMUNITY, &installed)
            .await
            .is_empty()
    );

    // L'auteur publie la 0.7.0, avec un fichier renommé.
    w.server.reset();
    w.publish(LOADER, "1.0.3", &[], loader_zip()).await;
    w.publish(
        PREPATCHER,
        "1.4.0",
        &["silksong_modding-BepInExPack_Silksong-1.0.0"],
        mod_zip("Prepatcher"),
    )
    .await;
    w.publish(
        KNIGHT,
        "0.7.0",
        &[
            "silksong_modding-BepInExPack_Silksong-1.0.3",
            "silksong_modding-SilksongPrepatcher-1.3.0",
        ],
        mod_zip("KnightV7"),
    )
    .await;

    let updates = installer::available_updates(&w.source, COMMUNITY, &installed).await;
    assert_eq!(
        updates,
        [installer::Update {
            mod_id: KNIGHT.into(),
            current: "0.6.7".into(),
            latest: "0.7.0".into(),
        }]
    );

    // Mettre à jour = installer à nouveau : l'ancienne version quitte le jeu.
    let (result, _) = w.install(KNIGHT).await;
    let after = result.unwrap();
    let knight = after.iter().find(|m| m.id == KNIGHT).unwrap();
    assert_eq!(knight.version, "0.7.0");
    assert!(knight.explicit);
    assert!(
        !old_dll.exists(),
        "les fichiers de l'ancienne version doivent avoir été retirés"
    );
    assert!(plugin(&w.game, KNIGHT, "KnightV7.dll").is_file());
    assert_eq!(after.len(), 3, "les dépendances à jour ne bougent pas");

    // L'ancienne version reste dans le stockage, pour y revenir.
    let store = w.paths.store.join("packages/steam-1030300").join(KNIGHT);
    assert_eq!(names(&store), ["0.6.7.json", "0.7.0.json"]);
    let previous: Vec<_> = knight.previous_versions.iter().map(|v| v.version.as_str()).collect();
    assert_eq!(previous, ["0.6.7"]);
    assert!(names(&w.paths.downloads).is_empty(), "archives rangées puis retirées");

    // Retour à la 0.6.7 : rien n'est retéléchargé (le serveur ne sert plus
    // rien), la 0.7.0 devient la version précédente.
    w.server.reset();
    let old = &knight.previous_versions[0].package;
    let back = installer::revert(&mut w.state, &w.paths, game, &w.game, KNIGHT, old).unwrap();
    assert_eq!(back.version, "0.6.7");
    assert!(back.explicit);
    assert!(old_dll.is_file());
    assert!(!plugin(&w.game, KNIGHT, "KnightV7.dll").exists());
    let previous: Vec<_> = back.previous_versions.iter().map(|v| v.version.as_str()).collect();
    assert_eq!(previous, ["0.7.0"]);

    // Et retour à la 0.7.0.
    let new = &back.previous_versions[0].package;
    let again = installer::revert(&mut w.state, &w.paths, game, &w.game, KNIGHT, new).unwrap();
    assert_eq!(again.version, "0.7.0");
    assert!(plugin(&w.game, KNIGHT, "KnightV7.dll").is_file());
    assert!(!old_dll.exists());
    assert!(matches!(
        installer::revert(&mut w.state, &w.paths, game, &w.game, KNIGHT, "steam-1030300/nope/1"),
        Err(InstallError::NoPreviousVersion)
    ));
}


fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .map(|entries| {
            entries
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[tokio::test]
async fn purging_a_removed_mod_frees_its_storage_but_not_the_others() {
    let mut w = World::new().await;
    w.publish_knight().await;
    w.install(KNIGHT).await.0.unwrap();
    let game = "steam:1030300";
    let store = w.paths.store.join("packages/steam-1030300");
    assert!(names(&w.paths.downloads).is_empty(), "archives rangées puis retirées");

    // Réinstallé avant la purge (« Undo ») : rien ne part.
    let removed = installer::uninstall(&mut w.state, &w.paths, game, &w.game, KNIGHT).unwrap();
    installer::restore(&mut w.state, &w.paths, &w.game, removed).unwrap();
    installer::purge(&mut w.state, &w.paths, game, KNIGHT).unwrap();
    assert!(store.join(KNIGHT).is_dir());

    installer::uninstall(&mut w.state, &w.paths, game, &w.game, KNIGHT).unwrap();
    installer::purge(&mut w.state, &w.paths, game, KNIGHT).unwrap();
    assert!(!store.join(KNIGHT).exists());
    assert_eq!(names(&store), [LOADER, PREPATCHER]);
    // Les fichiers des autres mods restent.
    let stored = w.paths.store();
    for m in w.state.list(game).unwrap() {
        for entry in stored.manifest(&m.package).unwrap().files.values() {
            assert!(stored.blob(&entry.hash).is_file());
        }
    }

    // Un identifiant qui sortirait du stockage est ignoré.
    installer::purge(&mut w.state, &w.paths, game, "..").unwrap();
    assert!(w.paths.store.is_dir());
}

#[tokio::test]
async fn files_stay_stored_while_another_game_uses_them() {
    let mut w = World::new().await;
    w.publish_knight().await;
    w.install(KNIGHT).await.0.unwrap();
    // Le même mod dans un autre jeu : son paquet a le même contenu.
    let mut other = w.state.get("steam:1030300", KNIGHT).unwrap().unwrap();
    let packages = w.paths.store.join("packages");
    let theirs = format!("steam-42/{KNIGHT}/0.6.7");
    fs::create_dir_all(packages.join(format!("steam-42/{KNIGHT}"))).unwrap();
    fs::copy(
        packages.join(format!("{}.json", other.package)),
        packages.join(format!("{theirs}.json")),
    )
    .unwrap();
    other.game = "steam:42".into();
    other.package = theirs.clone();
    w.state.save(&other).unwrap();

    installer::uninstall(&mut w.state, &w.paths, "steam:1030300", &w.game, KNIGHT).unwrap();
    installer::purge(&mut w.state, &w.paths, "steam:1030300", KNIGHT).unwrap();
    assert!(!packages.join("steam-1030300").join(KNIGHT).exists());
    let store = w.paths.store();
    for entry in store.manifest(&theirs).unwrap().files.values() {
        assert!(store.blob(&entry.hash).is_file());
    }
}

#[tokio::test]
async fn updating_a_dependency_keeps_it_a_dependency() {
    let mut w = World::new().await;
    w.publish_knight().await;
    w.install(KNIGHT).await.0.unwrap();

    w.server.reset();
    w.publish(LOADER, "1.0.3", &[], loader_zip()).await;
    w.publish(PREPATCHER, "1.5.0", &[], mod_zip("PrepatcherV15"))
        .await;

    w.install(PREPATCHER).await.0.unwrap();
    let prepatcher = w.state.get("steam:1030300", PREPATCHER).unwrap().unwrap();
    assert_eq!(prepatcher.version, "1.5.0");
    assert!(!prepatcher.explicit);
}

#[tokio::test]
async fn prefetch_downloads_without_touching_the_game() {
    let mut w = World::new().await;
    let downloads = w.publish_knight().await;
    let original = snapshot(&w.game);

    // Pendant la partie : tout est téléchargé et extrait, rien n'est posé.
    let installer = Installer {
        source: &w.source,
        catalog: Some(&w.catalog),
        client: &w.client,
        paths: &w.paths,
        cancel: None,
    };
    let target = Target {
        game: "steam:1030300",
        game_dir: &w.game,
        community: COMMUNITY,
    };
    let prepared = installer.prefetch(&[], &target, KNIGHT).await.unwrap();
    assert_eq!(prepared, 3);
    assert_eq!(
        snapshot(&w.game),
        original,
        "le jeu ne doit pas être touché"
    );
    assert!(w.state.list("steam:1030300").unwrap().is_empty());

    // À la fermeture du jeu : l'installation ne retélécharge rien.
    let (result, events) = w.install(KNIGHT).await;
    assert_eq!(result.unwrap().len(), 3);
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Progress::Downloading { .. }))
    );
    for d in downloads {
        d.expect(1).assert_async().await;
    }
}

#[tokio::test]
async fn melonloader_games_install_through_their_rules() {
    const MELON: &str = "LavaGang-MelonLoader";
    const COOL: &str = "Author-CoolMod";
    let mut w = World::new().await;
    w.community = "backpack-hero";
    let before = snapshot(&w.game);
    w.publish(
        MELON,
        "0.6.6",
        &[],
        zip_bytes(&[
            ("version.dll", b"proxy"),
            ("MelonLoader/net6/MelonLoader.dll", b"core"),
            ("manifest.json", b"{}"),
        ]),
    )
    .await;
    w.publish(
        COOL,
        "1.0.0",
        &["LavaGang-MelonLoader-0.6.6"],
        zip_bytes(&[
            ("CoolMod.dll", b"cool"),
            ("Shared.lib.dll", b"lib"),
            ("UserData/cool.cfg", b"cfg"),
            ("manifest.json", b"{}"),
        ]),
    )
    .await;

    let installed = w.install(COOL).await.0.unwrap();
    assert!(
        installed
            .iter()
            .find(|m| m.id == MELON)
            .unwrap()
            .is_loader()
    );
    assert!(w.game.join("version.dll").is_file());
    assert!(w.game.join("MelonLoader/net6/MelonLoader.dll").is_file());
    assert!(w.game.join("Mods/CoolMod.dll").is_file());
    assert!(w.game.join("UserLibs/Shared.lib.dll").is_file());
    // Pas de route `UserData` chez ce jeu : route par défaut, chemin gardé.
    assert!(
        w.game
            .join("UserData/ModManager")
            .join(COOL)
            .join("UserData/cool.cfg")
            .is_file()
    );
    assert!(!w.game.join("manifest.json").exists());

    // Désactiver puis réactiver suit les placements enregistrés.
    let game = "steam:1030300";
    installer::set_enabled(&mut w.state, &w.paths, game, &w.game, COOL, false).unwrap();
    assert!(!w.game.join("Mods/CoolMod.dll").exists());
    installer::set_enabled(&mut w.state, &w.paths, game, &w.game, COOL, true).unwrap();
    assert!(w.game.join("Mods/CoolMod.dll").is_file());

    // Tout retirer rend le jeu intact.
    installer::uninstall(&mut w.state, &w.paths, game, &w.game, COOL).unwrap();
    installer::uninstall(&mut w.state, &w.paths, game, &w.game, MELON).unwrap();
    assert_eq!(snapshot(&w.game), before);
}

#[tokio::test]
async fn a_cancelled_install_leaves_the_game_untouched() {
    use mod_engine::download::Cancel;

    let mut w = World::new().await;
    w.publish_knight().await;
    let before = snapshot(&w.game);
    let cancel = Cancel::default();
    let trigger = cancel.clone();
    let installer = Installer {
        source: &w.source,
        catalog: Some(&w.catalog),
        client: &w.client,
        paths: &w.paths,
        cancel: Some(&cancel),
    };
    let target = Target {
        game: "steam:1030300",
        game_dir: &w.game,
        community: COMMUNITY,
    };
    // Le loader est déjà posé quand l'utilisateur annule, pendant le
    // paquet suivant : tout est retiré.
    let err = installer
        .install(&mut w.state, &target, KNIGHT, |p| {
            if matches!(p, Progress::Installing { index: 1, .. }) {
                trigger.cancel();
            }
        })
        .await
        .unwrap_err();
    assert!(matches!(err, InstallError::Cancelled), "{err:?}");
    assert_eq!(snapshot(&w.game), before);
    assert!(w.state.list("steam:1030300").unwrap().is_empty());
}

#[test]
fn partial_downloads_are_found_per_mod_and_discarded() {
    use mod_engine::installer::{discard_partial_downloads, partial_downloads};

    let tmp = tempfile::tempdir().unwrap();
    let paths = Paths {
        store: tmp.path().join("store"),
        downloads: tmp.path().join("downloads"),
    };
    fs::create_dir_all(&paths.downloads).unwrap();
    for name in [
        "nexus-4198-1.27.5.part",
        "nexus-4198-1.27.5.part.json",
        "nexus-4198-1.27.4.zip",
        "nexus-41980-1.0.part",
        "Author-Mod-1.0.0.part",
    ] {
        fs::write(paths.downloads.join(name), b"x").unwrap();
    }
    let mut found: Vec<_> = partial_downloads(&paths, "nexus-4198")
        .into_iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    found.sort();
    assert_eq!(
        found,
        ["nexus-4198-1.27.5.part", "nexus-4198-1.27.5.part.json"]
    );

    discard_partial_downloads(&paths, "nexus-4198");
    assert!(partial_downloads(&paths, "nexus-4198").is_empty());
    // Les autres mods et l'archive complète ne bougent pas.
    assert!(paths.downloads.join("nexus-4198-1.27.4.zip").is_file());
    assert!(paths.downloads.join("nexus-41980-1.0.part").is_file());
    assert_eq!(partial_downloads(&paths, "Author-Mod").len(), 1);
}

#[tokio::test]
async fn the_old_storage_moves_to_packages_without_downloading() {
    use mod_engine::deploy::Deployment;
    use mod_engine::loaders::Placement;
    use mod_engine::state::InstalledMod;
    let mut w = World::new().await;
    let game = "steam:1030300";

    // Ancien stockage : un dossier extrait par version, lié dans le jeu.
    let legacy = w.paths.store.join("steam-1030300").join(KNIGHT).join("0.6.7");
    fs::create_dir_all(legacy.join("plugins")).unwrap();
    fs::write(legacy.join("plugins/Knight.dll"), "knight").unwrap();
    fs::write(legacy.join("plugins/knight.cfg"), "speed = 1").unwrap();
    let placements = vec![
        Placement {
            source: "plugins/Knight.dll".into(),
            target: format!("BepInEx/plugins/{KNIGHT}/Knight.dll"),
        },
        Placement {
            source: "plugins/knight.cfg".into(),
            target: "BepInEx/config/knight.cfg".into(),
        },
    ];
    let plugins = w.game.join("BepInEx/plugins").join(KNIGHT);
    fs::create_dir_all(&plugins).unwrap();
    fs::create_dir_all(w.game.join("BepInEx/config")).unwrap();
    fs::hard_link(legacy.join("plugins/Knight.dll"), plugins.join("Knight.dll")).unwrap();
    let config = w.game.join("BepInEx/config/knight.cfg");
    fs::hard_link(legacy.join("plugins/knight.cfg"), &config).unwrap();
    w.state
        .save(&InstalledMod {
            game: game.into(),
            id: KNIGHT.into(),
            name: "Knight".into(),
            author: "shownyoung".into(),
            version: "0.6.7".into(),
            icon_url: None,
            loader_root: None,
            enabled: true,
            explicit: true,
            sha256: "abc".into(),
            installed_at: 0,
            package: String::new(),
            legacy_store_dir: legacy.to_string_lossy().into_owned(),
            files: Vec::new(),
            deployment: Deployment {
                files: placements.iter().map(|p| p.target.clone()).collect(),
                created_dirs: Vec::new(),
                copied: 0,
            },
            placements,
            edits: BTreeMap::new(),
            previous_versions: Vec::new(),
        })
        .unwrap();
    // Restes : une vieille version, un mod retiré, une archive en cache.
    let old = w.paths.store.join("steam-1030300").join(KNIGHT).join("0.6.0");
    fs::create_dir_all(&old).unwrap();
    fs::write(old.join("Knight.dll"), "old").unwrap();
    fs::create_dir_all(w.paths.store.join("steam-1030300/Gone-Mod/1.0")).unwrap();
    fs::create_dir_all(&w.paths.downloads).unwrap();
    fs::write(w.paths.downloads.join(format!("{KNIGHT}-0.6.7.zip")), "zip").unwrap();
    fs::write(w.paths.downloads.join(format!("{KNIGHT}-0.7.0.part")), "part").unwrap();

    assert_eq!(installer::migrate_store(&mut w.state, &w.paths).unwrap(), 1);
    let knight = w.state.get(game, KNIGHT).unwrap().unwrap();
    assert_eq!(knight.package, format!("steam-1030300/{KNIGHT}/0.6.7"));
    assert!(knight.legacy_store_dir.is_empty());
    assert_eq!(names(&w.paths.store), ["files", "packages"]);
    assert_eq!(names(&w.paths.downloads), [format!("{KNIGHT}-0.7.0.part")]);
    assert_eq!(fs::read_to_string(plugins.join("Knight.dll")).unwrap(), "knight");

    // La config liée ne partage plus son contenu avec le stockage.
    fs::write(&config, "speed = 9").unwrap();
    let store = w.paths.store();
    let manifest = store.manifest(&knight.package).unwrap();
    let stored = store.blob(&manifest.files["plugins/knight.cfg"].hash);
    assert_eq!(fs::read_to_string(stored).unwrap(), "speed = 1");

    // Le mod se désactive et revient depuis le stockage, config comprise.
    installer::set_enabled(&mut w.state, &w.paths, game, &w.game, KNIGHT, false).unwrap();
    assert!(!plugins.join("Knight.dll").exists());
    installer::set_enabled(&mut w.state, &w.paths, game, &w.game, KNIGHT, true).unwrap();
    assert_eq!(fs::read_to_string(plugins.join("Knight.dll")).unwrap(), "knight");
    assert_eq!(fs::read_to_string(&config).unwrap(), "speed = 9");

    // Une seule fois.
    assert_eq!(installer::migrate_store(&mut w.state, &w.paths).unwrap(), 0);
}

#[test]
fn identical_files_are_stored_once_and_freed_when_unused() {
    use mod_engine::store::Store;
    use std::collections::HashSet;
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::new(tmp.path());
    let pack = |key: &str, files: &[(&str, &str)]| {
        let dir = store.scratch().unwrap();
        for (path, content) in files {
            fs::write(dir.join(path), content).unwrap();
        }
        store.ingest(key, &dir, "").unwrap()
    };
    let one = pack("g/a/1", &[("same.dll", "same"), ("a.dll", "one")]);
    let two = pack("g/a/2", &[("same.dll", "same"), ("a.dll", "two")]);
    assert_eq!(one.files["same.dll"], two.files["same.dll"]);
    let hashes = [&one.files["same.dll"], &one.files["a.dll"], &two.files["a.dll"]].map(|e| e.hash.clone());
    let blobs = |store: &Store| hashes.iter().filter(|h| store.blob(h).is_file()).count();
    assert_eq!(blobs(&store), 3, "le fichier commun n'est rangé qu'une fois");
    assert_eq!(store.size_of("g"), 4 + 3 + 3);

    // Paquets récents gardés même sans mod qui les utilise (délai de grâce).
    let keep: HashSet<String> = HashSet::from(["g/a/2".to_owned()]);
    store.gc(&keep, &HashSet::new()).unwrap();
    assert_eq!(blobs(&store), 3);

    // Retiré : ses fichiers propres partent, le fichier commun reste.
    fs::remove_file(tmp.path().join("packages/g/a/1.json")).unwrap();
    let freed = store.gc(&keep, &HashSet::new()).unwrap();
    assert_eq!(freed.files, 1);
    assert_eq!(blobs(&store), 2);
    assert!(store.has("g/a/2") && !store.has("g/a/1"));
}

#[tokio::test]
async fn removing_a_game_puts_its_folder_back_and_frees_everything() {
    let mut w = World::new().await;
    w.publish_knight().await;
    let original = snapshot(&w.game);
    w.install(KNIGHT).await.0.unwrap();
    let game = "steam:1030300";
    assert_ne!(snapshot(&w.game), original);

    let removed = installer::remove_game(&mut w.state, &w.paths, game, Some(&w.game)).unwrap();
    assert_eq!(removed, 3);
    assert_eq!(snapshot(&w.game), original, "le dossier du jeu revient à l'identique");
    assert!(w.state.list(game).unwrap().is_empty());
    assert_eq!(installer::storage_size(&w.paths, game), 0);
    assert!(names(&w.paths.store.join("files")).is_empty());

    // Dossier déjà parti : seuls la base et le stockage sont nettoyés.
    w.install(KNIGHT).await.0.unwrap();
    fs::remove_dir_all(&w.game).unwrap();
    assert_eq!(installer::remove_game(&mut w.state, &w.paths, game, None).unwrap(), 3);
    assert!(w.state.games().unwrap().is_empty());
    assert!(names(&w.paths.store.join("files")).is_empty());
}

#[tokio::test]
async fn resetting_a_game_clears_mods_from_unalia_and_other_tools() {
    let mut w = World::new().await;
    w.publish_knight().await;
    // `Plugins/` plein, sans MelonLoader : peut être au jeu, il reste.
    fs::create_dir_all(w.game.join("Plugins")).unwrap();
    fs::write(w.game.join("Plugins/native.dll"), "game").unwrap();
    let original = snapshot(&w.game);
    w.install(KNIGHT).await.0.unwrap();
    let game = "steam:1030300";

    // Restes d'autres outils, comme dans un vrai Silksong passé par Vortex.
    fs::create_dir_all(w.game.join("BepInEx/plugins/OldMod")).unwrap();
    fs::write(w.game.join("BepInEx/plugins/OldMod/Old.dll"), "old").unwrap();
    fs::write(w.game.join("version.dll"), "melon").unwrap();
    fs::create_dir(w.game.join("UserData")).unwrap();
    fs::write(w.game.join("vortex.deployment.hollowknightsilksong-bepinex.json"), "{}").unwrap();

    let found = installer::foreign_files(&w.state, game, &w.game, "bepinex").unwrap();
    let found: Vec<(&str, usize)> = found.iter().map(|f| (f.path.as_str(), f.files)).collect();
    assert_eq!(
        found,
        [
            ("BepInEx", 1),
            ("version.dll", 1),
            ("UserData", 0),
            ("vortex.deployment.hollowknightsilksong-bepinex.json", 1),
        ],
        "les fichiers d'Unalia ne comptent pas"
    );

    let trashed = std::cell::RefCell::new(Vec::new());
    let discard = |path: &Path| {
        trashed.borrow_mut().push(path.to_path_buf());
        if path.is_dir() {
            fs::remove_dir_all(path)
        } else {
            fs::remove_file(path)
        }
    };
    let reset = installer::reset(&mut w.state, &w.paths, game, &w.game, "bepinex", discard).unwrap();
    assert_eq!((reset.mods, reset.foreign), (3, 4));
    assert!(reset.left.is_empty());
    assert!(trashed.borrow().contains(&w.game.join("BepInEx")), "le plugin glissé à la main part à la corbeille");
    assert_eq!(snapshot(&w.game), original, "le jeu revient à l'identique");
    assert!(w.state.list(game).unwrap().is_empty());
    assert_eq!(installer::storage_size(&w.paths, game), 0);
}

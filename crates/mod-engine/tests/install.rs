//! Chaîne d'installation sans réseau : resolver, extraction sûre, placement
//! BepInEx et déploiement, sur le faux Silksong de `fixtures/steam`.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use mod_engine::archive::{self, ArchiveError, Limits};
use mod_engine::deploy::{self, DeployError, Source};
use mod_engine::loaders::bepinex::BepInEx;
use mod_engine::loaders::{Loader, PackageKind, Placement};
use mod_engine::resolver::{Installed, ResolveError, resolve};
use mod_engine::sources::{Dependency, Mod, ModVersion, SourceError};
use mod_engine::store::{Manifest, Store};
use zip::write::SimpleFileOptions;

// ---------------------------------------------------------------- outils

const GAME: &str = "../../fixtures/steam/root/steamapps/common/Hollow Knight Silksong";

/// Copie du faux jeu dans un dossier temporaire.
fn game() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("Hollow Knight Silksong");
    copy_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join(GAME), &dir);
    (tmp, dir)
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

/// Photographie complète d'un dossier : chemins, dossiers et contenus.
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

enum Entry<'a> {
    File(&'a str, &'a [u8]),
    Stored(&'a str, &'a [u8]),
    Symlink(&'a str, &'a str),
}

fn zip(dir: &Path, name: &str, entries: &[Entry]) -> PathBuf {
    let path = dir.join(name);
    let mut w = zip::ZipWriter::new(fs::File::create(&path).unwrap());
    let deflate = SimpleFileOptions::default();
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for entry in entries {
        match entry {
            Entry::File(name, data) => {
                w.start_file(*name, deflate).unwrap();
                w.write_all(data).unwrap();
            }
            Entry::Stored(name, data) => {
                w.start_file(*name, stored).unwrap();
                w.write_all(data).unwrap();
            }
            Entry::Symlink(name, target) => w.add_symlink(*name, *target, deflate).unwrap(),
        }
    }
    w.finish().unwrap();
    path
}

fn loader_zip(dir: &Path) -> PathBuf {
    zip(
        dir,
        "bepinex.zip",
        &[
            Entry::File("BepInExPack/winhttp.dll", b"winhttp"),
            Entry::File(
                "BepInExPack/doorstop_config.ini",
                b"[General]\nenabled=true",
            ),
            Entry::File("BepInExPack/BepInEx/core/BepInEx.dll", b"core"),
            Entry::File("manifest.json", b"{}"),
            Entry::File("icon.png", b"png"),
        ],
    )
}

fn mod_zip(dir: &Path) -> PathBuf {
    zip(
        dir,
        "skip-intro.zip",
        &[
            Entry::File("plugins\\SkipIntro.dll", b"skip"),
            Entry::File("config/com.bigfootmech.skipintro.cfg", b"enabled = true"),
            Entry::File("manifest.json", b"{}"),
            Entry::File("README.md", b"# Skip intro"),
        ],
    )
}

/// Paquet rangé dans un stockage de test.
struct Stored {
    store: Store,
    manifest: Manifest,
    edits: BTreeMap<String, String>,
}

impl Stored {
    fn source(&self) -> Source<'_> {
        Source {
            store: &self.store,
            manifest: &self.manifest,
            edits: &self.edits,
        }
    }

    /// Contenu rangé d'un fichier du paquet.
    fn blob(&self, path: &str) -> PathBuf {
        self.store.blob(&self.manifest.files[path].hash)
    }
}

/// Extrait un paquet dans le stockage et calcule où vont ses fichiers.
fn stage(store: &Path, archive: &Path, id: &str, kind: &PackageKind) -> (Stored, Vec<Placement>) {
    let store = Store::new(store);
    let dir = store.scratch().unwrap().join(id);
    let files = archive::extract(archive, &dir, Limits::default()).unwrap();
    let manifest = store.ingest(id, &dir, "").unwrap();
    assert_eq!(manifest.files.keys().cloned().collect::<Vec<_>>(), sorted(files.clone()));
    let placements = BepInEx.place(id, kind, &files);
    let stored = Stored {
        store,
        manifest,
        edits: BTreeMap::new(),
    };
    (stored, placements)
}

/// Range des fichiers donnés tels quels dans le stockage.
fn pack(store: &Path, key: &str, files: &[(&str, &str)]) -> Stored {
    let store = Store::new(store);
    let dir = store.scratch().unwrap();
    for (path, content) in files {
        let file = dir.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, content).unwrap();
    }
    let manifest = store.ingest(key, &dir, "").unwrap();
    Stored {
        store,
        manifest,
        edits: BTreeMap::new(),
    }
}

fn sorted(mut files: Vec<String>) -> Vec<String> {
    files.sort();
    files
}

fn loader_kind() -> PackageKind {
    PackageKind::Loader {
        root_folder: "BepInExPack".into(),
    }
}

// ---------------------------------------------------------------- archive

#[test]
fn extracts_and_normalizes_windows_paths() {
    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("out");
    let files = archive::extract(&mod_zip(tmp.path()), &dest, Limits::default()).unwrap();

    assert_eq!(
        files,
        [
            "README.md",
            "config/com.bigfootmech.skipintro.cfg",
            "manifest.json",
            "plugins/SkipIntro.dll"
        ]
    );
    assert_eq!(
        fs::read(dest.join("plugins/SkipIntro.dll")).unwrap(),
        b"skip"
    );
}

#[test]
fn refuses_zip_slip_and_leaves_nothing_behind() {
    let tmp = tempfile::tempdir().unwrap();
    for (i, evil) in [
        "../evil.txt",
        "plugins/../../evil.txt",
        "..\\evil.txt",
        "C:/evil.txt",
    ]
    .into_iter()
    .enumerate()
    {
        let archive = zip(
            tmp.path(),
            &format!("slip{i}.zip"),
            &[Entry::File("ok.txt", b"ok"), Entry::File(evil, b"pwned")],
        );
        let dest = tmp.path().join(format!("out{i}"));
        let err = archive::extract(&archive, &dest, Limits::default()).unwrap_err();

        assert!(
            matches!(err, ArchiveError::UnsafePath(_)),
            "{evil}: {err:?}"
        );
        assert!(!dest.exists(), "{evil} : extraction partielle laissée");
        assert!(!tmp.path().join("evil.txt").exists());
    }
}

#[test]
fn refuses_links() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = zip(
        tmp.path(),
        "link.zip",
        &[Entry::Symlink("plugins/link", "/etc/passwd")],
    );
    let err = archive::extract(&archive, &tmp.path().join("out"), Limits::default()).unwrap_err();
    assert!(matches!(err, ArchiveError::Link(_)), "{err:?}");
}

#[test]
fn refuses_zip_bombs() {
    let tmp = tempfile::tempdir().unwrap();
    let limits = Limits {
        max_total_bytes: 1024 * 1024,
        max_entries: 3,
        max_ratio: 50,
    };

    // 4 Mo de zéros compressés en quelques Ko : ratio absurde.
    let zeros = vec![0u8; 4 * 1024 * 1024];
    let bomb = zip(tmp.path(), "bomb.zip", &[Entry::File("zeros.bin", &zeros)]);
    let relaxed = Limits {
        max_total_bytes: u64::MAX,
        ..limits
    };
    let err = archive::extract(&bomb, &tmp.path().join("a"), relaxed).unwrap_err();
    assert!(matches!(err, ArchiveError::SuspiciousRatio(_)), "{err:?}");

    // Même non compressé, 2 Mo dépassent la limite de 1 Mo.
    let big = vec![7u8; 2 * 1024 * 1024];
    let large = zip(tmp.path(), "large.zip", &[Entry::Stored("big.bin", &big)]);
    let err = archive::extract(&large, &tmp.path().join("b"), limits).unwrap_err();
    assert!(matches!(err, ArchiveError::TooLarge), "{err:?}");

    let many = zip(
        tmp.path(),
        "many.zip",
        &[
            Entry::File("1", b""),
            Entry::File("2", b""),
            Entry::File("3", b""),
            Entry::File("4", b""),
        ],
    );
    let err = archive::extract(&many, &tmp.path().join("c"), limits).unwrap_err();
    assert!(matches!(err, ArchiveError::TooManyEntries), "{err:?}");
}

#[test]
fn extracts_7z_archives() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    fs::create_dir_all(src.join("archive/pc/mod")).unwrap();
    fs::write(src.join("archive/pc/mod/cool.archive"), b"archive").unwrap();
    fs::write(src.join("readme.txt"), b"hi").unwrap();
    // Extension trompeuse : le format se lit dans le fichier.
    let seven = tmp.path().join("cool.zip");
    sevenz_rust2::compress_to_path(&src, &seven).unwrap();

    let files = archive::extract(&seven, &tmp.path().join("out"), Limits::default()).unwrap();
    assert_eq!(files, ["archive/pc/mod/cool.archive", "readme.txt"]);
    assert_eq!(
        fs::read(tmp.path().join("out/archive/pc/mod/cool.archive")).unwrap(),
        b"archive"
    );
}

fn rar_fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/rar")
        .join(name)
}

#[test]
fn extracts_rar_archives() {
    let tmp = tempfile::tempdir().unwrap();
    // Extension trompeuse : le format se lit dans le fichier.
    let rar = tmp.path().join("mod.zip");
    fs::copy(rar_fixture("version.rar"), &rar).unwrap();

    let mut progress = Vec::new();
    let files = archive::extract_with_progress(
        &rar,
        &tmp.path().join("out"),
        Limits::default(),
        |done, total| progress.push((done, total)),
    )
    .unwrap();
    assert_eq!(files, ["VERSION"]);
    assert_eq!(
        fs::read(tmp.path().join("out/VERSION")).unwrap(),
        b"unrar-0.4.0"
    );
    assert_eq!(progress, [(11, 11)]);
}

#[test]
fn refuses_rar_archives_it_cannot_open_alone() {
    let tmp = tempfile::tempdir().unwrap();
    for (name, kind) in [
        ("crypted.rar", "Password-protected"),
        ("archive.part1.rar", "Multi-part RAR"),
    ] {
        let out = tmp.path().join(name).with_extension("out");
        let err = archive::extract(&rar_fixture(name), &out, Limits::default()).unwrap_err();
        assert!(
            matches!(err, ArchiveError::Unsupported(k) if k == kind),
            "{name}: {err:?}"
        );
        assert!(!out.exists());
        assert!(!out.with_extension("partial").exists());
    }
}

#[test]
fn refuses_rar_archives_over_the_limits() {
    let tmp = tempfile::tempdir().unwrap();
    let limits = Limits {
        max_total_bytes: 10,
        ..Limits::default()
    };
    let err =
        archive::extract(&rar_fixture("version.rar"), &tmp.path().join("out"), limits).unwrap_err();
    assert!(matches!(err, ArchiveError::TooLarge), "{err:?}");
    assert!(!tmp.path().join("out").exists());

    let rar = tmp.path().join("bad.rar");
    fs::write(&rar, b"Rar!\x1a\x07\x00 pas vraiment un rar").unwrap();
    let err = archive::extract(&rar, &tmp.path().join("bad"), Limits::default()).unwrap_err();
    assert!(matches!(err, ArchiveError::Rar(_)), "{err:?}");
}

#[test]
fn refuses_corrupt_archives() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("bad.zip");
    fs::write(&path, b"PK\x03\x04 pas vraiment un zip").unwrap();
    let err = archive::extract(&path, &tmp.path().join("out"), Limits::default()).unwrap_err();
    assert!(matches!(err, ArchiveError::Corrupt(_)), "{err:?}");
}

// ---------------------------------------------------------------- deploy

#[test]
fn install_then_uninstall_restores_the_game_exactly() {
    let (tmp, game) = game();
    let store = tmp.path().join("store");
    let original = snapshot(&game);

    let (loader_dir, loader_files) = stage(
        &store,
        &loader_zip(tmp.path()),
        "silksong_modding-BepInExPack_Silksong",
        &loader_kind(),
    );
    let loader = deploy::deploy(loader_dir.source(), &game, &loader_files).unwrap();
    let (mod_dir, mod_files) = stage(
        &store,
        &mod_zip(tmp.path()),
        "Bigfootmech-Silksong_Skip_Intro",
        &PackageKind::Mod,
    );
    let skip_intro = deploy::deploy(mod_dir.source(), &game, &mod_files).unwrap();

    assert!(game.join("winhttp.dll").is_file());
    assert!(game.join("BepInEx/core/BepInEx.dll").is_file());
    assert!(
        game.join("BepInEx/plugins/Bigfootmech-Silksong_Skip_Intro/SkipIntro.dll")
            .is_file()
    );
    assert!(
        game.join("BepInEx/config/com.bigfootmech.skipintro.cfg")
            .is_file()
    );
    assert!(
        !game.join("manifest.json").exists(),
        "métadonnées installées"
    );
    assert_eq!(
        loader.copied + skip_intro.copied,
        0,
        "même disque : liens physiques attendus"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let stored = fs::metadata(mod_dir.blob("plugins/SkipIntro.dll")).unwrap();
        let linked = fs::metadata(
            game.join("BepInEx/plugins/Bigfootmech-Silksong_Skip_Intro/SkipIntro.dll"),
        )
        .unwrap();
        assert_eq!(
            stored.ino(),
            linked.ino(),
            "le jeu doit voir un lien, pas une copie"
        );
    }

    // Le jeu tourne : BepInEx écrit ses journaux et son cache.
    fs::write(game.join("BepInEx/LogOutput.log"), "log").unwrap();
    fs::create_dir_all(game.join("BepInEx/cache")).unwrap();
    fs::write(game.join("BepInEx/cache/chainloader.dat"), "cache").unwrap();

    assert!(deploy::undeploy(&game, &skip_intro, false).is_empty());
    assert!(deploy::undeploy(&game, &loader, true).is_empty());
    assert_eq!(
        snapshot(&game),
        original,
        "le dossier du jeu doit revenir à l'identique"
    );
}

#[test]
fn never_overwrites_an_existing_file() {
    let (tmp, game) = game();
    // BepInEx installé à la main auparavant : winhttp.dll est déjà là.
    fs::write(game.join("winhttp.dll"), "manual install").unwrap();
    let before = snapshot(&game);

    let (dir, files) = stage(
        &tmp.path().join("store"),
        &loader_zip(tmp.path()),
        "silksong_modding-BepInExPack_Silksong",
        &loader_kind(),
    );
    let err = deploy::deploy(dir.source(), &game, &files).unwrap_err();

    assert!(
        matches!(&err, DeployError::Conflict(f) if f == "winhttp.dll"),
        "{err:?}"
    );
    assert_eq!(snapshot(&game), before, "rien ne doit avoir été écrit");
    assert!(!BepInEx.installed_manually(&game));
}

#[test]
fn a_failure_halfway_rolls_everything_back() {
    let (tmp, game) = game();
    let store = pack(&tmp.path().join("store"), "test", &[("a.txt", "a"), ("b.txt", "b")]);
    let before = snapshot(&game);

    // Le 2e fichier vise un dossier qui est en fait un fichier du jeu :
    // l'écriture échoue après que le 1er a été posé.
    let placements = vec![
        Placement {
            source: "a.txt".into(),
            target: "BepInEx/plugins/Test/a.txt".into(),
        },
        Placement {
            source: "b.txt".into(),
            target: "UnityPlayer.dll/b.txt".into(),
        },
    ];
    let err = deploy::deploy(store.source(), &game, &placements).unwrap_err();

    assert!(matches!(err, DeployError::Io { .. }), "{err:?}");
    assert_eq!(
        snapshot(&game),
        before,
        "le premier fichier doit avoir été retiré"
    );
}

#[test]
fn disable_and_enable_are_undeploy_and_redeploy() {
    let (tmp, game) = game();
    let store = tmp.path().join("store");
    let (dir, files) = stage(
        &store,
        &mod_zip(tmp.path()),
        "Bigfootmech-Silksong_Skip_Intro",
        &PackageKind::Mod,
    );
    let plugin = game.join("BepInEx/plugins/Bigfootmech-Silksong_Skip_Intro/SkipIntro.dll");

    let deployed = deploy::deploy(dir.source(), &game, &files).unwrap();
    deploy::undeploy(&game, &deployed, false);
    assert!(!plugin.exists());
    assert!(
        dir.blob("plugins/SkipIntro.dll").is_file(),
        "le stockage garde le mod désactivé"
    );

    deploy::deploy(dir.source(), &game, &files).unwrap();
    assert!(plugin.is_file());
}

#[test]
fn configs_are_copied_and_their_edits_kept() {
    let (tmp, game) = game();
    let (mut stored, files) = stage(
        &tmp.path().join("store"),
        &mod_zip(tmp.path()),
        "Bigfootmech-Silksong_Skip_Intro",
        &PackageKind::Mod,
    );
    let config = game.join("BepInEx/config/com.bigfootmech.skipintro.cfg");
    let plugin = game.join("BepInEx/plugins/Bigfootmech-Silksong_Skip_Intro/SkipIntro.dll");
    let original = stored.blob("config/com.bigfootmech.skipintro.cfg");

    let deployed = deploy::deploy(stored.source(), &game, &files).unwrap();
    // BepInEx réécrit sa config sur place : le contenu rangé ne bouge pas.
    fs::write(&config, "enabled = false").unwrap();
    assert_eq!(fs::read_to_string(&original).unwrap(), "enabled = true");
    // Le reste est lié : le jeu voit le contenu rangé lui-même.
    fs::write(&plugin, "patched").unwrap();
    assert_eq!(
        fs::read_to_string(stored.blob("plugins/SkipIntro.dll")).unwrap(),
        "patched"
    );

    // Désactivé puis réactivé : la config modifiée revient.
    stored.edits = deploy::capture_edits(stored.source(), &game, &files).unwrap();
    assert_eq!(stored.edits.len(), 1);
    deploy::undeploy(&game, &deployed, false);
    assert!(!config.exists());
    deploy::deploy(stored.source(), &game, &files).unwrap();
    assert_eq!(fs::read_to_string(&config).unwrap(), "enabled = false");

    // Remise à l'original : plus rien à garder.
    fs::write(&config, "enabled = true").unwrap();
    let edits = deploy::capture_edits(stored.source(), &game, &files).unwrap();
    assert!(edits.is_empty());
}

#[test]
fn refuses_unsafe_targets() {
    let (tmp, game) = game();
    let stored = pack(&tmp.path().join("store"), "test", &[("x", "x")]);
    let err = deploy::deploy(
        stored.source(),
        &game,
        &[Placement {
            source: "x".into(),
            target: "../outside.dll".into(),
        }],
    )
    .unwrap_err();
    assert!(matches!(err, DeployError::UnsafeTarget(_)), "{err:?}");
    assert!(!tmp.path().join("outside.dll").exists());
}

// ---------------------------------------------------------------- resolver

fn package(id: &str, version: &str, deps: &[(&str, &str)]) -> Mod {
    Mod {
        id: id.into(),
        source: "thunderstore",
        name: id.split_once('-').unwrap().1.into(),
        author: id.split_once('-').unwrap().0.into(),
        summary: String::new(),
        images: vec![],
        categories: vec![],
        downloads: None,
        updated_at: String::new(),
        source_url: String::new(),
        website_url: None,
        donation_url: None,
        deprecated: false,
        versions: vec![ModVersion {
            number: version.into(),
            published_at: String::new(),
            download_url: format!("https://thunderstore.io/package/download/{id}/{version}/"),
            sha256: None,
            size: None,
            dependencies: deps
                .iter()
                .map(|(id, v)| Dependency {
                    id: (*id).into(),
                    min_version: (*v).into(),
                })
                .collect(),
        }],
    }
}

/// Le vrai arbre de KnightInSilksong, avec les deux loaders concurrents.
fn catalog() -> HashMap<String, Mod> {
    [
        package(
            "shownyoung-KnightInSilksong",
            "0.6.7",
            &[
                ("silksong_modding-BepInExPack_Silksong", "1.0.3"),
                ("silksong_modding-SilksongPrepatcher", "1.3.0"),
                ("silksong_modding-I18N", "1.0.3"),
            ],
        ),
        package("silksong_modding-BepInExPack_Silksong", "1.0.3", &[]),
        package("BepInEx-BepInExPack_Silksong", "5.4.2304", &[]),
        package(
            "silksong_modding-SilksongPrepatcher",
            "1.4.0",
            &[("silksong_modding-BepInExPack_Silksong", "1.0.0")],
        ),
        package(
            "silksong_modding-I18N",
            "1.1.1",
            &[("BepInEx-BepInExPack_Silksong", "5.4.2304")],
        ),
        package("a-Loop", "1.0.0", &[("b-Loop", "1.0.0")]),
        package("b-Loop", "1.0.0", &[("a-Loop", "1.0.0")]),
        package("x-Needy", "1.0.0", &[("silksong_modding-I18N", "9.0.0")]),
    ]
    .into_iter()
    .map(|m| (m.id.clone(), m))
    .collect()
}

async fn plan(root: &str, installed: &Installed) -> Result<Vec<String>, ResolveError> {
    let catalog = catalog();
    let plan = resolve(
        root,
        installed,
        |id| {
            let found = catalog.get(&id).cloned();
            async move {
                found.ok_or(SourceError::NotFound {
                    source_name: "test",
                })
            }
        },
        |id| id.contains("BepInExPack"),
    )
    .await?;
    Ok(plan.steps.into_iter().map(|s| s.id).collect())
}

#[tokio::test]
async fn dependencies_come_first_and_only_one_loader() {
    let steps = plan("shownyoung-KnightInSilksong", &Installed::default())
        .await
        .unwrap();
    assert_eq!(
        steps,
        [
            "silksong_modding-BepInExPack_Silksong",
            "silksong_modding-SilksongPrepatcher",
            "silksong_modding-I18N",
            "shownyoung-KnightInSilksong",
        ],
        "I18N demande l'autre BepInExPack : le loader déjà choisi suffit"
    );
}

#[tokio::test]
async fn already_installed_packages_are_skipped() {
    let installed = Installed {
        versions: HashMap::from([
            (
                "silksong_modding-BepInExPack_Silksong".into(),
                "1.0.3".into(),
            ),
            ("silksong_modding-I18N".into(), "1.1.1".into()),
        ]),
        loader: Some("silksong_modding-BepInExPack_Silksong".into()),
    };
    let steps = plan("shownyoung-KnightInSilksong", &installed)
        .await
        .unwrap();
    assert_eq!(
        steps,
        [
            "silksong_modding-SilksongPrepatcher",
            "shownyoung-KnightInSilksong"
        ]
    );

    // Le mod lui-même déjà à jour : plan vide.
    let up_to_date = Installed {
        versions: HashMap::from([("silksong_modding-I18N".into(), "1.1.1".into())]),
        loader: Some("BepInEx-BepInExPack_Silksong".into()),
    };
    assert!(
        plan("silksong_modding-I18N", &up_to_date)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn outdated_dependencies_are_updated() {
    let installed = Installed {
        versions: HashMap::from([("silksong_modding-SilksongPrepatcher".into(), "1.0.0".into())]),
        loader: None,
    };
    let catalog = catalog();
    let plan = resolve(
        "shownyoung-KnightInSilksong",
        &installed,
        |id| {
            let found = catalog.get(&id).cloned();
            async move {
                found.ok_or(SourceError::NotFound {
                    source_name: "test",
                })
            }
        },
        |id| id.contains("BepInExPack"),
    )
    .await
    .unwrap();
    let prepatcher = plan
        .steps
        .iter()
        .find(|s| s.id == "silksong_modding-SilksongPrepatcher")
        .unwrap();
    assert_eq!(prepatcher.replaces.as_deref(), Some("1.0.0"));
    assert_eq!(prepatcher.version.number, "1.4.0");
    assert_eq!(plan.dependency_count(), 3);
}

#[tokio::test]
async fn cycles_missing_and_impossible_versions_are_errors() {
    let none = Installed::default();
    assert!(matches!(
        plan("a-Loop", &none).await,
        Err(ResolveError::Cycle(_))
    ));
    assert!(matches!(
        plan("ghost-Mod", &none).await,
        Err(ResolveError::Source { .. })
    ));
    assert!(matches!(
        plan("x-Needy", &none).await,
        Err(ResolveError::TooOld { .. })
    ));
}

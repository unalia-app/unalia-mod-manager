//! Tests de la recherche d'images dans le cache de `fixtures/steam/root`.

use std::path::{Path, PathBuf};

use mod_engine::artwork::{ArtworkKind, find_artwork, game_accent};

fn steam_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/steam/root")
}

fn relative(path: Option<PathBuf>) -> Option<String> {
    let cache = steam_root().join("appcache/librarycache");
    path.map(|p| {
        p.strip_prefix(&cache)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/")
    })
}

#[test]
fn finds_artwork_in_the_current_layout() {
    let root = steam_root();
    assert_eq!(
        relative(find_artwork(&root, 1030300, ArtworkKind::Cover)).as_deref(),
        Some("1030300/library_600x900.jpg")
    );
    assert_eq!(
        relative(find_artwork(&root, 1030300, ArtworkKind::Hero)).as_deref(),
        Some("1030300/library_hero.jpg")
    );
}

#[test]
fn finds_artwork_in_hashed_subfolders() {
    assert_eq!(
        relative(find_artwork(&steam_root(), 2000001, ArtworkKind::Cover)).as_deref(),
        Some("2000001/5d41402abc4b2a76b9719d911017c592/library_600x900.jpg")
    );
}

#[test]
fn finds_artwork_in_the_legacy_layout() {
    assert_eq!(
        relative(find_artwork(&steam_root(), 228980, ArtworkKind::Header)).as_deref(),
        Some("228980_header.jpg")
    );
}

#[test]
fn missing_artwork_is_none() {
    let root = steam_root();
    assert_eq!(find_artwork(&root, 2000002, ArtworkKind::Cover), None);
    assert_eq!(find_artwork(&root, 1030300, ArtworkKind::Logo), None);
    assert_eq!(
        find_artwork(Path::new("/nonexistent"), 1030300, ArtworkKind::Cover),
        None
    );
}

#[test]
fn parses_kinds_from_urls() {
    assert_eq!("cover".parse(), Ok(ArtworkKind::Cover));
    assert_eq!("hero-blur".parse(), Ok(ArtworkKind::HeroBlur));
    assert!("../secret".parse::<ArtworkKind>().is_err());
}

#[test]
fn accent_comes_from_the_cover_and_greys_give_none() {
    let root = steam_root();

    // Jaquette sombre à halo cramoisi : teinte rouge.
    let silksong = game_accent(&root, 1030300).expect("accent attendu");
    assert!(
        silksong.h < 40.0 || silksong.h > 340.0,
        "teinte rouge attendue : {silksong:?}"
    );

    // Jaquette sarcelle trouvée dans un sous-dossier.
    let unreal = game_accent(&root, 2000001).expect("accent attendu");
    assert!((160.0..220.0).contains(&unreal.h), "{unreal:?}");

    // Capsule grise : pas d'accent inventé.
    assert_eq!(game_accent(&root, 228980), None);
    // Aucune image.
    assert_eq!(game_accent(&root, 2000002), None);
}

#[test]
fn trims_the_transparent_margins_of_a_logo() {
    use image::{Rgba, RgbaImage};
    use mod_engine::artwork::trim_logo;

    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("logo.png");
    let target = dir.path().join("trimmed.png");
    let mut logo = RgbaImage::new(64, 36);
    for x in 20..30 {
        for y in 10..14 {
            logo.put_pixel(x, y, Rgba([255, 230, 0, 255]));
        }
    }
    logo.save(&source).unwrap();
    assert!(trim_logo(&source, &target).unwrap());
    assert_eq!(image::open(&target).unwrap().into_rgba8().dimensions(), (10, 4));

    // Déjà sans marge, ou entièrement vide : rien à écrire.
    let full = RgbaImage::from_pixel(8, 8, Rgba([0, 0, 0, 255]));
    full.save(&source).unwrap();
    assert!(!trim_logo(&source, &target).unwrap());
    RgbaImage::new(8, 8).save(&source).unwrap();
    assert!(!trim_logo(&source, &target).unwrap());
}

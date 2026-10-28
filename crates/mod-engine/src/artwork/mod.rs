//! Game images, read from Steam's local cache (`appcache/librarycache`).
//!
//! No official asset is embedded or downloaded: we only use what
//! Steam already cached on the player's machine.

mod accent;

use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

pub use accent::{Accent, accent_from_image, accent_from_path};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArtworkKind {
    /// Jaquette verticale 600×900.
    Cover,
    /// Wide banner at the top of the game page in Steam.
    Hero,
    /// Same banner, already blurred by Steam.
    HeroBlur,
    /// Capsule horizontale 460×215.
    Header,
    /// Cut-out logo (PNG).
    Logo,
}

impl ArtworkKind {
    /// Possible file names, from best to worst fit.
    fn file_names(self) -> &'static [&'static str] {
        match self {
            Self::Cover => &[
                "library_600x900.jpg",
                "library_600x900_2x.jpg",
                "library_capsule.jpg",
            ],
            Self::Hero => &["library_hero.jpg"],
            Self::HeroBlur => &["library_hero_blur.jpg"],
            Self::Header => &["header.jpg"],
            Self::Logo => &["logo.png"],
        }
    }
}

impl FromStr for ArtworkKind {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, ()> {
        match s {
            "cover" => Ok(Self::Cover),
            "hero" => Ok(Self::Hero),
            "hero-blur" => Ok(Self::HeroBlur),
            "header" => Ok(Self::Header),
            "logo" => Ok(Self::Logo),
            _ => Err(()),
        }
    }
}

/// Looks for a game image in Steam's cache. Handles both known layouts:
///
/// - current: `librarycache/<appid>/<name>`, sometimes in a subfolder named
///   after a hash (`librarycache/<appid>/<hash>/<name>`);
/// - old: `librarycache/<appid>_<name>`.
pub fn find_artwork(steam_root: &Path, app_id: u32, kind: ArtworkKind) -> Option<PathBuf> {
    let cache = steam_root.join("appcache").join("librarycache");
    let app_dir = cache.join(app_id.to_string());

    for name in kind.file_names() {
        let direct = app_dir.join(name);
        if direct.is_file() {
            return Some(direct);
        }
        if let Some(nested) = newest_in_subdirs(&app_dir, name) {
            return Some(nested);
        }
        let legacy = cache.join(format!("{app_id}_{name}"));
        if legacy.is_file() {
            return Some(legacy);
        }
    }
    None
}

/// Most recent image among the subfolders: Steam sometimes keeps several
/// versions of the same artwork there.
fn newest_in_subdirs(dir: &Path, name: &str) -> Option<PathBuf> {
    fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path().join(name))
        .filter_map(|p| {
            let modified = fs::metadata(&p).ok()?.modified().ok()?;
            Some((modified, p))
        })
        .max()
        .map(|(_, p)| p)
}

/// The game's accent color, computed from the first available image among
/// the cover, the capsule and the banner.
pub fn game_accent(steam_root: &Path, app_id: u32) -> Option<Accent> {
    [ArtworkKind::Cover, ArtworkKind::Header, ArtworkKind::Hero]
        .into_iter()
        .filter_map(|kind| find_artwork(steam_root, app_id, kind))
        .find_map(|path| accent_from_path(&path))
}

/// Copy of a logo without its transparent margins: Steam logos sit on a
/// fixed canvas (640×360), sometimes with a lot of empty space around.
/// `false` if there is nothing to trim (the original logo is fine as is).
pub fn trim_logo(source: &Path, target: &Path) -> image::ImageResult<bool> {
    let image = image::open(source)?.into_rgba8();
    let (width, height) = image.dimensions();
    let (mut left, mut top, mut right, mut bottom) = (width, height, 0, 0);
    for (x, y, pixel) in image.enumerate_pixels() {
        // A near-zero alpha counts as empty (very faint shadows).
        if pixel[3] > 8 {
            left = left.min(x);
            top = top.min(y);
            right = right.max(x);
            bottom = bottom.max(y);
        }
    }
    if left > right || (left == 0 && top == 0 && right == width - 1 && bottom == height - 1) {
        return Ok(false);
    }
    let cropped = image::imageops::crop_imm(&image, left, top, right - left + 1, bottom - top + 1);
    cropped.to_image().save_with_format(target, image::ImageFormat::Png)?;
    Ok(true)
}

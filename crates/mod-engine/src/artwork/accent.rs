//! Couleur d'accent extraite d'une image, en OKLCH (SPEC §4).
//!
//! 1. On réduit l'image et on classe ses pixels colorés par teinte.
//! 2. La teinte dominante (pondérée par la saturation) est retenue.
//! 3. On impose une luminosité et une saturation lisibles sur le fond sombre
//!    de Modalia, puis on ramène la couleur dans le gamut sRGB.
//!
//! Une image presque grise ne donne pas d'accent : l'interface garde alors
//! l'accent neutre plutôt que d'inventer une couleur.

use std::f32::consts::TAU;
use std::path::Path;

use image::{DynamicImage, GenericImageView};

/// Fond de Modalia (`--color-bg`, #0d0d0f), pour le calcul de contraste.
const BACKGROUND: [f32; 3] = [13.0 / 255.0, 13.0 / 255.0, 15.0 / 255.0];
/// Contraste minimal d'un texte en couleur d'accent sur le fond (WCAG AA).
const MIN_CONTRAST: f32 = 4.5;

const TARGET_LIGHTNESS: f32 = 0.78;
const MIN_CHROMA: f32 = 0.07;
const MAX_CHROMA: f32 = 0.17;

const HUE_BINS: usize = 24;
/// Part minimale de pixels colorés dans la teinte retenue.
const MIN_COVERAGE: f32 = 0.03;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Accent {
    /// Luminosité perçue, 0 à 1.
    pub l: f32,
    /// Saturation (chroma OKLCH).
    pub c: f32,
    /// Teinte en degrés, 0 à 360.
    pub h: f32,
}

impl Accent {
    /// Valeur CSS, ex. `oklch(0.780 0.140 264.1)`.
    pub fn css(&self) -> String {
        format!("oklch({:.3} {:.3} {:.1})", self.l, self.c, self.h)
    }

    pub fn to_srgb(self) -> [f32; 3] {
        oklab_to_srgb(
            self.l,
            self.c * self.h.to_radians().cos(),
            self.c * self.h.to_radians().sin(),
        )
    }

    /// Contraste WCAG d'un texte de cette couleur sur le fond de Modalia.
    pub fn contrast_on_background(self) -> f32 {
        contrast(
            relative_luminance(self.to_srgb()),
            relative_luminance(BACKGROUND),
        )
    }
}

pub fn accent_from_path(path: &Path) -> Option<Accent> {
    accent_from_image(&image::open(path).ok()?)
}

pub fn accent_from_image(image: &DynamicImage) -> Option<Accent> {
    // 64 px de côté suffisent pour la couleur dominante et gardent le calcul
    // sous la milliseconde.
    let small = image.thumbnail(64, 64);

    #[derive(Default, Clone, Copy)]
    struct Bin {
        weight: f32,
        count: u32,
        a: f32,
        b: f32,
        chroma: f32,
    }

    let mut bins = [Bin::default(); HUE_BINS];
    let mut total = 0u32;
    for (_, _, pixel) in small.pixels() {
        let [r, g, b, alpha] = pixel.0;
        if alpha < 128 {
            continue;
        }
        total += 1;
        let (l, a, b) = srgb_to_oklab([r, g, b].map(|v| f32::from(v) / 255.0));
        let chroma = a.hypot(b);
        // Ni les noirs ni les gris (blancs compris) ne portent une teinte fiable.
        if l < 0.2 || chroma < 0.04 {
            continue;
        }
        let hue = b.atan2(a).rem_euclid(TAU);
        let bin = &mut bins[((hue / TAU) * HUE_BINS as f32) as usize % HUE_BINS];
        // Les couleurs vives et de luminosité moyenne comptent davantage.
        let weight = chroma * (1.0 - (l - 0.6).abs());
        bin.weight += weight;
        bin.count += 1;
        bin.a += a * weight;
        bin.b += b * weight;
        bin.chroma += chroma * weight;
    }

    // On regroupe chaque case avec ses voisines pour qu'une teinte à cheval
    // sur deux cases ne soit pas pénalisée.
    let merged = (0..HUE_BINS)
        .map(|i| {
            let mut m = Bin::default();
            for j in [HUE_BINS - 1, 0, 1] {
                let n = bins[(i + j) % HUE_BINS];
                m.weight += n.weight;
                m.count += n.count;
                m.a += n.a;
                m.b += n.b;
                m.chroma += n.chroma;
            }
            m
        })
        .max_by(|x, y| x.weight.total_cmp(&y.weight))?;

    if merged.count == 0 || (merged.count as f32) < MIN_COVERAGE * total as f32 {
        return None;
    }

    let hue = merged.b.atan2(merged.a).to_degrees().rem_euclid(360.0);
    let chroma = (merged.chroma / merged.weight).clamp(MIN_CHROMA, MAX_CHROMA);
    Some(legible(hue, chroma))
}

/// Luminosité fixe pour un rendu homogène d'un jeu à l'autre, saturation
/// réduite jusqu'à entrer dans le gamut sRGB, puis luminosité relevée si le
/// contraste reste insuffisant.
fn legible(hue: f32, chroma: f32) -> Accent {
    let mut accent = Accent {
        l: TARGET_LIGHTNESS,
        c: chroma,
        h: hue,
    };
    loop {
        while accent.c > 0.0 && !in_gamut(accent) {
            accent.c = (accent.c - 0.005).max(0.0);
        }
        if accent.contrast_on_background() >= MIN_CONTRAST || accent.l >= 0.95 {
            return accent;
        }
        accent.l += 0.02;
    }
}

fn in_gamut(accent: Accent) -> bool {
    let [l, a, b] = [
        accent.l,
        accent.c * accent.h.to_radians().cos(),
        accent.c * accent.h.to_radians().sin(),
    ];
    oklab_to_linear(l, a, b)
        .iter()
        .all(|v| (-1e-4..=1.0 + 1e-4).contains(v))
}

// Conversions OKLab de Björn Ottosson (https://bottosson.github.io/posts/oklab/).

fn srgb_to_oklab(rgb: [f32; 3]) -> (f32, f32, f32) {
    let [r, g, b] = rgb.map(to_linear);
    let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    (
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    )
}

fn oklab_to_linear(l: f32, a: f32, b: f32) -> [f32; 3] {
    let l_ = (l + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
    let m_ = (l - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
    let s_ = (l - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
    [
        4.076_741_7 * l_ - 3.307_711_6 * m_ + 0.230_969_94 * s_,
        -1.268_438 * l_ + 2.609_757_4 * m_ - 0.341_319_38 * s_,
        -0.004_196_086_3 * l_ - 0.703_418_6 * m_ + 1.707_614_7 * s_,
    ]
}

fn oklab_to_srgb(l: f32, a: f32, b: f32) -> [f32; 3] {
    oklab_to_linear(l, a, b).map(|v| to_gamma(v.clamp(0.0, 1.0)))
}

fn to_linear(v: f32) -> f32 {
    if v <= 0.040_45 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn to_gamma(v: f32) -> f32 {
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

fn relative_luminance(rgb: [f32; 3]) -> f32 {
    let [r, g, b] = rgb.map(to_linear);
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

fn contrast(a: f32, b: f32) -> f32 {
    let (hi, lo) = if a > b { (a, b) } else { (b, a) };
    (hi + 0.05) / (lo + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    fn solid(rgb: [u8; 3]) -> DynamicImage {
        DynamicImage::ImageRgb8(RgbImage::from_pixel(60, 90, Rgb(rgb)))
    }

    /// Fond `base` avec un rectangle `spot` couvrant la fraction `share` de la hauteur.
    fn with_spot(base: [u8; 3], spot: [u8; 3], share: f32) -> DynamicImage {
        let mut img = RgbImage::from_pixel(60, 90, Rgb(base));
        let rows = (90.0 * share) as u32;
        for y in 0..rows {
            for x in 0..60 {
                img.put_pixel(x, y, Rgb(spot));
            }
        }
        DynamicImage::ImageRgb8(img)
    }

    fn hue_distance(a: f32, b: f32) -> f32 {
        let d = (a - b).rem_euclid(360.0);
        d.min(360.0 - d)
    }

    #[test]
    fn oklab_roundtrip() {
        for rgb in [[1.0, 0.0, 0.0], [0.2, 0.5, 0.9], [0.5, 0.5, 0.5]] {
            let (l, a, b) = srgb_to_oklab(rgb);
            let back = oklab_to_srgb(l, a, b);
            for (x, y) in rgb.iter().zip(back) {
                assert!((x - y).abs() < 1e-3, "{rgb:?} -> {back:?}");
            }
        }
    }

    #[test]
    fn keeps_the_hue_of_a_vivid_image() {
        // Bleu pur : teinte OKLCH ≈ 264°.
        let blue = accent_from_image(&solid([20, 60, 230])).unwrap();
        assert!(hue_distance(blue.h, 264.0) < 10.0, "{blue:?}");

        // Rouge : teinte ≈ 29°.
        let red = accent_from_image(&solid([200, 30, 40])).unwrap();
        assert!(hue_distance(red.h, 27.0) < 10.0, "{red:?}");
    }

    #[test]
    fn grey_images_have_no_accent() {
        assert_eq!(accent_from_image(&solid([128, 128, 128])), None);
        assert_eq!(accent_from_image(&solid([5, 5, 5])), None);
        assert_eq!(accent_from_image(&solid([250, 250, 250])), None);
    }

    #[test]
    fn a_small_vivid_area_beats_a_large_dark_one() {
        // Jaquette typique : surtout sombre, un titre ou un personnage coloré.
        let img = with_spot([15, 15, 20], [230, 160, 20], 0.15);
        let accent = accent_from_image(&img).unwrap();
        assert!(hue_distance(accent.h, 70.0) < 15.0, "{accent:?}");
    }

    #[test]
    fn a_tiny_speck_of_color_is_ignored() {
        let img = with_spot([40, 40, 40], [230, 30, 30], 0.02);
        assert_eq!(accent_from_image(&img), None);
    }

    #[test]
    fn dark_colors_are_lifted_to_a_legible_accent() {
        for rgb in [[10, 20, 70], [60, 10, 10], [10, 50, 20], [80, 0, 120]] {
            let accent = accent_from_image(&solid(rgb)).unwrap();
            assert!(accent.l >= TARGET_LIGHTNESS, "{rgb:?} -> {accent:?}");
            assert!(
                accent.contrast_on_background() >= MIN_CONTRAST,
                "{rgb:?} -> {accent:?}"
            );
        }
    }

    #[test]
    fn accents_stay_inside_srgb_and_bounds() {
        for rgb in [
            [0, 255, 0],
            [255, 0, 255],
            [0, 255, 255],
            [255, 255, 0],
            [255, 0, 0],
        ] {
            let accent = accent_from_image(&solid(rgb)).unwrap();
            assert!(in_gamut(accent), "{rgb:?} -> {accent:?}");
            assert!(accent.c <= MAX_CHROMA, "{rgb:?} -> {accent:?}");
            assert!(accent.contrast_on_background() >= MIN_CONTRAST);
        }
    }

    #[test]
    fn css_format() {
        let accent = Accent {
            l: 0.78,
            c: 0.1234,
            h: 264.06,
        };
        assert_eq!(accent.css(), "oklch(0.780 0.123 264.1)");
    }
}

//! Archives that hold the same mod several times, one folder per variant
//! (`Skip Intro/` and `Skip Intro + Instant Menu/`, each with the same
//! `.pak`): placed as is, they would write the same files twice. The player
//! picks one; an update keeps the one installed before.

use std::collections::HashMap;

use crate::loaders::Placement;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Variants {
    /// Folder that holds the variants (`""`: the root of the archive), with
    /// its trailing `/`.
    prefix: String,
    /// One per folder, in alphabetical order.
    pub names: Vec<String>,
}

/// The variants of a package, if its placements write some files twice
/// from sibling folders.
pub(crate) fn variants_of(placements: &[Placement]) -> Option<Variants> {
    // Windows paths: `a.pak` and `A.pak` are the same file.
    let mut count: HashMap<String, usize> = HashMap::new();
    for p in placements {
        *count.entry(p.target.to_lowercase()).or_default() += 1;
    }
    let clashing: Vec<&str> = placements
        .iter()
        .filter(|p| count[&p.target.to_lowercase()] > 1)
        .map(|p| p.source.as_str())
        .collect();
    if clashing.is_empty() {
        return None;
    }
    let paths: Vec<Vec<&str>> = clashing.iter().map(|s| s.split('/').collect()).collect();
    // Folders shared by every clashing file: the variants are just below.
    let shortest = paths.iter().map(Vec::len).min()?;
    let mut depth = 0;
    while depth + 1 < shortest && paths.iter().all(|p| p[depth] == paths[0][depth]) {
        depth += 1;
    }
    // The difference must be a folder, not the file name.
    if paths.iter().any(|p| p.len() <= depth + 1) {
        return None;
    }
    let prefix = paths[0][..depth].iter().map(|s| format!("{s}/")).collect();
    let mut names: Vec<String> = paths.iter().map(|p| p[depth].to_owned()).collect();
    names.sort();
    names.dedup();
    (names.len() > 1).then_some(Variants { prefix, names })
}

impl Variants {
    /// Variant of a file (`None`: shared by all, like a README next to the
    /// variant folders).
    fn of<'a>(&self, source: &'a str) -> Option<&'a str> {
        let (folder, _) = source.strip_prefix(&self.prefix)?.split_once('/')?;
        self.names.iter().any(|n| n == folder).then_some(folder)
    }

    /// Keeps the shared files and the ones of `pick`.
    pub fn keep(&self, placements: &mut Vec<Placement>, pick: &str) {
        placements.retain(|p| self.of(&p.source).is_none_or(|v| v == pick));
    }

    /// The variant an installed version used, found by its folder name in
    /// the paths it was placed from (the folders around it can change from
    /// one version to the next).
    pub fn used_by(&self, placements: &[Placement]) -> Option<String> {
        self.names
            .iter()
            .find(|name| {
                placements
                    .iter()
                    .any(|p| p.source.split('/').any(|folder| folder == name.as_str()))
            })
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placed(pairs: &[(&str, &str)]) -> Vec<Placement> {
        pairs
            .iter()
            .map(|(source, target)| Placement {
                source: (*source).into(),
                target: (*target).into(),
            })
            .collect()
    }

    const PAK: &str = "ProjectPBZ/Content/Paks/~mods/zzz_PBZSkipIntro_P.pak";

    fn skip_intro() -> Vec<Placement> {
        placed(&[
            ("README.txt", "README.txt"),
            (&format!("Skip Intro + Instant Menu/{PAK}"), PAK),
            (&format!("Skip Intro/{PAK}"), PAK),
        ])
    }

    #[test]
    fn finds_the_variant_folders_and_keeps_the_picked_one() {
        let mut placements = skip_intro();
        let variants = variants_of(&placements).unwrap();
        assert_eq!(variants.names, ["Skip Intro", "Skip Intro + Instant Menu"]);

        variants.keep(&mut placements, "Skip Intro + Instant Menu");
        let sources: Vec<&str> = placements.iter().map(|p| p.source.as_str()).collect();
        assert_eq!(
            sources,
            ["README.txt", &format!("Skip Intro + Instant Menu/{PAK}")]
        );
    }

    #[test]
    fn variants_deeper_in_the_archive() {
        let placements = placed(&[
            ("PBZSkipIntro/Lite/a.pak", "a.pak"),
            ("PBZSkipIntro/Full/a.pak", "a.pak"),
            ("PBZSkipIntro/Full/b.pak", "b.pak"),
        ]);
        let variants = variants_of(&placements).unwrap();
        assert_eq!(variants.names, ["Full", "Lite"]);
        let mut lite = placements.clone();
        variants.keep(&mut lite, "Lite");
        assert_eq!(lite.len(), 1);
    }

    #[test]
    fn no_clash_no_variants() {
        let placements = placed(&[("A/a.pak", "a.pak"), ("B/b.pak", "b.pak")]);
        assert_eq!(variants_of(&placements), None);
        // Two names for the same file in one folder: not variants.
        let same_folder = placed(&[("a.pak", "x.pak"), ("b.pak", "x.pak")]);
        assert_eq!(variants_of(&same_folder), None);
    }

    #[test]
    fn an_update_finds_the_variant_installed_before() {
        let variants = variants_of(&skip_intro()).unwrap();
        let before = placed(&[(&format!("PBZSkipIntro 1.1/Skip Intro/{PAK}"), PAK)]);
        assert_eq!(variants.used_by(&before).as_deref(), Some("Skip Intro"));
        // Installed before the archive had variants: the player picks.
        let old = placed(&[(PAK, PAK)]);
        assert_eq!(variants.used_by(&old), None);
    }
}

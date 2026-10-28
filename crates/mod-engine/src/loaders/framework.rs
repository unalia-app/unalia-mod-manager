//! Frameworks that a game's mods depend on (RED4ext, UE4SS…), described by
//! its profile (`games/*.toml`). Modalia installs them by itself, from their
//! authors' GitHub release, as soon as a mod needs one.
//!
//! Paths can contain `{project}`: the game's Unreal project folder, only
//! known once the game is installed (see `unreal`).

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::Placement;

/// Framework that mods depend on (game profile).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct Framework {
    pub name: String,
    /// Its Nexus page for this game, if there is one (used as the id).
    #[serde(default)]
    pub nexus_id: Option<u32>,
    /// The author's GitHub repo, e.g. `wopss/RED4ext`.
    pub github: String,
    /// Release to take (tag), else the latest stable one. UE4SS: recent
    /// Unreal 5 games need `experimental-latest`.
    #[serde(default)]
    pub release: Option<String>,
    /// Release file name: `{version}` replaced by the version, or `*` for a
    /// variable part (which then becomes the version).
    pub asset: String,
    /// File whose presence proves the framework is installed (by Modalia or
    /// by hand).
    pub marker: String,
    #[serde(default)]
    pub needed_by_prefix: Vec<String>,
    #[serde(default)]
    pub needed_by_ext: Vec<String>,
}

impl Framework {
    /// Install id: its Nexus page's, else taken from the GitHub repo
    /// (`github-ue4ss-re-re-ue4ss`).
    pub fn key(&self) -> String {
        match self.nexus_id {
            Some(id) => format!("nexus-{id}"),
            None => format!(
                "github-{}",
                self.github.to_ascii_lowercase().replace('/', "-")
            ),
        }
    }

    /// Replaces `{project}` in the paths.
    pub fn resolve(&self, project: Option<&str>) -> Self {
        let fill = |path: &str| match project {
            Some(project) => path.replace("{project}", project),
            None => path.to_owned(),
        };
        Self {
            marker: fill(&self.marker),
            needed_by_prefix: self.needed_by_prefix.iter().map(|p| fill(p)).collect(),
            ..self.clone()
        }
    }

    /// Does a file placed at `target` need this framework?
    pub fn needed_by(&self, target: &str) -> bool {
        let lower = target.to_ascii_lowercase();
        self.needed_by_prefix
            .iter()
            .any(|p| lower.starts_with(&p.to_ascii_lowercase()))
            || self
                .needed_by_ext
                .iter()
                .any(|e| lower.ends_with(&e.to_ascii_lowercase()))
    }

    /// Do these placements provide the framework itself?
    pub fn provided_by(&self, placements: &[Placement]) -> bool {
        placements
            .iter()
            .any(|p| p.target.eq_ignore_ascii_case(&self.marker))
    }

    pub fn installed_in(&self, game_dir: &Path) -> bool {
        game_dir.join(&self.marker).is_file()
    }
}

/// Frameworks needed by the placed files, except the ones they provide
/// themselves (ArchiveXL contains `.xl` files).
pub fn needed_frameworks<'a>(
    frameworks: &'a [Framework],
    placements: &[Placement],
) -> Vec<&'a Framework> {
    frameworks
        .iter()
        .filter(|f| !f.provided_by(placements))
        .filter(|f| placements.iter().any(|p| f.needed_by(&p.target)))
        .collect()
}

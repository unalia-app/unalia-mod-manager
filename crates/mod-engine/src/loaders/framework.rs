//! Frameworks dont dépendent les mods d'un jeu (RED4ext, UE4SS…), décrits
//! par son profil (`games/*.toml`). Modalia les installe tout seul, depuis
//! la release GitHub de leurs auteurs, dès qu'un mod en a besoin.
//!
//! Les chemins peuvent contenir `{project}` : le dossier du projet Unreal du
//! jeu, connu seulement une fois le jeu installé (voir `unreal`).

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::Placement;

/// Framework dont dépendent les mods (profil du jeu).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct Framework {
    pub name: String,
    /// Sa page Nexus pour ce jeu, si elle existe (sert d'identifiant).
    #[serde(default)]
    pub nexus_id: Option<u32>,
    /// Dépôt GitHub de l'auteur, ex. `wopss/RED4ext`.
    pub github: String,
    /// Release à prendre (tag), sinon la dernière stable. UE4SS : les jeux
    /// Unreal 5 récents ont besoin de `experimental-latest`.
    #[serde(default)]
    pub release: Option<String>,
    /// Nom du fichier de la release : `{version}` remplacé par la version,
    /// ou `*` pour une partie variable (qui devient alors la version).
    pub asset: String,
    /// Fichier dont la présence prouve que le framework est installé
    /// (par Modalia ou à la main).
    pub marker: String,
    #[serde(default)]
    pub needed_by_prefix: Vec<String>,
    #[serde(default)]
    pub needed_by_ext: Vec<String>,
}

impl Framework {
    /// Identifiant de l'installation : celui de sa page Nexus, sinon tiré du
    /// dépôt GitHub (`github-ue4ss-re-re-ue4ss`).
    pub fn key(&self) -> String {
        match self.nexus_id {
            Some(id) => format!("nexus-{id}"),
            None => format!(
                "github-{}",
                self.github.to_ascii_lowercase().replace('/', "-")
            ),
        }
    }

    /// Remplace `{project}` dans les chemins.
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

    /// Un fichier posé à `target` a-t-il besoin de ce framework ?
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

    /// Ces placements fournissent-ils le framework lui-même ?
    pub fn provided_by(&self, placements: &[Placement]) -> bool {
        placements
            .iter()
            .any(|p| p.target.eq_ignore_ascii_case(&self.marker))
    }

    pub fn installed_in(&self, game_dir: &Path) -> bool {
        game_dir.join(&self.marker).is_file()
    }
}

/// Frameworks dont ont besoin les fichiers placés, hors ceux qu'ils
/// fournissent eux-mêmes (ArchiveXL contient des `.xl`).
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

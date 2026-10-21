//! Brique **sources** : catalogues de mods.
//!
//! Chaque source traduit son catalogue dans un format de mod neutre
//! (SPEC §6.5), le seul que connaissent le reste du moteur et l'interface.

pub mod github;
pub mod nexus;
pub mod thunderstore;

use std::future::Future;

use serde::Serialize;

/// Identifiant d'un mod, unique dans sa source. Thunderstore :
/// `namespace-nom`, ex. `Bigfootmech-Silksong_Skip_Intro`.
pub type ModId = String;

/// Ce qu'affiche une carte de la grille Découvrir.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModSummary {
    pub id: ModId,
    pub source: &'static str,
    pub name: String,
    pub author: String,
    pub summary: String,
    pub icon_url: Option<String>,
    pub categories: Vec<String>,
    pub downloads: u64,
    /// Date ISO 8601.
    pub updated_at: String,
    /// Mis en avant par la source (souvent les loaders et bibliothèques).
    pub pinned: bool,
    pub deprecated: bool,
}

/// Fiche complète d'un mod (page de détail, installation).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Mod {
    pub id: ModId,
    pub source: &'static str,
    pub name: String,
    pub author: String,
    pub summary: String,
    pub images: Vec<String>,
    pub categories: Vec<String>,
    pub downloads: Option<u64>,
    pub updated_at: String,
    /// Page du mod sur la source : toujours accessible (SPEC §2.4).
    pub source_url: String,
    pub website_url: Option<String>,
    /// Lien de don de l'auteur (Ko-fi, Patreon…), s'il en a publié un.
    pub donation_url: Option<String>,
    pub deprecated: bool,
    /// Versions, la plus récente en premier.
    pub versions: Vec<ModVersion>,
}

impl Mod {
    pub fn latest(&self) -> Option<&ModVersion> {
        self.versions.first()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModVersion {
    pub number: String,
    pub published_at: String,
    pub download_url: String,
    /// Empreinte fournie par la source, si elle en publie une.
    /// Thunderstore n'en publie pas : Modalia calcule la sienne au
    /// téléchargement et la conserve.
    pub sha256: Option<String>,
    pub size: Option<u64>,
    pub dependencies: Vec<Dependency>,
}

/// Dépendance déclarée par une version.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dependency {
    pub id: ModId,
    /// Version minimale demandée (convention Thunderstore : on installe la
    /// plus récente si elle est supérieure).
    pub min_version: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Sort {
    #[default]
    Popular,
    Newest,
    Updated,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchQuery {
    pub text: String,
    pub sort: Sort,
    /// À partir de 1.
    pub page: u32,
    /// Inclure les bibliothèques techniques. Par défaut non : elles
    /// trustent les « populaires » (tout le monde en dépend) alors que
    /// Modalia les installe tout seul comme dépendances.
    pub include_libraries: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: u64,
    pub has_more: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("{source_name} can't be reached")]
    Network {
        source_name: &'static str,
        #[source]
        error: reqwest::Error,
    },
    #[error("this mod no longer exists on {source_name}")]
    NotFound { source_name: &'static str },
    #[error("connect your {source_name} account first")]
    NotConnected { source_name: &'static str },
    #[error("{source_name} refused your API key")]
    Unauthorized { source_name: &'static str },
    #[error("{source_name} only allows this download from its website")]
    WebsiteOnly { source_name: &'static str },
    #[error("unexpected response from {source_name}: {detail}")]
    Unexpected {
        source_name: &'static str,
        detail: String,
    },
}

/// Un catalogue de mods. `game` est l'identifiant du jeu propre à la source
/// (Thunderstore : la communauté, ex. `hollow-knight-silksong`).
pub trait ModSource {
    fn name(&self) -> &'static str;

    fn search(
        &self,
        game: &str,
        query: &SearchQuery,
    ) -> impl Future<Output = Result<Page<ModSummary>, SourceError>> + Send;

    fn get(&self, game: &str, id: &str) -> impl Future<Output = Result<Mod, SourceError>> + Send;

    /// Description longue, en Markdown brut de la source : à nettoyer avant
    /// tout affichage (SPEC §7, jamais de HTML brut).
    fn description(
        &self,
        game: &str,
        id: &str,
        version: &str,
    ) -> impl Future<Output = Result<String, SourceError>> + Send;
}

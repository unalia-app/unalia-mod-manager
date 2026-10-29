//! **sources** block: mod catalogs.
//!
//! Each source translates its catalog into a neutral mod format, the only
//! one that the rest of the engine and the UI know.

pub mod github;
pub mod hexium;
pub mod nexus;
pub mod nexus_sso;
pub mod thunderstore;

use std::future::Future;

use serde::Serialize;

/// A mod's id, unique within its source. Thunderstore: `namespace-name`,
/// e.g. `Bigfootmech-Silksong_Skip_Intro`.
pub type ModId = String;

/// What a card of the Discover grid shows.
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
    /// Featured by the source (often loaders and libraries).
    pub pinned: bool,
    pub deprecated: bool,
}

/// Full page of a mod (detail page, install).
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
    /// The mod's page on the source: always reachable.
    pub source_url: String,
    pub website_url: Option<String>,
    /// The author's donation link (Ko-fi, Patreon…), if they published one.
    pub donation_url: Option<String>,
    pub deprecated: bool,
    /// Versions, the most recent first.
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
    /// Hash provided by the source, if it publishes one. Thunderstore
    /// doesn't: Modalia computes its own at download time and keeps it.
    pub sha256: Option<String>,
    pub size: Option<u64>,
    pub dependencies: Vec<Dependency>,
}

/// Dependency declared by a version.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dependency {
    pub id: ModId,
    /// Minimum version asked (Thunderstore convention: we install the most
    /// recent one if it is higher).
    pub min_version: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Sort {
    /// The best rated by the community (Thunderstore: votes, Nexus:
    /// endorsements).
    #[default]
    Popular,
    /// The most downloaded.
    Downloads,
    Newest,
    Updated,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchQuery {
    pub text: String,
    pub sort: Sort,
    /// Starts at 1.
    pub page: u32,
    /// Include technical libraries. Off by default: they crowd the
    /// "popular" list (everyone depends on them) while Modalia installs
    /// them by itself as dependencies.
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
    #[error("{source_name} limits requests per hour: try again in {minutes} min")]
    RateLimited {
        source_name: &'static str,
        minutes: u64,
    },
    #[error("{source_name} only allows this download from its website")]
    WebsiteOnly { source_name: &'static str },
    #[error("unexpected response from {source_name}: {detail}")]
    Unexpected {
        source_name: &'static str,
        detail: String,
    },
}

/// A mod catalog. `game` is the source's own game id (Thunderstore: the
/// community, e.g. `hollow-knight-silksong`).
pub trait ModSource {
    fn name(&self) -> &'static str;

    fn search(
        &self,
        game: &str,
        query: &SearchQuery,
    ) -> impl Future<Output = Result<Page<ModSummary>, SourceError>> + Send;

    fn get(&self, game: &str, id: &str) -> impl Future<Output = Result<Mod, SourceError>> + Send;

    /// Long description, in the source's raw Markdown: sanitize it before
    /// any display (never raw HTML).
    fn description(
        &self,
        game: &str,
        id: &str,
        version: &str,
    ) -> impl Future<Output = Result<String, SourceError>> + Send;
}

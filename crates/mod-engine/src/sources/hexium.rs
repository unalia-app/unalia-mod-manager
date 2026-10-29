//! **Hexium** source (hexium.gg), a Thunderstore-compatible mod repository
//! that many Valheim authors moved to.
//!
//! - Catalog: `c/<community>/api/v1/package-listing-chunk/`, the whole
//!   community in one gzip-encoded JSON file. Hexium has no search API:
//!   the catalog is kept in memory for a few minutes and searched here.
//! - Description: `api/experimental/package/...` (same routes as
//!   Thunderstore).
//! - Mods published on Thunderstore only (a lot of modpack dependencies)
//!   are fetched from Thunderstore: ids are the same `namespace-name` on
//!   both sites.

use std::cmp::Reverse;
use std::collections::HashMap;
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use reqwest::{Client, StatusCode};
use serde::Deserialize;

use super::thunderstore::Thunderstore;
use super::{
    Dependency, Mod, ModSource, ModSummary, ModVersion, Page, SearchQuery, Sort, SourceError,
};

pub const BASE_URL: &str = "https://hexium.gg";
const NAME: &str = "Hexium";
const SOURCE_ID: &str = "hexium";

/// Mods per page, as on Thunderstore.
const PAGE_SIZE: usize = 20;
/// A catalog is downloaded again after this: new mods show up without
/// restarting, without a download on every search.
const CATALOG_MAX_AGE: Duration = Duration::from_secs(10 * 60);
/// Beyond this, the catalog is abnormal: we stop reading it.
const MAX_CATALOG_SIZE: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct Hexium {
    client: Client,
    base_url: String,
    /// For the mods Hexium doesn't have.
    thunderstore: Thunderstore,
    catalogs: Arc<Mutex<HashMap<String, Fetched>>>,
    /// Communities hosted on Hexium, asked once per session.
    communities: Arc<tokio::sync::Mutex<Option<Vec<String>>>>,
    /// Refuse downloads that aren't over HTTPS. Always true outside tests
    /// (the fake test server speaks HTTP).
    require_https: bool,
}

impl Hexium {
    pub fn new(client: Client, thunderstore: Thunderstore) -> Self {
        Self {
            require_https: true,
            ..Self::with_base_url(client, BASE_URL, thunderstore)
        }
    }

    /// For tests: points to a fake server, which may serve its archives
    /// over HTTP.
    pub fn with_base_url(client: Client, base_url: &str, thunderstore: Thunderstore) -> Self {
        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_owned(),
            thunderstore,
            catalogs: Arc::default(),
            communities: Arc::default(),
            require_https: base_url.starts_with("https://"),
        }
    }

    /// Communities hosted on Hexium (e.g. `valheim`), the same ids as on
    /// Thunderstore. Unreachable: retried at the next call.
    pub async fn communities(&self) -> Result<Vec<String>, SourceError> {
        let mut known = self.communities.lock().await;
        if let Some(list) = known.as_ref() {
            return Ok(list.clone());
        }
        let url = format!("{}/api/experimental/community/", self.base_url);
        let body = self.fetch(&url).await?;
        let list: CommunityList = parse(&body)?;
        let ids: Vec<String> = list.results.into_iter().map(|c| c.identifier).collect();
        *known = Some(ids.clone());
        Ok(ids)
    }

    /// The community's whole catalog, from memory if it is recent.
    async fn catalog(&self, community: &str) -> Result<Arc<Catalog>, SourceError> {
        let community = segment(community)?;
        if let Some((at, catalog)) = self.catalogs.lock().unwrap().get(community)
            && at.elapsed() < CATALOG_MAX_AGE
        {
            return Ok(catalog.clone());
        }
        let url = format!(
            "{}/c/{community}/api/v1/package-listing-chunk/",
            self.base_url
        );
        let body = gunzip(&self.fetch(&url).await?)?;
        let packages: Vec<Package> = parse(&body)?;
        let catalog = Arc::new(Catalog::new(packages));
        self.catalogs
            .lock()
            .unwrap()
            .insert(community.to_owned(), (Instant::now(), catalog.clone()));
        Ok(catalog)
    }

    async fn fetch(&self, url: &str) -> Result<Vec<u8>, SourceError> {
        let network = |error| SourceError::Network {
            source_name: NAME,
            error,
        };
        let response = self.client.get(url).send().await.map_err(network)?;
        if response.status() == StatusCode::NOT_FOUND {
            return Err(SourceError::NotFound { source_name: NAME });
        }
        let body = response
            .error_for_status()
            .map_err(network)?
            .bytes()
            .await
            .map_err(network)?;
        Ok(body.to_vec())
    }
}

impl ModSource for Hexium {
    fn name(&self) -> &'static str {
        NAME
    }

    async fn search(
        &self,
        community: &str,
        query: &SearchQuery,
    ) -> Result<Page<ModSummary>, SourceError> {
        let catalog = self.catalog(community).await?;
        let words: Vec<String> = query
            .text
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();
        let mut found: Vec<&Package> = catalog
            .packages
            .iter()
            .filter(|p| !p.has_nsfw_content && p.latest().is_some())
            .filter(|p| words.iter().all(|w| p.matches(w)))
            .collect();
        match query.sort {
            Sort::Popular => found.sort_by_key(|p| Reverse((p.rating_score, p.downloads()))),
            Sort::Downloads => found.sort_by_key(|p| Reverse(p.downloads())),
            Sort::Newest => found.sort_by(|a, b| b.date_created.cmp(&a.date_created)),
            Sort::Updated => found.sort_by(|a, b| b.date_updated.cmp(&a.date_updated)),
        }

        let start = (query.page.max(1) as usize - 1) * PAGE_SIZE;
        let items = found
            .iter()
            .skip(start)
            .take(PAGE_SIZE)
            .map(|p| p.summary())
            .collect();
        Ok(Page {
            items,
            total: found.len() as u64,
            has_more: start + PAGE_SIZE < found.len(),
        })
    }

    async fn get(&self, community: &str, id: &str) -> Result<Mod, SourceError> {
        let catalog = self.catalog(community).await?;
        match catalog.get(id) {
            Some(package) => package.to_mod(self.require_https),
            None => self.thunderstore.get(community, id).await,
        }
    }

    async fn description(
        &self,
        community: &str,
        id: &str,
        version: &str,
    ) -> Result<String, SourceError> {
        let catalog = self.catalog(community).await?;
        let Some(package) = catalog.get(id) else {
            return self.thunderstore.description(community, id, version).await;
        };
        let url = format!(
            "{}/api/experimental/package/{}/{}/{}/readme/",
            self.base_url,
            segment(&package.owner)?,
            segment(&package.name)?,
            segment(version)?
        );
        let readme: Readme = parse(&self.fetch(&url).await?)?;
        Ok(readme.markdown)
    }
}

/// A catalog and when it was downloaded.
type Fetched = (Instant, Arc<Catalog>);

/// A community's packages, with an index by id.
#[derive(Debug)]
struct Catalog {
    packages: Vec<Package>,
    by_id: HashMap<String, usize>,
}

impl Catalog {
    fn new(packages: Vec<Package>) -> Self {
        let by_id = packages
            .iter()
            .enumerate()
            .map(|(i, p)| (p.full_name.clone(), i))
            .collect();
        Self { packages, by_id }
    }

    fn get(&self, id: &str) -> Option<&Package> {
        self.by_id.get(id).map(|&i| &self.packages[i])
    }
}

#[derive(Debug, Deserialize)]
struct CommunityList {
    results: Vec<Community>,
}

#[derive(Debug, Deserialize)]
struct Community {
    identifier: String,
}

#[derive(Debug, Deserialize)]
struct Readme {
    markdown: String,
}

#[derive(Debug, Deserialize)]
struct Package {
    name: String,
    /// `namespace-name`: the id, as on Thunderstore.
    full_name: String,
    owner: String,
    package_url: String,
    donation_link: Option<String>,
    date_created: String,
    date_updated: String,
    #[serde(default)]
    rating_score: i64,
    #[serde(default)]
    is_pinned: bool,
    #[serde(default)]
    is_deprecated: bool,
    #[serde(default)]
    has_nsfw_content: bool,
    #[serde(default)]
    categories: Vec<String>,
    /// The most recent first.
    versions: Vec<Version>,
}

#[derive(Debug, Deserialize)]
struct Version {
    version_number: String,
    description: String,
    icon: Option<String>,
    #[serde(default)]
    dependencies: Vec<String>,
    download_url: String,
    #[serde(default)]
    downloads: u64,
    date_created: String,
    website_url: Option<String>,
    #[serde(default = "active")]
    is_active: bool,
    file_size: Option<u64>,
}

fn active() -> bool {
    true
}

impl Package {
    fn latest(&self) -> Option<&Version> {
        self.versions.iter().find(|v| v.is_active)
    }

    fn downloads(&self) -> u64 {
        self.versions.iter().map(|v| v.downloads).sum()
    }

    /// `word` is lowercase: name, author or description contains it.
    fn matches(&self, word: &str) -> bool {
        self.name.to_lowercase().replace('_', " ").contains(word)
            || self.owner.to_lowercase().contains(word)
            || self
                .latest()
                .is_some_and(|v| v.description.to_lowercase().contains(word))
    }

    fn summary(&self) -> ModSummary {
        let latest = self.latest();
        ModSummary {
            id: self.full_name.clone(),
            source: SOURCE_ID,
            name: display_name(&self.name),
            author: self.owner.clone(),
            summary: latest.map(|v| v.description.clone()).unwrap_or_default(),
            icon_url: latest.and_then(|v| v.icon.clone()),
            categories: self.categories.clone(),
            downloads: self.downloads(),
            updated_at: self.date_updated.clone(),
            pinned: self.is_pinned,
            deprecated: self.is_deprecated,
        }
    }

    fn to_mod(&self, require_https: bool) -> Result<Mod, SourceError> {
        let versions: Vec<&Version> = self.versions.iter().filter(|v| v.is_active).collect();
        // We refuse any other scheme rather than download in clear text.
        if require_https
            && let Some(v) = versions
                .iter()
                .find(|v| !v.download_url.starts_with("https://"))
        {
            return Err(SourceError::Unexpected {
                source_name: NAME,
                detail: format!("insecure download URL: {}", v.download_url),
            });
        }
        let latest = versions.first();
        Ok(Mod {
            id: self.full_name.clone(),
            source: SOURCE_ID,
            name: display_name(&self.name),
            author: self.owner.clone(),
            summary: latest.map(|v| v.description.clone()).unwrap_or_default(),
            images: latest.and_then(|v| v.icon.clone()).into_iter().collect(),
            categories: self.categories.clone(),
            downloads: Some(self.downloads()),
            updated_at: self.date_updated.clone(),
            source_url: self.package_url.clone(),
            website_url: latest
                .and_then(|v| v.website_url.clone())
                .filter(|u| !u.is_empty()),
            donation_url: self
                .donation_link
                .clone()
                .filter(|l| l.starts_with("https://")),
            deprecated: self.is_deprecated,
            versions: versions
                .into_iter()
                .map(|v| ModVersion {
                    number: v.version_number.clone(),
                    published_at: v.date_created.clone(),
                    download_url: v.download_url.clone(),
                    sha256: None,
                    size: v.file_size,
                    dependencies: v
                        .dependencies
                        .iter()
                        .filter_map(|d| parse_dependency(d))
                        .collect(),
                })
                .collect(),
        })
    }
}

/// The catalog comes gzip-encoded, without saying so in its headers. A
/// body that isn't gzip is taken as is.
fn gunzip(body: &[u8]) -> Result<Vec<u8>, SourceError> {
    if !body.starts_with(&[0x1f, 0x8b]) {
        return Ok(body.to_vec());
    }
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(body)
        .take(MAX_CATALOG_SIZE)
        .read_to_end(&mut out)
        .map_err(|e| SourceError::Unexpected {
            source_name: NAME,
            detail: e.to_string(),
        })?;
    Ok(out)
}

fn parse<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, SourceError> {
    serde_json::from_slice(body).map_err(|e| SourceError::Unexpected {
        source_name: NAME,
        detail: e.to_string(),
    })
}

/// Safe URL segment (community, author, name, version number).
fn segment(s: &str) -> Result<&str, SourceError> {
    let ok = !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if ok && s != "." && s != ".." {
        Ok(s)
    } else {
        Err(SourceError::NotFound { source_name: NAME })
    }
}

/// `namespace-name-1.2.3` gives dependency `namespace-name` ≥ 1.2.3.
fn parse_dependency(full: &str) -> Option<Dependency> {
    let (id, version) = full.rsplit_once('-')?;
    let (namespace, name) = id.split_once('-')?;
    let valid = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    (valid(namespace) && valid(name) && !version.is_empty()).then(|| Dependency {
        id: id.to_owned(),
        min_version: version.to_owned(),
    })
}

/// `Azu_Crafty_Boxes` → `Azu Crafty Boxes`.
fn display_name(name: &str) -> String {
    name.replace('_', " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_dependencies() {
        assert_eq!(
            parse_dependency("denikson-BepInExPack_Valheim-5.4.2333"),
            Some(Dependency {
                id: "denikson-BepInExPack_Valheim".into(),
                min_version: "5.4.2333".into(),
            })
        );
        assert_eq!(parse_dependency("BepInExPack-5.4.2100"), None);
        assert_eq!(parse_dependency("ns-name-"), None);
        assert_eq!(parse_dependency("ns/..-name-1.0.0"), None);
    }

    #[test]
    fn url_segments_are_restricted() {
        assert!(segment("valheim").is_ok());
        assert!(segment("1.0.3").is_ok());
        for bad in ["", ".", "..", "a/b", "a?b", "a b"] {
            assert!(segment(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn plain_bodies_are_not_unzipped() {
        assert_eq!(gunzip(b"[]").unwrap(), b"[]");
    }
}

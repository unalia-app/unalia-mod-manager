//! **Thunderstore** source (thunderstore.io).
//!
//! - Search: `api/cyberstorm/listing/<community>/`, the site's paginated
//!   API (undocumented, but the only one that allows search and sorting
//!   without downloading the whole catalog).
//! - Mod page and description: `api/experimental/package/...` (documented).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use reqwest::{Client, StatusCode};
use serde::Deserialize;
use serde::de::DeserializeOwned;

use super::{
    Dependency, Mod, ModSource, ModSummary, ModVersion, Page, SearchQuery, Sort, SourceError,
};

pub const BASE_URL: &str = "https://thunderstore.io";
const NAME: &str = "Thunderstore";
const SOURCE_ID: &str = "thunderstore";

/// Mod managers published as packages: useless in Modalia, we don't offer
/// them.
pub const MOD_MANAGERS: [&str; 2] = ["ebkr-r2modman", "Kesomannen-GaleModManager"];

#[derive(Debug, Clone)]
pub struct Thunderstore {
    client: Client,
    base_url: String,
    /// Id of the "Libraries" category of each community (`None`: the
    /// community has none).
    library_categories: Arc<Mutex<HashMap<String, Option<u64>>>>,
    /// Refuse downloads that aren't over HTTPS. Always true outside tests
    /// (the fake test server speaks HTTP).
    require_https: bool,
}

impl Thunderstore {
    pub fn new(client: Client) -> Self {
        Self {
            require_https: true,
            ..Self::with_base_url(client, BASE_URL)
        }
    }

    /// For tests: points to a fake server, which may serve its archives
    /// over HTTP.
    pub fn with_base_url(client: Client, base_url: &str) -> Self {
        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_owned(),
            library_categories: Arc::default(),
            require_https: base_url.starts_with("https://"),
        }
    }

    /// Enforces (or not) HTTPS for downloads.
    pub fn require_https(mut self, required: bool) -> Self {
        self.require_https = required;
        self
    }

    /// Donation link of the author's team. Optional: an error here must not
    /// prevent showing the mod. HTTPS only.
    async fn donation_url(&self, namespace: &str) -> Option<String> {
        let url = format!("{}/api/cyberstorm/team/{namespace}/", self.base_url);
        let team: Team = self.fetch(&url, &[]).await.ok()?;
        team.donation_link.filter(|l| l.starts_with("https://"))
    }

    /// Category filters expect ids specific to each community: we ask for
    /// them once. On failure, no filter (and we try again at the next
    /// search).
    async fn library_category(&self, community: &str) -> Option<u64> {
        if let Some(&cached) = self.library_categories.lock().unwrap().get(community) {
            return cached;
        }
        let url = format!(
            "{}/api/cyberstorm/community/{community}/filters/",
            self.base_url
        );
        let filters: Filters = self.fetch(&url, &[]).await.ok()?;
        let id = filters
            .package_categories
            .iter()
            .find(|c| c.slug == "libraries")
            .and_then(|c| c.id.parse().ok());
        self.library_categories
            .lock()
            .unwrap()
            .insert(community.to_owned(), id);
        id
    }

    async fn fetch<T: DeserializeOwned>(
        &self,
        url: &str,
        query: &[(&str, String)],
    ) -> Result<T, SourceError> {
        let network = |error| SourceError::Network {
            source_name: NAME,
            error,
        };
        let response = self
            .client
            .get(url)
            .query(query)
            .send()
            .await
            .map_err(network)?;
        if response.status() == StatusCode::NOT_FOUND {
            return Err(SourceError::NotFound { source_name: NAME });
        }
        let body = response
            .error_for_status()
            .map_err(network)?
            .text()
            .await
            .map_err(network)?;
        serde_json::from_str(&body).map_err(|e| SourceError::Unexpected {
            source_name: NAME,
            detail: e.to_string(),
        })
    }
}

impl ModSource for Thunderstore {
    fn name(&self) -> &'static str {
        NAME
    }

    async fn search(
        &self,
        community: &str,
        query: &SearchQuery,
    ) -> Result<Page<ModSummary>, SourceError> {
        let community = segment(community)?;
        let url = format!("{}/api/cyberstorm/listing/{community}/", self.base_url);
        let ordering = match query.sort {
            Sort::Popular => "top-rated",
            Sort::Downloads => "most-downloaded",
            Sort::Newest => "newest",
            Sort::Updated => "last-updated",
        };
        let mut params = vec![
            ("ordering", ordering.to_owned()),
            ("page", query.page.max(1).to_string()),
        ];
        if !query.text.trim().is_empty() {
            params.push(("q", query.text.trim().to_owned()));
        }
        if !query.include_libraries
            && let Some(id) = self.library_category(community).await
        {
            params.push(("excluded_categories", id.to_string()));
        }

        let listing: Listing = self.fetch(&url, &params).await?;
        Ok(Page {
            has_more: listing.next.is_some(),
            total: listing.count,
            items: listing
                .results
                .into_iter()
                .map(ListingItem::into_summary)
                .collect(),
        })
    }

    async fn get(&self, community: &str, id: &str) -> Result<Mod, SourceError> {
        let (namespace, name) = split_id(id)?;
        let url = format!(
            "{}/api/experimental/package/{namespace}/{name}/",
            self.base_url
        );
        let package: Package = self.fetch(&url, &[]).await?;
        let mut found =
            package.into_mod(&self.base_url, segment(community)?, self.require_https)?;
        found.donation_url = self.donation_url(namespace).await;
        Ok(found)
    }

    async fn description(
        &self,
        _game: &str,
        id: &str,
        version: &str,
    ) -> Result<String, SourceError> {
        let (namespace, name) = split_id(id)?;
        let url = format!(
            "{}/api/experimental/package/{namespace}/{name}/{}/readme/",
            self.base_url,
            segment(version)?
        );
        let readme: Readme = self.fetch(&url, &[]).await?;
        Ok(readme.markdown)
    }
}

/// `namespace-name` to (`namespace`, `name`). Thunderstore only allows
/// letters, digits and `_`: anything else is refused before building a
/// URL.
fn split_id(id: &str) -> Result<(&str, &str), SourceError> {
    id.split_once('-')
        .filter(|(ns, name)| is_identifier(ns) && is_identifier(name))
        .ok_or(SourceError::NotFound { source_name: NAME })
}

fn is_identifier(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Safe URL segment (community, version number).
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
    split_id(id).ok()?;
    (!version.is_empty()).then(|| Dependency {
        id: id.to_owned(),
        min_version: version.to_owned(),
    })
}

#[derive(Deserialize)]
struct Team {
    donation_link: Option<String>,
}

#[derive(Deserialize)]
struct Filters {
    package_categories: Vec<FilterCategory>,
}

#[derive(Deserialize)]
struct FilterCategory {
    id: String,
    slug: String,
}

#[derive(Deserialize)]
struct Listing {
    count: u64,
    next: Option<String>,
    results: Vec<ListingItem>,
}

#[derive(Deserialize)]
struct ListingItem {
    namespace: String,
    name: String,
    description: String,
    icon_url: Option<String>,
    #[serde(default)]
    categories: Vec<Category>,
    download_count: u64,
    last_updated: String,
    #[serde(default)]
    is_pinned: bool,
    #[serde(default)]
    is_deprecated: bool,
}

#[derive(Deserialize)]
struct Category {
    name: String,
}

impl ListingItem {
    fn into_summary(self) -> ModSummary {
        ModSummary {
            id: format!("{}-{}", self.namespace, self.name),
            source: SOURCE_ID,
            name: display_name(&self.name),
            author: self.namespace,
            summary: self.description,
            icon_url: self.icon_url,
            categories: categories(self.categories.into_iter().map(|c| c.name)),
            downloads: self.download_count,
            updated_at: self.last_updated,
            pinned: self.is_pinned,
            deprecated: self.is_deprecated,
        }
    }
}

#[derive(Deserialize)]
struct Package {
    namespace: String,
    name: String,
    owner: String,
    date_updated: String,
    #[serde(default)]
    is_deprecated: bool,
    /// -1 when Thunderstore doesn't compute it.
    total_downloads: i64,
    latest: Version,
    #[serde(default)]
    community_listings: Vec<CommunityListing>,
}

#[derive(Deserialize)]
struct Version {
    version_number: String,
    description: String,
    icon: Option<String>,
    #[serde(default)]
    dependencies: Vec<String>,
    download_url: String,
    date_created: String,
    website_url: Option<String>,
}

#[derive(Deserialize)]
struct CommunityListing {
    community: String,
    #[serde(default)]
    categories: Vec<String>,
}

#[derive(Deserialize)]
struct Readme {
    markdown: String,
}

impl Package {
    fn into_mod(
        self,
        base_url: &str,
        community: &str,
        require_https: bool,
    ) -> Result<Mod, SourceError> {
        // The API doesn't guarantee an HTTPS URL: we refuse any other scheme
        // rather than download in clear text.
        if require_https && !self.latest.download_url.starts_with("https://") {
            return Err(SourceError::Unexpected {
                source_name: NAME,
                detail: format!("insecure download URL: {}", self.latest.download_url),
            });
        }

        let categories = self
            .community_listings
            .into_iter()
            .find(|l| l.community == community)
            .map(|l| l.categories)
            .unwrap_or_default();
        let latest = self.latest;

        Ok(Mod {
            id: format!("{}-{}", self.namespace, self.name),
            source: SOURCE_ID,
            name: display_name(&self.name),
            source_url: format!(
                "{base_url}/c/{community}/p/{}/{}/",
                self.namespace, self.name
            ),
            author: self.owner,
            summary: latest.description,
            images: latest.icon.into_iter().collect(),
            categories: self::categories(categories.into_iter()),
            downloads: u64::try_from(self.total_downloads).ok(),
            updated_at: self.date_updated,
            website_url: latest.website_url.filter(|u| !u.is_empty()),
            donation_url: None,
            deprecated: self.is_deprecated,
            versions: vec![ModVersion {
                dependencies: latest
                    .dependencies
                    .iter()
                    .filter_map(|d| parse_dependency(d))
                    .collect(),
                number: latest.version_number,
                published_at: latest.date_created,
                download_url: latest.download_url,
                sha256: None,
                size: None,
            }],
        })
    }
}

/// `Silksong_Skip_Intro` → `Silksong Skip Intro`.
fn display_name(name: &str) -> String {
    name.replace('_', " ")
}

/// Removes the categories Thunderstore marks as deprecated.
fn categories(names: impl Iterator<Item = String>) -> Vec<String> {
    names.filter(|n| !n.starts_with("(Deprecated")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_ids_and_rejects_anything_else() {
        assert_eq!(
            split_id("Bigfootmech-Silksong_Skip_Intro").unwrap(),
            ("Bigfootmech", "Silksong_Skip_Intro")
        );
        for bad in [
            "",
            "nodash",
            "-name",
            "ns-",
            "ns-na-me",
            "ns/../x-y",
            "ns-na me",
            "ns-n%2F",
        ] {
            assert!(split_id(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn parses_dependencies() {
        assert_eq!(
            parse_dependency("silksong_modding-BepInExPack_Silksong-1.0.3"),
            Some(Dependency {
                id: "silksong_modding-BepInExPack_Silksong".into(),
                min_version: "1.0.3".into(),
            })
        );
        assert_eq!(parse_dependency("BepInExPack-5.4.2100"), None);
        assert_eq!(parse_dependency("ns-name-"), None);
    }

    #[test]
    fn url_segments_are_restricted() {
        assert!(segment("hollow-knight-silksong").is_ok());
        assert!(segment("1.0.3").is_ok());
        for bad in ["", ".", "..", "a/b", "a?b", "a b"] {
            assert!(segment(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn display_names_and_categories() {
        assert_eq!(display_name("Silksong_Skip_Intro"), "Silksong Skip Intro");
        assert_eq!(
            categories(["(Deprecated category) Misc".into(), "Utility".into()].into_iter()),
            ["Utility"]
        );
    }
}

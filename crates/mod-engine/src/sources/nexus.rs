//! **Nexus Mods** source (nexusmods.com).
//!
//! - Search, mod page, files and requirements: public GraphQL v2 API.
//! - Account and download links: REST v1 API, with the user's API key.
//!
//! Nexus keeps direct downloads through the API for Premium accounts. A
//! free account clicks "Mod Manager Download" on the site, which opens an
//! `nxm://` link: that link carries a temporary key that allows the
//! download. Modalia never works around this rule.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use reqwest::{Client, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{
    Dependency, Mod, ModSource, ModSummary, ModVersion, Page, SearchQuery, Sort, SourceError,
};

pub const GRAPHQL_URL: &str = "https://api.nexusmods.com/v2/graphql";
pub const API_URL: &str = "https://api.nexusmods.com/v1";
const NAME: &str = "Nexus Mods";
const SOURCE_ID: &str = "nexus";
const PAGE_SIZE: u32 = 24;
/// Nexus category of libraries: Modalia installs them by itself.
const LIBRARIES: &str = "Modders Resources";

/// Modalia id of a Nexus mod: `nexus-<id>`.
pub fn mod_key(mod_id: u32) -> String {
    format!("nexus-{mod_id}")
}

/// `nexus-<id>` → `<id>`.
pub fn parse_mod_key(key: &str) -> Option<u32> {
    key.strip_prefix("nexus-")?.parse().ok()
}

/// Modalia community of a Nexus game: `nexus:<domain>`.
pub fn domain_of(community: &str) -> Option<&str> {
    community
        .strip_prefix("nexus:")
        .filter(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_alphanumeric()))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub name: String,
    pub premium: bool,
    pub avatar_url: Option<String>,
}

/// Game found on Nexus from its name (`Nexus::find_games`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FoundGame {
    pub name: String,
    pub domain: String,
    pub game_id: u32,
}

/// Link `nxm://<domain>/mods/<mod>/files/<file>?key=…&expires=…`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NxmLink {
    pub domain: String,
    pub mod_id: u32,
    pub file_id: u32,
    /// Missing for a Premium account.
    pub key: Option<String>,
    pub expires: Option<u64>,
}

impl NxmLink {
    pub fn parse(url: &str) -> Option<Self> {
        let rest = url.strip_prefix("nxm://")?;
        let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
        let parts: Vec<&str> = path.trim_end_matches('/').split('/').collect();
        let [domain, "mods", mod_id, "files", file_id] = parts.as_slice() else {
            return None;
        };
        let domain = domain.to_ascii_lowercase();
        if domain.is_empty() || !domain.chars().all(|c| c.is_ascii_alphanumeric()) {
            return None;
        }
        let mut key = None;
        let mut expires = None;
        for pair in query.split('&') {
            match pair.split_once('=') {
                Some(("key", v)) if is_token(v) => key = Some(v.to_owned()),
                Some(("expires", v)) => expires = v.parse().ok(),
                _ => {}
            }
        }
        Some(Self {
            domain,
            mod_id: mod_id.parse().ok()?,
            file_id: file_id.parse().ok()?,
            key,
            expires,
        })
    }
}

fn is_token(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

/// A downloadable file of a mod.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModFile {
    pub file_id: u32,
    pub name: String,
    pub version: String,
    /// `MAIN`, `OPTIONAL`, `UPDATE`, `OLD_VERSION`…
    pub category: String,
    pub primary: bool,
    pub size: Option<u64>,
    /// Seconds since the Unix epoch.
    pub uploaded_at: i64,
    /// The author's note on this file (simple Markdown).
    pub description: String,
}

/// What we need to know about a mod to install it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModInfo {
    pub mod_id: u32,
    pub name: String,
    /// Raw description (BBCode): we look there for an official source of
    /// the author.
    pub description: String,
    pub author: String,
    pub version: String,
    pub thumbnail_url: Option<String>,
    pub files: Vec<ModFile>,
    pub requirements: Vec<Requirement>,
}

impl ModInfo {
    /// File to install in one click: the main one, else the one whose name
    /// says so ("Main File", next to the color variants), else the most
    /// recent of the "MAIN" files.
    pub fn main_file(&self) -> Option<&ModFile> {
        let main = self.files.iter().filter(|f| f.category == "MAIN");
        main.clone()
            .find(|f| f.primary)
            .or_else(|| {
                main.clone()
                    .filter(|f| f.name.to_lowercase().contains("main"))
                    .max_by_key(|f| f.uploaded_at)
            })
            .or_else(|| main.max_by_key(|f| f.uploaded_at))
    }

    /// Files to choose from (variants, options), the one to install by
    /// default first. For an old version of the game, each current file as
    /// it was before `before`.
    pub fn choices(&self, before: Option<&str>) -> Vec<&ModFile> {
        let main = self.main_file().map(|f| f.file_id);
        let mut current: Vec<&ModFile> = self
            .files
            .iter()
            .filter(|f| CURRENT.contains(&f.category.as_str()))
            .collect();
        // The main file, then by category, the most recent first.
        current.sort_by_key(|f| {
            let rank = CURRENT.iter().position(|c| *c == f.category);
            (
                Some(f.file_id) != main,
                rank,
                std::cmp::Reverse(f.uploaded_at),
            )
        });
        let mut list: Vec<&ModFile> = Vec::new();
        for file in current {
            let file = match before {
                Some(before) => self.as_of(file, before),
                None => Some(file),
            };
            if let Some(file) = file
                && !list.iter().any(|f| f.file_id == file.file_id)
            {
                list.push(file);
            }
        }
        list
    }

    /// The same file today (update, reinstall), or as it was before
    /// `before`: the most recent one with the same name, version aside. No
    /// file with that name anymore: this one as is, unless the author moved
    /// it to the old versions, then the main file.
    pub fn successor(&self, file_id: u32, before: Option<&str>) -> Option<&ModFile> {
        let old = self.files.iter().find(|f| f.file_id == file_id)?;
        let current = self
            .files
            .iter()
            .filter(|f| CURRENT.contains(&f.category.as_str()))
            .filter(|f| same_name(&f.name, &old.name))
            .max_by_key(|f| f.uploaded_at);
        match before {
            Some(before) => self.as_of(current.unwrap_or(old), before),
            // Reinstalling a replaced file would offer the update forever.
            None if !CURRENT.contains(&old.category.as_str()) => {
                current.or_else(|| self.main_file())
            }
            None => current,
        }
        .or(Some(old))
    }

    /// The most recent file named like `file` published before `before`.
    fn as_of<'a>(&'a self, file: &'a ModFile, before: &str) -> Option<&'a ModFile> {
        self.files
            .iter()
            .filter(|f| same_name(&f.name, &file.name))
            .filter(|f| iso_date(f.uploaded_at).as_str() < before)
            .max_by_key(|f| f.uploaded_at)
    }

    /// File to install for a game that stayed on a version released before
    /// `before` (ISO date): the main file as it was then. Once replaced, it
    /// moves to `OLD_VERSION` or `ARCHIVED`, usually under the same name.
    /// `None`: the mod didn't exist yet.
    pub fn main_file_before(&self, before: &str) -> Option<&ModFile> {
        let older: Vec<&ModFile> = self
            .files
            .iter()
            .filter(|f| matches!(f.category.as_str(), "MAIN" | "OLD_VERSION" | "ARCHIVED"))
            .filter(|f| iso_date(f.uploaded_at).as_str() < before)
            .collect();
        let newest = |pick: &dyn Fn(&ModFile) -> bool| {
            older
                .iter()
                .copied()
                .filter(|f| pick(f))
                .max_by_key(|f| f.uploaded_at)
        };
        let current = self.main_file().map(|f| f.name.as_str());
        newest(&|f| Some(f.name.as_str()) == current)
            .or_else(|| newest(&|f| f.category == "MAIN"))
            .or_else(|| newest(&|_| true))
    }

    /// File to install in one click, for the game version: before `before`
    /// if it is old, else the main one.
    pub fn file_for(&self, before: Option<&str>) -> Option<&ModFile> {
        before
            .and_then(|b| self.main_file_before(b))
            .or_else(|| self.main_file())
    }
}

/// Categories of a mod's current files, in display order. The others
/// (`OLD_VERSION`, `ARCHIVED`) are replaced versions.
const CURRENT: [&str; 4] = ["MAIN", "OPTIONAL", "UPDATE", "MISCELLANEOUS"];

/// Two file names for the same file, from one version to the next
/// (`WHITE_BLACK` became `White_Black`, `PBZSkipIntro 1.0.0` became
/// `PBZSkipIntro 1.1.0`).
fn same_name(a: &str, b: &str) -> bool {
    if a.trim().eq_ignore_ascii_case(b.trim()) {
        return true;
    }
    let (a, b) = (without_version(a), without_version(b));
    !a.is_empty() && a == b
}

/// A file name in lowercase, without the words that are a version (`1.1.0`,
/// `v2`): what stays the same from one upload to the next.
fn without_version(name: &str) -> String {
    name.to_lowercase()
        .split(|c: char| c.is_whitespace() || matches!(c, '-' | '_' | '(' | ')' | '[' | ']'))
        .filter(|word| {
            let number = word.strip_prefix('v').unwrap_or(word);
            !(number.starts_with(|c: char| c.is_ascii_digit())
                && number.chars().all(|c| c.is_ascii_digit() || c == '.'))
        })
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Requirement declared on Nexus.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Requirement {
    /// Nexus mod of the same game, else `None` (external link).
    pub mod_id: Option<u32>,
    pub name: String,
    pub url: Option<String>,
}

/// Sent with every request, as the API acceptable use policy asks: the same
/// name from one version to the next, and the current version.
const APPLICATION_NAME: &str = "Unalia Mod Manager";

#[derive(Debug, Clone)]
pub struct Nexus {
    client: Client,
    graphql_url: String,
    api_url: String,
    api_key: Arc<RwLock<Option<String>>>,
    /// HTTPS-only download links, outside tests.
    require_https: bool,
}

impl Nexus {
    pub fn new(client: Client) -> Self {
        Self::with_urls(client, GRAPHQL_URL, API_URL)
    }

    /// For tests: fake servers.
    pub fn with_urls(client: Client, graphql_url: &str, api_url: &str) -> Self {
        Self {
            client,
            graphql_url: graphql_url.to_owned(),
            api_url: api_url.trim_end_matches('/').to_owned(),
            api_key: Arc::default(),
            require_https: api_url.starts_with("https://"),
        }
    }

    pub fn set_api_key(&self, key: Option<String>) {
        *self.api_key.write().unwrap() = key.filter(|k| !k.trim().is_empty());
    }

    pub fn connected(&self) -> bool {
        self.api_key.read().unwrap().is_some()
    }

    /// Account tied to the API key (also checks the key).
    pub async fn account(&self) -> Result<Account, SourceError> {
        let user: User = self.rest("/users/validate.json").await?;
        Ok(Account {
            name: user.name,
            premium: user.is_premium,
            avatar_url: user.profile_url.filter(|u| u.starts_with("https://")),
        })
    }

    /// Nexus games with these names, in one request. Exact name, ignoring
    /// case, ™ and ® (`games::auto::normalize`): no fuzzy match, an
    /// ambiguous name returns nothing.
    pub async fn find_games(&self, names: &[String]) -> Result<Vec<Option<FoundGame>>, SourceError> {
        if names.is_empty() {
            return Ok(Vec::new());
        }
        let wanted: Vec<String> = names.iter().map(|n| crate::games::auto::normalize(n)).collect();
        let params: Vec<String> = (0..names.len()).map(|i| format!("$n{i}: String!")).collect();
        let fields: Vec<String> = (0..names.len())
            .map(|i| {
                format!(
                    "g{i}: games(filter: {{name: [{{value: $n{i}, op: MATCHES}}]}}, count: 5) \
                     {{ nodes {{ id name domainName }} }}"
                )
            })
            .collect();
        let query = format!("query({}) {{ {} }}", params.join(", "), fields.join(" "));
        let variables: serde_json::Map<String, serde_json::Value> = wanted
            .iter()
            .enumerate()
            .map(|(i, n)| (format!("n{i}"), json!(n)))
            .collect();
        let mut data: HashMap<String, GameNodes> = self.graphql(&query, variables.into()).await?;
        Ok(wanted
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let nodes = data.remove(&format!("g{i}"))?.nodes;
                let mut exact = nodes
                    .into_iter()
                    .filter(|g| crate::games::auto::normalize(&g.name) == *name);
                let first = exact.next()?;
                exact.next().is_none().then_some(FoundGame {
                    name: first.name,
                    domain: first.domain_name,
                    game_id: first.id,
                })
            })
            .collect())
    }

    /// Install sheet of a mod: name, version, files, requirements.
    pub async fn mod_info(&self, domain: &str, mod_id: u32) -> Result<ModInfo, SourceError> {
        let node = self.node(domain, mod_id).await?;
        let files = self.files(mod_id, node.game_id).await?;
        Ok(ModInfo {
            mod_id,
            name: node.name.clone(),
            description: node.description.clone().unwrap_or_default(),
            author: node.author.clone().unwrap_or_default(),
            version: node.version.clone().unwrap_or_default(),
            thumbnail_url: node.thumbnail_url.clone(),
            requirements: node.requirements(),
            files,
        })
    }

    /// Latest published version of each mod, in one request.
    pub async fn latest_versions(
        &self,
        domain: &str,
        mod_ids: &[u32],
    ) -> Result<HashMap<u32, String>, SourceError> {
        if mod_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let ids: Vec<_> = mod_ids
            .iter()
            .map(|id| json!({ "gameDomain": domain, "modId": id }))
            .collect();
        let data: LegacyData = self
            .graphql(
                "query($ids: [CompositeDomainWithIdInput!]!) {
                    legacyModsByDomain(ids: $ids) { nodes { modId version } }
                }",
                json!({ "ids": ids }),
            )
            .await?;
        Ok(data
            .legacy_mods_by_domain
            .nodes
            .into_iter()
            .filter_map(|n| Some((n.mod_id, n.version?)))
            .collect())
    }

    /// Download URL of a file. Without `link` (the site's `nxm://` link),
    /// only a Premium account is allowed.
    pub async fn download_url(
        &self,
        domain: &str,
        mod_id: u32,
        file_id: u32,
        link: Option<&NxmLink>,
    ) -> Result<String, SourceError> {
        let mut path = format!("/games/{domain}/mods/{mod_id}/files/{file_id}/download_link.json");
        if let Some(NxmLink {
            key: Some(key),
            expires: Some(expires),
            ..
        }) = link
        {
            path.push_str(&format!("?key={key}&expires={expires}"));
        }
        let links: Vec<CdnLink> = self.rest(&path).await?;
        links
            .into_iter()
            .map(|l| l.uri)
            .find(|u| !self.require_https || u.starts_with("https://"))
            .ok_or(SourceError::Unexpected {
                source_name: NAME,
                detail: "no download link".into(),
            })
    }

    /// Nexus files identical, byte for byte, to the file with hash `md5`:
    /// (mod, file).
    pub async fn md5_search(
        &self,
        domain: &str,
        md5: &str,
    ) -> Result<Vec<(u32, u32)>, SourceError> {
        if md5.len() != 32 || !md5.chars().all(|c| c.is_ascii_hexdigit()) {
            return Ok(Vec::new());
        }
        match self
            .rest::<Vec<Md5Match>>(&format!("/games/{domain}/mods/md5_search/{md5}.json"))
            .await
        {
            Ok(matches) => Ok(matches
                .into_iter()
                .map(|m| (m.r#mod.mod_id, m.file_details.file_id))
                .collect()),
            Err(SourceError::NotFound { .. }) => Ok(Vec::new()),
            Err(error) => Err(error),
        }
    }

    /// The author's donation link: the one in their description (Ko-fi,
    /// Patreon…), else their PayPal if Nexus makes it public. Never
    /// blocking: no answer, no button.
    async fn donation_url(&self, domain: &str, mod_id: u32, description: &str) -> Option<String> {
        if let Some(url) = donation_link_in(description) {
            return Some(url);
        }
        let data: DonationData = self
            .graphql(
                "query($ids: [CompositeDomainWithIdInput!]!) {
                    legacyModsByDomain(ids: $ids) { nodes { uploader { paypal donationsEnabled } } }
                }",
                json!({ "ids": [{ "gameDomain": domain, "modId": mod_id }] }),
            )
            .await
            .ok()?;
        let uploader = data
            .legacy_mods_by_domain
            .nodes
            .into_iter()
            .next()?
            .uploader?;
        let paypal = uploader.paypal.filter(|_| uploader.donations_enabled)?;
        let email = paypal.trim();
        let plain = email.contains('@')
            && email
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '@' | '.' | '-' | '_' | '+'));
        plain.then(|| {
            format!(
                "https://www.paypal.com/donate/?business={}",
                email.replace('+', "%2B").replace('@', "%40")
            )
        })
    }

    async fn node(&self, domain: &str, mod_id: u32) -> Result<ModNode, SourceError> {
        let data: LegacyData = self
            .graphql(
                &format!(
                    "query($ids: [CompositeDomainWithIdInput!]!) {{
                        legacyModsByDomain(ids: $ids) {{ nodes {{ {MOD_FIELDS} gameId description
                            modRequirements {{ nexusRequirements {{ nodes {{
                                modId modName gameId externalRequirement url
                            }} }} }}
                        }} }}
                    }}"
                ),
                json!({ "ids": [{ "gameDomain": domain, "modId": mod_id }] }),
            )
            .await?;
        data.legacy_mods_by_domain
            .nodes
            .into_iter()
            .next()
            .ok_or(SourceError::NotFound { source_name: NAME })
    }

    async fn files(&self, mod_id: u32, game_id: Option<u32>) -> Result<Vec<ModFile>, SourceError> {
        let Some(game_id) = game_id else {
            return Ok(Vec::new());
        };
        let data: FilesData = self
            .graphql(
                "query($mod: ID!, $game: ID!) {
                    modFiles(modId: $mod, gameId: $game) {
                        fileId name version category primary sizeInBytes date description
                    }
                }",
                json!({ "mod": mod_id.to_string(), "game": game_id.to_string() }),
            )
            .await?;
        let mut files: Vec<ModFile> = data
            .mod_files
            .into_iter()
            .map(|f| ModFile {
                file_id: f.file_id,
                name: f.name,
                version: f.version.unwrap_or_default(),
                category: f.category,
                primary: f.primary == 1,
                size: f.size_in_bytes.and_then(|s| s.parse().ok()),
                uploaded_at: f.date,
                description: bbcode_to_markdown(f.description.as_deref().unwrap_or_default())
                    .trim()
                    .to_owned(),
            })
            .collect();
        files.sort_by_key(|f| std::cmp::Reverse(f.uploaded_at));
        Ok(files)
    }

    async fn graphql<T: DeserializeOwned>(
        &self,
        query: &str,
        variables: serde_json::Value,
    ) -> Result<T, SourceError> {
        let response = self
            .client
            .post(&self.graphql_url)
            .header("Application-Name", APPLICATION_NAME)
            .header("Application-Version", env!("CARGO_PKG_VERSION"))
            .json(&json!({ "query": query, "variables": variables }))
            .send()
            .await
            .map_err(network)?;
        let body: GraphqlResponse<T> = parse(response).await?;
        if let Some(error) = body.errors.into_iter().next() {
            return Err(SourceError::Unexpected {
                source_name: NAME,
                detail: error.message,
            });
        }
        body.data.ok_or(SourceError::Unexpected {
            source_name: NAME,
            detail: "empty response".into(),
        })
    }

    async fn rest<T: DeserializeOwned>(&self, path: &str) -> Result<T, SourceError> {
        let key = self
            .api_key
            .read()
            .unwrap()
            .clone()
            .ok_or(SourceError::NotConnected { source_name: NAME })?;
        let response = self
            .client
            .get(format!("{}{path}", self.api_url))
            .header("apikey", key)
            .header("Application-Name", APPLICATION_NAME)
            .header("Application-Version", env!("CARGO_PKG_VERSION"))
            .send()
            .await
            .map_err(network)?;
        match response.status() {
            StatusCode::UNAUTHORIZED => Err(SourceError::Unauthorized { source_name: NAME }),
            StatusCode::FORBIDDEN => Err(SourceError::WebsiteOnly { source_name: NAME }),
            StatusCode::NOT_FOUND => Err(SourceError::NotFound { source_name: NAME }),
            StatusCode::TOO_MANY_REQUESTS => Err(SourceError::Unexpected {
                source_name: NAME,
                detail: "hourly request limit reached, try again later".into(),
            }),
            _ => parse(response).await,
        }
    }
}

fn network(error: reqwest::Error) -> SourceError {
    SourceError::Network {
        source_name: NAME,
        error,
    }
}

async fn parse<T: DeserializeOwned>(response: reqwest::Response) -> Result<T, SourceError> {
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

const MOD_FIELDS: &str = "modId name summary author thumbnailUrl thumbnailLargeUrl pictureUrl downloads \
    updatedAt createdAt version adultContent status modCategory { name }";

impl ModSource for Nexus {
    fn name(&self) -> &'static str {
        NAME
    }

    async fn search(
        &self,
        game: &str,
        query: &SearchQuery,
    ) -> Result<Page<ModSummary>, SourceError> {
        let domain = domain_of(game).ok_or(SourceError::NotFound { source_name: NAME })?;
        let mut filters = vec![
            json!({ "gameDomainName": [{ "value": domain, "op": "EQUALS" }] }),
            json!({ "adultContent": [{ "value": false, "op": "EQUALS" }] }),
        ];
        let text = query.text.trim();
        if !text.is_empty() {
            filters.push(json!({ "nameStemmed": [{ "value": text, "op": "MATCHES" }] }));
        } else if !query.include_libraries {
            filters.push(json!({ "categoryName": [{ "value": LIBRARIES, "op": "NOT_EQUALS" }] }));
        }
        let sort = match (query.sort, text.is_empty()) {
            (Sort::Popular, false) => json!({ "relevance": { "direction": "DESC" } }),
            (Sort::Popular, true) => json!({ "endorsements": { "direction": "DESC" } }),
            (Sort::Downloads, _) => json!({ "downloads": { "direction": "DESC" } }),
            (Sort::Newest, _) => json!({ "createdAt": { "direction": "DESC" } }),
            (Sort::Updated, _) => json!({ "updatedAt": { "direction": "DESC" } }),
        };
        let page = query.page.max(1);
        let data: ModsData = self
            .graphql(
                &format!(
                    "query($filter: ModsFilter, $sort: [ModsSort!], $offset: Int, $count: Int) {{
                        mods(filter: $filter, sort: $sort, offset: $offset, count: $count) {{
                            totalCount nodes {{ {MOD_FIELDS} }}
                        }}
                    }}"
                ),
                json!({
                    "filter": { "op": "AND", "filter": filters },
                    "sort": [sort],
                    "offset": (page - 1) * PAGE_SIZE,
                    "count": PAGE_SIZE,
                }),
            )
            .await?;
        let total = data.mods.total_count;
        Ok(Page {
            has_more: u64::from(page * PAGE_SIZE) < total,
            total,
            items: data.mods.nodes.into_iter().map(ModNode::summary).collect(),
        })
    }

    async fn get(&self, game: &str, id: &str) -> Result<Mod, SourceError> {
        let not_found = SourceError::NotFound { source_name: NAME };
        let domain = domain_of(game).ok_or(SourceError::NotFound { source_name: NAME })?;
        let mod_id = parse_mod_key(id).ok_or(not_found)?;
        let node = self.node(domain, mod_id).await?;
        let (files, donation_url) = tokio::join!(
            self.files(mod_id, node.game_id),
            self.donation_url(
                domain,
                mod_id,
                node.description.as_deref().unwrap_or_default()
            ),
        );
        let files = files?;
        let dependencies: Vec<Dependency> = node
            .requirements()
            .into_iter()
            .filter_map(|r| {
                Some(Dependency {
                    id: mod_key(r.mod_id?),
                    min_version: String::new(),
                })
            })
            .collect();
        let versions = files
            .iter()
            .filter(|f| f.category == "MAIN")
            .map(|f| ModVersion {
                number: f.version.clone(),
                published_at: iso_date(f.uploaded_at),
                // Resolved at install time: it depends on the account.
                download_url: String::new(),
                sha256: None,
                size: f.size,
                dependencies: dependencies.clone(),
            })
            .collect();
        Ok(Mod {
            id: id.to_owned(),
            source: SOURCE_ID,
            source_url: format!("https://www.nexusmods.com/{domain}/mods/{mod_id}"),
            name: node.name,
            author: node.author.unwrap_or_default(),
            summary: node.summary.unwrap_or_default(),
            images: node.picture_url.into_iter().collect(),
            categories: node.mod_category.map(|c| c.name).into_iter().collect(),
            downloads: node.downloads,
            updated_at: node.updated_at.unwrap_or_default(),
            website_url: None,
            donation_url,
            deprecated: node.status.as_deref().is_some_and(|s| s != "published"),
            versions,
        })
    }

    async fn description(
        &self,
        game: &str,
        id: &str,
        _version: &str,
    ) -> Result<String, SourceError> {
        let domain = domain_of(game).ok_or(SourceError::NotFound { source_name: NAME })?;
        let mod_id = parse_mod_key(id).ok_or(SourceError::NotFound { source_name: NAME })?;
        let data: DescriptionData = self
            .graphql(
                "query($ids: [CompositeDomainWithIdInput!]!) {
                    legacyModsByDomain(ids: $ids) { nodes { description } }
                }",
                json!({ "ids": [{ "gameDomain": domain, "modId": mod_id }] }),
            )
            .await?;
        let text = data
            .legacy_mods_by_domain
            .nodes
            .into_iter()
            .next()
            .and_then(|n| n.description)
            .unwrap_or_default();
        Ok(bbcode_to_markdown(&text))
    }
}

/// Donation sites recognized in a description.
const DONATION_HOSTS: [&str; 8] = [
    "ko-fi.com/",
    "patreon.com/",
    "buymeacoffee.com/",
    "paypal.me/",
    "paypal.com/donate",
    "paypal.com/paypalme/",
    "github.com/sponsors/",
    "liberapay.com/",
];

/// First HTTPS donation link of a description, e.g.
/// `https://ko-fi.com/author`.
pub fn donation_link_in(text: &str) -> Option<String> {
    text.match_indices("https://").find_map(|(start, _)| {
        let url: String = text[start..]
            .chars()
            .take_while(|c| {
                !c.is_whitespace() && !matches!(c, '"' | '\'' | '<' | '>' | ']' | '[' | ')' | '(')
            })
            .collect();
        let rest = url
            .trim_start_matches("https://")
            .trim_start_matches("www.");
        let host_ok = DONATION_HOSTS
            .iter()
            .any(|h| rest.to_ascii_lowercase().starts_with(h) && rest.len() > h.len());
        host_ok.then(|| url.trim_end_matches(['.', ',', ';']).to_owned())
    })
}

/// Unix seconds to ISO 8601 date (UTC), without a calendar dependency.
pub(crate) fn iso_date(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Algorithme de Howard Hinnant (civil_from_days).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Nexus descriptions are BBCode, with `<br />`. We translate them into
/// simple Markdown; the UI then sanitizes them like any description.
/// Unknown tags disappear, their text stays.
pub fn bbcode_to_markdown(input: &str) -> String {
    let text = input
        .replace("<br />", "\n")
        .replace("<br/>", "\n")
        .replace("<br>", "\n")
        .replace("\r", "");
    let mut out = String::with_capacity(text.len());
    let mut rest = text.as_str();
    // Stack of open links: `[url=…]text[/url]` becomes `[text](…)`.
    let mut links: Vec<String> = Vec::new();
    while let Some(start) = rest.find('[') {
        out.push_str(&rest[..start]);
        let Some(len) = rest[start..].find(']') else {
            out.push_str(&rest[start..]);
            rest = "";
            break;
        };
        let tag = &rest[start + 1..start + len];
        rest = &rest[start + len + 1..];
        let (name, value) = tag.split_once('=').unwrap_or((tag, ""));
        let value = value.trim_matches(['"', '\'']);
        match name.to_ascii_lowercase().as_str() {
            "b" | "/b" => out.push_str("**"),
            "i" | "/i" => out.push('*'),
            "s" | "/s" => out.push_str("~~"),
            "*" => out.push_str("\n- "),
            "list" | "/list" | "ul" | "/ul" | "ol" | "/ol" => out.push('\n'),
            "quote" => out.push_str("\n> "),
            "/quote" => out.push('\n'),
            "line" | "hr" => out.push_str("\n\n---\n\n"),
            "heading" => out.push_str("\n### "),
            "/heading" => out.push('\n'),
            "code" => out.push_str("\n```\n"),
            "/code" => out.push_str("\n```\n"),
            "url" => {
                links.push(value.to_owned());
                out.push('[');
            }
            "/url" => {
                let href = links.pop().unwrap_or_default();
                out.push_str(&format!("]({href})"));
            }
            "img" => {
                if let Some(end) = rest.to_ascii_lowercase().find("[/img]") {
                    out.push_str(&format!("![]({})", rest[..end].trim()));
                    rest = &rest[end + "[/img]".len()..];
                }
            }
            "youtube" => {
                if let Some(end) = rest.to_ascii_lowercase().find("[/youtube]") {
                    let id = rest[..end].trim();
                    out.push_str(&format!("[YouTube](https://www.youtube.com/watch?v={id})"));
                    rest = &rest[end + "[/youtube]".len()..];
                }
            }
            // Formatting with no equivalent: keep the text.
            "size" | "/size" | "color" | "/color" | "font" | "/font" | "center" | "/center"
            | "right" | "/right" | "left" | "/left" | "u" | "/u" | "spoiler" | "/spoiler"
            | "/youtube" | "/img" | "/*" => {}
            // Not a tag (e.g. "[v1.2]"): text as is.
            _ => {
                out.push('[');
                out.push_str(tag);
                out.push(']');
            }
        }
    }
    out.push_str(rest);
    out
}

// ---------------------------------------------------------------- responses

#[derive(Deserialize)]
struct GraphqlResponse<T> {
    data: Option<T>,
    #[serde(default)]
    errors: Vec<GraphqlError>,
}

#[derive(Deserialize)]
struct GraphqlError {
    message: String,
}

#[derive(Deserialize)]
struct GameNodes {
    nodes: Vec<GameNode>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GameNode {
    id: u32,
    name: String,
    domain_name: String,
}

#[derive(Deserialize)]
struct ModsData {
    mods: ModsPage,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModsPage {
    total_count: u64,
    nodes: Vec<ModNode>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyData {
    legacy_mods_by_domain: LegacyPage,
}

#[derive(Deserialize)]
struct LegacyPage {
    nodes: Vec<ModNode>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DescriptionData {
    legacy_mods_by_domain: DescriptionPage,
}

#[derive(Deserialize)]
struct DescriptionPage {
    nodes: Vec<DescriptionNode>,
}

#[derive(Deserialize)]
struct DescriptionNode {
    description: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModNode {
    mod_id: u32,
    #[serde(default)]
    name: String,
    summary: Option<String>,
    author: Option<String>,
    thumbnail_url: Option<String>,
    /// 622 × 349 (the plain thumbnail is 385 × 216): sharp on the cards.
    #[serde(default)]
    thumbnail_large_url: Option<String>,
    picture_url: Option<String>,
    downloads: Option<u64>,
    updated_at: Option<String>,
    version: Option<String>,
    status: Option<String>,
    mod_category: Option<Category>,
    game_id: Option<u32>,
    description: Option<String>,
    mod_requirements: Option<Requirements>,
}

impl ModNode {
    fn summary(self) -> ModSummary {
        ModSummary {
            id: mod_key(self.mod_id),
            source: SOURCE_ID,
            name: self.name,
            author: self.author.unwrap_or_default(),
            summary: self.summary.unwrap_or_default(),
            icon_url: self.thumbnail_large_url.or(self.thumbnail_url),
            categories: self.mod_category.map(|c| c.name).into_iter().collect(),
            downloads: self.downloads.unwrap_or_default(),
            updated_at: self.updated_at.unwrap_or_default(),
            pinned: false,
            deprecated: false,
        }
    }

    fn requirements(&self) -> Vec<Requirement> {
        let Some(requirements) = &self.mod_requirements else {
            return Vec::new();
        };
        requirements
            .nexus_requirements
            .nodes
            .iter()
            .map(|r| {
                let same_game = r.game_id.as_deref().and_then(|g| g.parse().ok()) == self.game_id;
                Requirement {
                    mod_id: (!r.external_requirement && same_game)
                        .then(|| r.mod_id.as_deref()?.parse().ok())
                        .flatten(),
                    name: r.mod_name.clone().unwrap_or_default(),
                    url: r.url.clone().filter(|u| u.starts_with("https://")),
                }
            })
            .collect()
    }
}

#[derive(Deserialize)]
struct Category {
    name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Requirements {
    nexus_requirements: RequirementPage,
}

#[derive(Deserialize)]
struct RequirementPage {
    nodes: Vec<RawRequirement>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawRequirement {
    mod_id: Option<String>,
    mod_name: Option<String>,
    game_id: Option<String>,
    #[serde(default)]
    external_requirement: bool,
    url: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FilesData {
    mod_files: Vec<RawFile>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawFile {
    file_id: u32,
    name: String,
    version: Option<String>,
    category: String,
    #[serde(default)]
    primary: i64,
    size_in_bytes: Option<String>,
    #[serde(default)]
    date: i64,
    description: Option<String>,
}

#[derive(Deserialize)]
struct User {
    name: String,
    #[serde(default)]
    is_premium: bool,
    /// Despite its name, the avatar URL.
    profile_url: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DonationData {
    legacy_mods_by_domain: DonationPage,
}

#[derive(Deserialize)]
struct DonationPage {
    nodes: Vec<DonationNode>,
}

#[derive(Deserialize)]
struct DonationNode {
    uploader: Option<Uploader>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Uploader {
    paypal: Option<String>,
    #[serde(default)]
    donations_enabled: bool,
}

#[derive(Deserialize)]
struct Md5Match {
    r#mod: Md5Mod,
    file_details: Md5File,
}

#[derive(Deserialize)]
struct Md5Mod {
    mod_id: u32,
}

#[derive(Deserialize)]
struct Md5File {
    file_id: u32,
}

#[derive(Deserialize)]
struct CdnLink {
    #[serde(rename = "URI")]
    uri: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nxm_links() {
        let link = NxmLink::parse(
            "nxm://cyberpunk2077/mods/4198/files/167253?key=AbC-12_x&expires=1792500000&user_id=1",
        )
        .unwrap();
        assert_eq!(link.domain, "cyberpunk2077");
        assert_eq!((link.mod_id, link.file_id), (4198, 167253));
        assert_eq!(link.key.as_deref(), Some("AbC-12_x"));
        assert_eq!(link.expires, Some(1792500000));

        let premium = NxmLink::parse("nxm://CyberPunk2077/mods/1/files/2").unwrap();
        assert_eq!(premium.domain, "cyberpunk2077");
        assert_eq!(premium.key, None);

        for bad in [
            "https://cyberpunk2077/mods/1/files/2",
            "nxm://cyberpunk2077/mods/x/files/2",
            "nxm://../mods/1/files/2",
            "nxm://cyberpunk2077/collections/abc",
        ] {
            assert_eq!(NxmLink::parse(bad), None, "{bad}");
        }
        // A key that could break the URL is ignored.
        let odd = NxmLink::parse("nxm://cyberpunk2077/mods/1/files/2?key=a%26b&expires=3").unwrap();
        assert_eq!(odd.key, None);
    }

    #[test]
    fn converts_bbcode() {
        let md = bbcode_to_markdown(
            "[size=5][b]Title[/b][/size]<br />See [url=https://x.io]the docs[/url].<br />[list][*]one[*]two[/*][/list][img]https://i.io/a.png[/img] [v1.2]",
        );
        assert!(md.contains("**Title**"), "{md}");
        assert!(md.contains("[the docs](https://x.io)"), "{md}");
        assert!(md.contains("- one\n- two"), "{md}");
        // Closed list items (`[/*]`) leave nothing behind.
        assert!(!md.contains("[/*]"), "{md}");
        assert!(md.contains("![](https://i.io/a.png)"), "{md}");
        assert!(md.contains("[v1.2]"), "{md}");
    }

    #[test]
    fn finds_donation_links() {
        assert_eq!(
            donation_link_in(
                "Support me: [url=https://ko-fi.com/author]Ko-fi[/url] or https://github.com/a/b"
            )
            .as_deref(),
            Some("https://ko-fi.com/author")
        );
        assert_eq!(
            donation_link_in("https://www.patreon.com/someone.").as_deref(),
            Some("https://www.patreon.com/someone")
        );
        assert_eq!(
            donation_link_in("http://ko-fi.com/insecure https://ko-fi.com/"),
            None
        );
    }

    #[test]
    fn formats_dates() {
        assert_eq!(iso_date(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_date(1_792_493_526), "2026-10-20T10:52:06Z");
    }

    #[test]
    fn main_file_prefers_the_primary_one() {
        let file = |id, category: &str, primary, at| ModFile {
            file_id: id,
            name: String::new(),
            version: String::new(),
            category: category.into(),
            primary,
            size: None,
            uploaded_at: at,
            description: String::new(),
        };
        let mut info = ModInfo {
            mod_id: 1,
            name: String::new(),
            description: String::new(),
            author: String::new(),
            version: String::new(),
            thumbnail_url: None,
            files: vec![
                file(1, "MAIN", false, 30),
                file(2, "MAIN", true, 10),
                file(3, "OPTIONAL", false, 50),
            ],
            requirements: Vec::new(),
        };
        assert_eq!(info.main_file().unwrap().file_id, 2);
        info.files[1].primary = false;
        assert_eq!(info.main_file().unwrap().file_id, 1);
    }

    #[test]
    fn an_older_game_gets_the_file_of_its_time() {
        // 2026-10-20 and 2026-10-23, on either side of 2.5.
        let (before, after) = (1_792_493_526, 1_792_752_726);
        let file = |id, name: &str, category: &str, at| ModFile {
            file_id: id,
            name: name.into(),
            version: String::new(),
            category: category.into(),
            primary: false,
            size: None,
            uploaded_at: at,
            description: String::new(),
        };
        let info = |files| ModInfo {
            mod_id: 1,
            name: String::new(),
            description: String::new(),
            author: String::new(),
            version: String::new(),
            thumbnail_url: None,
            files,
            requirements: Vec::new(),
        };
        let mod_ = info(vec![
            file(1, "Main", "MAIN", after),
            file(2, "Main", "OLD_VERSION", before),
            file(3, "Old extra", "OLD_VERSION", before + 60),
            file(4, "Patch", "OPTIONAL", before - 60),
        ]);
        assert_eq!(mod_.file_for(Some("2026-10-22")).unwrap().file_id, 2);
        assert_eq!(mod_.file_for(None).unwrap().file_id, 1);
        // Renamed at each version: the most recent one from before.
        let renamed = info(vec![
            file(1, "Main v2", "MAIN", after),
            file(2, "Main v1", "ARCHIVED", before),
        ]);
        assert_eq!(renamed.file_for(Some("2026-10-22")).unwrap().file_id, 2);
        // Published after: nothing older, the current file.
        let new = info(vec![file(1, "Main", "MAIN", after)]);
        assert_eq!(new.file_for(Some("2026-10-22")).unwrap().file_id, 1);
    }

    #[test]
    fn an_update_finds_the_file_renamed_with_its_version() {
        let file = |id, name: &str, category: &str, at| ModFile {
            file_id: id,
            name: name.into(),
            version: String::new(),
            category: category.into(),
            primary: false,
            size: None,
            uploaded_at: at,
            description: String::new(),
        };
        let info = |files| ModInfo {
            mod_id: 12,
            name: String::new(),
            description: String::new(),
            author: String::new(),
            version: String::new(),
            thumbnail_url: None,
            files,
            requirements: Vec::new(),
        };
        // Skip Intro on Phantom Blade Zero: the version is in the name.
        let skip = info(vec![
            file(9, "PBZSkipIntro 1.0.0", "OLD_VERSION", 1),
            file(94, "PBZSkipIntro 1.1.0", "MAIN", 2),
        ]);
        assert_eq!(skip.successor(9, None).unwrap().file_id, 94);
        // Variants keep their own file.
        let variants = info(vec![
            file(1, "Dark v1", "OLD_VERSION", 1),
            file(2, "Light v1", "OLD_VERSION", 1),
            file(3, "Dark v2", "MAIN", 2),
            file(4, "Light v2", "OPTIONAL", 2),
        ]);
        assert_eq!(variants.successor(2, None).unwrap().file_id, 4);
        // Renamed for good and moved to the old versions: the main file.
        let renamed = info(vec![
            file(1, "Old name", "OLD_VERSION", 1),
            file(2, "New name", "MAIN", 2),
        ]);
        assert_eq!(renamed.successor(1, None).unwrap().file_id, 2);
        // Still current under another name: kept as is.
        let optional = info(vec![
            file(1, "Main", "MAIN", 2),
            file(2, "Extra", "OPTIONAL", 1),
        ]);
        assert_eq!(optional.successor(2, None).unwrap().file_id, 2);
        // Names made only of a version are compared as they are.
        assert!(!same_name("1.0", "2.0"));
        assert!(same_name("Mod_v1.2", "mod v1.3"));
    }
}

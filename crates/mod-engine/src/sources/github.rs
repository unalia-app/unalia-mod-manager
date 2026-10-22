//! Releases **GitHub** : source officielle des auteurs qui y publient
//! (frameworks de Cyberpunk 2077). Gratuit, sans compte, sans bridage : on
//! la préfère à Nexus quand elle existe (échelle de sources, SPEC §2.4).

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::header::{ETAG, IF_NONE_MATCH};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};

use super::SourceError;

pub const API_URL: &str = "https://api.github.com";
const NAME: &str = "GitHub";

/// Dépôts GitHub cités dans un texte (description d'un mod), sans doublon,
/// dans l'ordre d'apparition : `owner/repo`.
pub fn repos_in(text: &str) -> Vec<String> {
    let mut repos: Vec<String> = Vec::new();
    for (start, _) in text.match_indices("github.com/") {
        let rest = &text[start + "github.com/".len()..];
        let mut parts = rest.split('/');
        let (Some(owner), Some(repo)) = (parts.next(), parts.next()) else {
            continue;
        };
        let name = |s: &str| -> String {
            s.chars()
                .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
                .collect()
        };
        let (owner, repo) = (name(owner), name(repo));
        let repo = repo
            .trim_end_matches(".git")
            .trim_end_matches('.')
            .to_owned();
        if owner.is_empty() || repo.is_empty() || owner.contains("..") || repo.contains("..") {
            continue;
        }
        let full = format!("{owner}/{repo}");
        if !repos.iter().any(|r| r.eq_ignore_ascii_case(&full)) {
            repos.push(full);
        }
    }
    repos
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    /// Version sans le `v` du tag, ex. `1.30.0`.
    pub version: String,
    pub published_at: String,
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asset {
    pub name: String,
    #[serde(rename = "browser_download_url")]
    pub url: String,
    pub size: u64,
}

impl Release {
    /// Fichier de la release d'après son modèle, `{version}` remplacé. Les
    /// auteurs changent parfois la façon d'écrire la version dans le nom
    /// (CET 1.38 : `cet_1_38_0.zip` après `cet_1.37.1.zip`) : points, tirets
    /// bas et tirets sont acceptés.
    pub fn asset(&self, pattern: &str) -> Option<&Asset> {
        self.asset_and_version(pattern).map(|(asset, _)| asset)
    }

    /// Le fichier et la version qu'il porte. Avec un `*` dans le modèle
    /// (`UE4SS_v*.zip`), la partie variable du nom est la version : une
    /// release « experimental-latest » n'a pas de numéro dans son tag.
    pub fn asset_and_version(&self, pattern: &str) -> Option<(&Asset, String)> {
        if let Some((prefix, suffix)) = pattern.split_once('*') {
            return self.assets.iter().find_map(|a| {
                let name = a.name.as_str();
                let middle = name.strip_prefix(prefix)?.strip_suffix(suffix)?;
                (!middle.is_empty() && !middle.contains('/'))
                    .then(|| (a, middle.trim_start_matches('v').to_owned()))
            });
        }
        let asset = ['.', '_', '-']
            .iter()
            .map(|sep| pattern.replace("{version}", &self.version.replace('.', &sep.to_string())))
            .find_map(|name| {
                self.assets
                    .iter()
                    .find(|a| a.name.eq_ignore_ascii_case(&name))
            })?;
        Some((asset, self.version.clone()))
    }
}

/// Réponse gardée pour un dépôt : la release (ou `None` : pas de release),
/// et son ETag pour redemander sans tout recevoir.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Cached {
    release: Option<Release>,
    etag: Option<String>,
    /// Secondes Unix de la dernière réponse.
    fetched_at: u64,
}

/// Client des releases GitHub. Sans compte, GitHub n'accorde que 60
/// requêtes par heure et par adresse IP : chaque réponse est gardée
/// (`ttl`), redemandée avec son ETag (une réponse « inchangé » ne renvoie
/// rien), et resservie si la limite est atteinte. Le cache peut vivre sur
/// disque, pour survivre aux redémarrages.
#[derive(Debug, Clone)]
pub struct GitHub {
    client: Client,
    api_url: String,
    /// Fichiers en HTTPS seulement, hors des tests.
    require_https: bool,
    ttl: Duration,
    cache: Arc<Mutex<HashMap<String, Cached>>>,
    cache_file: Option<PathBuf>,
    /// Jeton personnel facultatif : 5 000 requêtes par heure au lieu de 60.
    token: Option<String>,
}

impl GitHub {
    pub fn new(client: Client) -> Self {
        Self::with_url(client, API_URL)
    }

    pub fn with_url(client: Client, api_url: &str) -> Self {
        Self {
            client,
            api_url: api_url.trim_end_matches('/').to_owned(),
            require_https: api_url.starts_with("https://"),
            ttl: Duration::from_secs(10 * 60),
            cache: Arc::default(),
            cache_file: None,
            token: None,
        }
    }

    pub fn with_token(mut self, token: Option<String>) -> Self {
        self.token = token.filter(|t| !t.trim().is_empty());
        self
    }

    /// Durée pendant laquelle une réponse est resservie sans redemander.
    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }

    /// Cache sur disque : relu tout de suite, réécrit à chaque réponse.
    pub fn with_cache_file(mut self, file: PathBuf) -> Self {
        if let Some(saved) = fs::read(&file)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<HashMap<String, Cached>>(&bytes).ok())
        {
            *self.cache.lock().unwrap() = saved;
        }
        self.cache_file = Some(file);
        self
    }

    /// Dernière release publiée (hors préversions) de `owner/repo`.
    pub async fn latest_release(&self, repo: &str) -> Result<Release, SourceError> {
        self.release(repo, None).await
    }

    /// Release `tag` de `owner/repo`, sinon la dernière stable.
    pub async fn release(&self, repo: &str, tag: Option<&str>) -> Result<Release, SourceError> {
        let safe = |s: &str| {
            !s.is_empty()
                && !s.contains("..")
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.'))
        };
        let valid = repo.split('/').count() == 2
            && safe(repo)
            && tag.is_none_or(|t| safe(t) && !t.contains('/'));
        if !valid {
            return Err(SourceError::NotFound { source_name: NAME });
        }
        let key = match tag {
            Some(tag) => format!("{}@{tag}", repo.to_ascii_lowercase()),
            None => repo.to_ascii_lowercase(),
        };
        let cached = self.cache.lock().unwrap().get(&key).cloned();
        if let Some(cached) = &cached
            && now().saturating_sub(cached.fetched_at) < self.ttl.as_secs()
        {
            return found(cached.release.clone());
        }

        let network = |error| SourceError::Network {
            source_name: NAME,
            error,
        };
        let mut request = self
            .client
            .get(match tag {
                Some(tag) => format!("{}/repos/{repo}/releases/tags/{tag}", self.api_url),
                None => format!("{}/repos/{repo}/releases/latest", self.api_url),
            })
            .header("Accept", "application/vnd.github+json");
        if let Some(etag) = cached.as_ref().and_then(|c| c.etag.as_deref()) {
            request = request.header(IF_NONE_MATCH, etag);
        }
        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }
        let response = match request.send().await {
            Ok(response) => response,
            // Hors ligne : la dernière réponse connue vaut mieux que rien.
            Err(error) => {
                return match cached {
                    Some(cached) => found(cached.release),
                    None => Err(network(error)),
                };
            }
        };
        let status = response.status();
        if let Some(minutes) = rate_limited(&response) {
            return match cached {
                Some(cached) => found(cached.release),
                None => Err(SourceError::RateLimited {
                    source_name: NAME,
                    minutes,
                }),
            };
        }
        let etag = response
            .headers()
            .get(ETAG)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let release = if status == StatusCode::NOT_MODIFIED {
            cached.and_then(|c| c.release)
        } else if status == StatusCode::NOT_FOUND {
            None
        } else {
            let body = response
                .error_for_status()
                .map_err(network)?
                .text()
                .await
                .map_err(network)?;
            let raw: RawRelease =
                serde_json::from_str(&body).map_err(|e| SourceError::Unexpected {
                    source_name: NAME,
                    detail: e.to_string(),
                })?;
            Some(Release {
                version: raw.tag_name.trim_start_matches('v').to_owned(),
                published_at: raw.published_at.unwrap_or_default(),
                assets: raw
                    .assets
                    .into_iter()
                    .filter(|a| !self.require_https || a.url.starts_with("https://"))
                    .collect(),
            })
        };
        self.remember(
            key,
            Cached {
                release: release.clone(),
                etag,
                fetched_at: now(),
            },
        );
        found(release)
    }

    fn remember(&self, key: String, entry: Cached) {
        let mut cache = self.cache.lock().unwrap();
        cache.insert(key, entry);
        if let Some(file) = &self.cache_file
            && let Ok(json) = serde_json::to_vec(&*cache)
        {
            let tmp = file.with_extension("tmp");
            if fs::write(&tmp, json).is_ok() {
                let _ = fs::rename(tmp, file);
            }
        }
    }
}

fn found(release: Option<Release>) -> Result<Release, SourceError> {
    release.ok_or(SourceError::NotFound { source_name: NAME })
}

/// Limite horaire atteinte : minutes avant qu'elle se lève.
fn rate_limited(response: &reqwest::Response) -> Option<u64> {
    let status = response.status();
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
    };
    let exhausted = header("x-ratelimit-remaining") == Some(0);
    if !(status == StatusCode::TOO_MANY_REQUESTS || (status == StatusCode::FORBIDDEN && exhausted))
    {
        return None;
    }
    let wait = header("retry-after")
        .or_else(|| header("x-ratelimit-reset").map(|reset| reset.saturating_sub(now())))
        .unwrap_or(3600);
    Some(wait.div_ceil(60).max(1))
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

#[derive(Deserialize)]
struct RawRelease {
    tag_name: String,
    published_at: Option<String>,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_names_tolerate_version_separators() {
        let release = |names: &[&str]| Release {
            version: "1.38.0".into(),
            published_at: String::new(),
            assets: names
                .iter()
                .map(|n| Asset {
                    name: n.to_string(),
                    url: format!("https://example.com/{n}"),
                    size: 1,
                })
                .collect(),
        };
        let cet = release(&["cet_1_38_0.zip"]);
        assert_eq!(
            cet.asset("cet_{version}.zip").unwrap().name,
            "cet_1_38_0.zip"
        );
        let red4ext = release(&["red4ext-symbols-1.38.0.zip", "red4ext-1.38.0.zip"]);
        assert_eq!(
            red4ext.asset("red4ext-{version}.zip").unwrap().name,
            "red4ext-1.38.0.zip"
        );
        assert!(release(&["other.zip"]).asset("cet_{version}.zip").is_none());
    }

    #[test]
    fn wildcard_assets_carry_their_version() {
        let release = Release {
            version: "experimental-latest".into(),
            published_at: String::new(),
            assets: [
                "zDEV-UE4SS_v3.0.1-1181-g83330635.zip",
                "UE4SS_v3.0.1-1181-g83330635.zip",
                "zMapGenBP.zip",
            ]
            .iter()
            .map(|n| Asset {
                name: n.to_string(),
                url: format!("https://example.com/{n}"),
                size: 1,
            })
            .collect(),
        };
        let (asset, version) = release.asset_and_version("UE4SS_v*.zip").unwrap();
        assert_eq!(asset.name, "UE4SS_v3.0.1-1181-g83330635.zip");
        assert_eq!(version, "3.0.1-1181-g83330635");
        assert!(release.asset_and_version("Other_*.zip").is_none());
    }

    #[test]
    fn finds_repositories_in_a_description() {
        let text = "Source: [GitHub](https://github.com/jackhumbert/let_there_be_flight). \
            Needs https://github.com/jac3km4/redscript/releases/latest and \
            github.com/jackhumbert/let_there_be_flight.git again.";
        assert_eq!(
            repos_in(text),
            ["jackhumbert/let_there_be_flight", "jac3km4/redscript"]
        );
        assert!(repos_in("github.com/ alone, github.com/x").is_empty());
    }
}

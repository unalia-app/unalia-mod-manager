//! Releases **GitHub** : source officielle des auteurs qui y publient
//! (frameworks de Cyberpunk 2077). Gratuit, sans compte, sans bridage : on
//! la préfère à Nexus quand elle existe (échelle de sources, SPEC §2.4).

use reqwest::{Client, StatusCode};
use serde::Deserialize;

use super::SourceError;

pub const API_URL: &str = "https://api.github.com";
const NAME: &str = "GitHub";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// Version sans le `v` du tag, ex. `1.30.0`.
    pub version: String,
    pub published_at: String,
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Asset {
    pub name: String,
    #[serde(rename = "browser_download_url")]
    pub url: String,
    pub size: u64,
}

impl Release {
    /// Fichier de la release d'après son modèle, `{version}` remplacé.
    pub fn asset(&self, pattern: &str) -> Option<&Asset> {
        let name = pattern.replace("{version}", &self.version);
        self.assets.iter().find(|a| a.name == name)
    }
}

#[derive(Debug, Clone)]
pub struct GitHub {
    client: Client,
    api_url: String,
    /// Fichiers en HTTPS seulement, hors des tests.
    require_https: bool,
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
        }
    }

    /// Dernière release publiée (hors préversions) de `owner/repo`.
    pub async fn latest_release(&self, repo: &str) -> Result<Release, SourceError> {
        let valid = repo.split('/').count() == 2
            && repo
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.'))
            && !repo.contains("..");
        if !valid {
            return Err(SourceError::NotFound { source_name: NAME });
        }
        let network = |error| SourceError::Network {
            source_name: NAME,
            error,
        };
        let response = self
            .client
            .get(format!("{}/repos/{repo}/releases/latest", self.api_url))
            .header("Accept", "application/vnd.github+json")
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
        let raw: RawRelease = serde_json::from_str(&body).map_err(|e| SourceError::Unexpected {
            source_name: NAME,
            detail: e.to_string(),
        })?;
        Ok(Release {
            version: raw.tag_name.trim_start_matches('v').to_owned(),
            published_at: raw.published_at.unwrap_or_default(),
            assets: raw
                .assets
                .into_iter()
                .filter(|a| !self.require_https || a.url.starts_with("https://"))
                .collect(),
        })
    }
}

#[derive(Deserialize)]
struct RawRelease {
    tag_name: String,
    published_at: Option<String>,
    #[serde(default)]
    assets: Vec<Asset>,
}

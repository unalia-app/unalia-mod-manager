//! Releases **GitHub** : source officielle des auteurs qui y publient
//! (frameworks de Cyberpunk 2077). Gratuit, sans compte, sans bridage : on
//! la préfère à Nexus quand elle existe (échelle de sources, SPEC §2.4).

use reqwest::{Client, StatusCode};
use serde::Deserialize;

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
    /// Fichier de la release d'après son modèle, `{version}` remplacé. Les
    /// auteurs changent parfois la façon d'écrire la version dans le nom
    /// (CET 1.38 : `cet_1_38_0.zip` après `cet_1.37.1.zip`) : points, tirets
    /// bas et tirets sont acceptés.
    pub fn asset(&self, pattern: &str) -> Option<&Asset> {
        ['.', '_', '-']
            .iter()
            .map(|sep| pattern.replace("{version}", &self.version.replace('.', &sep.to_string())))
            .find_map(|name| {
                self.assets
                    .iter()
                    .find(|a| a.name.eq_ignore_ascii_case(&name))
            })
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

//! Reads Thunderstore's ecosystem schema. We only keep what Modalia uses;
//! everything else is ignored to stay tolerant of schema changes.

use std::collections::HashMap;

use serde::Deserialize;

use super::{GameProfile, LoaderPackage, SUPPORTED_LOADERS};
use crate::loaders::rules::{InstallRule, InstallRules, Tracking};

/// Image paths in the schema are relative to this CDN.
const ASSET_BASE: &str = "https://gcdn.thunderstore.io/live/community/";

#[derive(Debug, thiserror::Error)]
#[error("invalid Thunderstore schema: {0}")]
pub struct SchemaError(#[from] serde_json::Error);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Schema {
    games: HashMap<String, Game>,
    #[serde(default)]
    communities: HashMap<String, Community>,
    #[serde(default)]
    modloader_packages: Vec<ModloaderPackage>,
}

#[derive(Deserialize)]
struct Game {
    label: String,
    meta: Meta,
    #[serde(default)]
    r2modman: Option<Vec<R2modman>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Meta {
    display_name: String,
    icon_url: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct R2modman {
    #[serde(default)]
    distributions: Vec<Distribution>,
    package_index: Option<String>,
    package_loader: Option<String>,
    #[serde(default)]
    exe_names: Vec<String>,
    data_folder_name: Option<String>,
    game_instance_type: Option<String>,
    #[serde(default)]
    install_rules: Vec<RawRule>,
    #[serde(default)]
    relative_file_exclusions: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawRule {
    route: String,
    #[serde(default)]
    default_file_extensions: Vec<String>,
    tracking_method: Tracking,
    #[serde(default)]
    sub_routes: Vec<RawRule>,
    #[serde(default)]
    is_default_location: bool,
}

/// Flattened rules: a subroute becomes `route/subroute`.
fn flatten(rules: Vec<RawRule>, parent: &str, out: &mut Vec<InstallRule>) {
    for rule in rules {
        let route = if parent.is_empty() {
            rule.route.clone()
        } else {
            format!("{parent}/{}", rule.route)
        };
        out.push(InstallRule {
            route: route.clone(),
            extensions: rule.default_file_extensions,
            tracking: rule.tracking_method,
            default: rule.is_default_location,
        });
        flatten(rule.sub_routes, &route, out);
    }
}

#[derive(Deserialize)]
struct Distribution {
    platform: String,
    identifier: Option<String>,
}

#[derive(Deserialize)]
struct Community {
    #[serde(default)]
    meta: CommunityMeta,
}

#[derive(Deserialize, Default)]
struct CommunityMeta {
    cover: Option<String>,
    hero: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModloaderPackage {
    package_id: String,
    root_folder: String,
    loader: String,
}

pub(super) fn parse(json: &str) -> Result<(Vec<GameProfile>, Vec<LoaderPackage>), SchemaError> {
    let schema: Schema = serde_json::from_str(json)?;

    let mut games: Vec<GameProfile> = schema
        .games
        .into_values()
        .filter_map(|game| profile(game, &schema.communities))
        .collect();
    // Stable order, independent of the hash table's.
    games.sort_by(|a, b| a.id.cmp(&b.id));

    let loaders = schema
        .modloader_packages
        .into_iter()
        .map(|m| LoaderPackage {
            package_id: m.package_id,
            loader: m.loader,
            root_folder: m.root_folder,
        })
        .collect();

    Ok((games, loaders))
}

/// A game without an r2modman entry of type "game" can't be installed: we
/// drop it (dedicated servers, incomplete entries).
fn profile(game: Game, communities: &HashMap<String, Community>) -> Option<GameProfile> {
    let entry = game
        .r2modman?
        .into_iter()
        .find(|r| r.game_instance_type.as_deref().unwrap_or("game") == "game")?;

    let community = entry
        .package_index
        .as_deref()
        .and_then(community_from_index)
        .unwrap_or_else(|| game.label.clone());
    let loader = entry.package_loader.unwrap_or_else(|| "none".into());
    let community_meta = communities.get(&community).map(|c| &c.meta);
    let mut rules = Vec::new();
    flatten(entry.install_rules, "", &mut rules);
    let mut install_rules = InstallRules {
        rules,
        exclusions: entry.relative_file_exclusions.unwrap_or_default(),
    };
    // MelonLoader games without detailed rules: MelonLoader conventions.
    if install_rules.rules.is_empty() && loader == "recursive-melonloader" {
        install_rules.rules = InstallRules::melonloader_defaults().rules;
    }

    Some(GameProfile {
        steam_app_ids: entry
            .distributions
            .iter()
            .filter(|d| d.platform == "steam")
            .filter_map(|d| d.identifier.as_deref()?.parse().ok())
            .collect(),
        supported: SUPPORTED_LOADERS.contains(&loader.as_str()) && install_rules.installable(),
        install_rules,
        loader,
        exe_names: entry.exe_names,
        data_folder_name: entry.data_folder_name.filter(|d| !d.is_empty()),
        cover_url: community_meta
            .and_then(|m| m.cover.clone())
            .or(game.meta.icon_url)
            .map(asset_url),
        hero_url: community_meta.and_then(|m| m.hero.clone()).map(asset_url),
        nexus: None,
        frameworks: Vec::new(),
        mirrors: Vec::new(),
        launchers: Vec::new(),
        versions: Vec::new(),
        plugins: None,
        modded: None,
        name: game.meta.display_name,
        community,
        id: game.label,
    })
}

/// `https://thunderstore.io/c/<community>/api/v1/...` gives `<community>`.
fn community_from_index(url: &str) -> Option<String> {
    let rest = url.split("/c/").nth(1)?;
    let community = rest.split('/').next()?;
    (!community.is_empty()).then(|| community.to_owned())
}

fn asset_url(path: String) -> String {
    if path.starts_with("https://") {
        path
    } else {
        format!("{ASSET_BASE}{}", path.trim_start_matches('/'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn community_comes_from_the_package_index() {
        assert_eq!(
            community_from_index(
                "https://thunderstore.io/c/lethal-company/api/v1/package-listing-index/"
            )
            .as_deref(),
            Some("lethal-company")
        );
        assert_eq!(
            community_from_index("https://thunderstore.io/api/v1/"),
            None
        );
    }

    #[test]
    fn asset_paths_become_cdn_urls() {
        assert_eq!(
            asset_url("valheim/valheim-cover-360x480.webp".into()),
            "https://gcdn.thunderstore.io/live/community/valheim/valheim-cover-360x480.webp"
        );
        assert_eq!(asset_url("https://x.io/a.png".into()), "https://x.io/a.png");
    }

    #[test]
    fn rejects_invalid_json() {
        assert!(parse("{").is_err());
        assert!(parse("{}").is_err());
    }
}

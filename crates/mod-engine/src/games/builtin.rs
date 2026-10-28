//! Hand-written game profiles (`games/*.toml`), for games missing from the
//! Thunderstore schema. Embedded in the binary: pure data, reviewed like
//! code.

use serde::Deserialize;

use super::{GameProfile, GameVersion, Mirror, NexusGame};
use crate::loaders::rules::InstallRules;

const PROFILES: [&str; 4] = [
    include_str!("../../../../games/cyberpunk-2077.toml"),
    include_str!("../../../../games/fallout-4.toml"),
    include_str!("../../../../games/phantom-blade-zero.toml"),
    include_str!("../../../../games/skyrim-special-edition.toml"),
];

/// Loaders of the hand-written profiles that Modalia knows how to install.
pub const BUILTIN_LOADERS: [&str; 3] = ["bethesda", "redengine", "unreal"];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProfile {
    id: String,
    name: String,
    #[serde(default)]
    steam_app_ids: Vec<u32>,
    #[serde(default)]
    exe_names: Vec<String>,
    loader: String,
    nexus: Option<NexusGame>,
    #[serde(default)]
    frameworks: Vec<crate::loaders::redengine::Framework>,
    #[serde(default)]
    mirrors: Vec<Mirror>,
    #[serde(default)]
    launchers: Vec<String>,
    #[serde(default)]
    versions: Vec<GameVersion>,
    plugins: Option<String>,
}

pub fn profiles() -> Vec<GameProfile> {
    PROFILES
        .iter()
        .map(|text| {
            // Embedded and tested: an error here is a Modalia bug.
            let raw: RawProfile = toml::from_str(text).expect("invalid built-in game profile");
            GameProfile {
                community: raw
                    .nexus
                    .as_ref()
                    .map(|n| format!("nexus:{}", n.domain))
                    .unwrap_or_else(|| raw.id.clone()),
                supported: BUILTIN_LOADERS.contains(&raw.loader.as_str()),
                id: raw.id,
                name: raw.name,
                steam_app_ids: raw.steam_app_ids,
                loader: raw.loader,
                exe_names: raw.exe_names,
                data_folder_name: None,
                cover_url: None,
                hero_url: None,
                nexus: raw.nexus,
                install_rules: InstallRules::default(),
                frameworks: raw.frameworks,
                mirrors: raw.mirrors,
                launchers: raw.launchers,
                versions: raw.versions,
                plugins: raw.plugins,
            }
        })
        .collect()
}

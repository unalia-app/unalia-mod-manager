//! Profils de jeux écrits à la main (`games/*.toml`), pour les jeux absents
//! du schéma Thunderstore. Embarqués dans le binaire : de la donnée pure,
//! relue comme du code (SPEC §7).

use serde::Deserialize;

use super::{GameProfile, GameVersion, Mirror, NexusGame};
use crate::loaders::rules::InstallRules;

const PROFILES: [&str; 2] = [
    include_str!("../../../../games/cyberpunk-2077.toml"),
    include_str!("../../../../games/phantom-blade-zero.toml"),
];

/// Loaders des profils écrits à la main, que Modalia sait installer.
pub const BUILTIN_LOADERS: [&str; 2] = ["redengine", "unreal"];

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
}

pub fn profiles() -> Vec<GameProfile> {
    PROFILES
        .iter()
        .map(|text| {
            // Embarqué et testé : une erreur ici est un bug de Modalia.
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
            }
        })
        .collect()
}

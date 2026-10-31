//! **games** block: what Modalia knows about each moddable game.
//!
//! The base comes from Thunderstore's ecosystem schema (r2modman's): for
//! each game, its Steam ids, its Thunderstore community, its loader, its
//! executables. It is pure data, never code.
//!
//! The schema is cached on disk: the app starts offline with the last known
//! version.

pub mod auto;
mod builtin;
mod schema;

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Serialize;

use crate::loaders::redengine::Framework;
use crate::loaders::rules::InstallRules;

pub use schema::SchemaError;

pub const SCHEMA_URL: &str = "https://thunderstore.io/api/experimental/schema/dev/latest/";

/// Loaders that Modalia knows how to install, with the schema's placement
/// rules (`loaders::rules`).
pub const SUPPORTED_LOADERS: [&str; 7] = [
    "bepinex",
    "bepisloader",
    "melonloader",
    "recursive-melonloader",
    "shimloader",
    "umm",
    "northstar",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameProfile {
    /// Game id in the schema, e.g. `hollow-knight-silksong`.
    pub id: String,
    pub name: String,
    /// Thunderstore community to look for mods in.
    pub community: String,
    pub steam_app_ids: Vec<u32>,
    /// Loader expected by this game's mods, e.g. `bepinex`, `melonloader`.
    pub loader: String,
    /// `true` if Modalia knows how to install this loader.
    pub supported: bool,
    pub exe_names: Vec<String>,
    /// Unity data folder, e.g. `Hollow Knight Silksong_Data`.
    pub data_folder_name: Option<String>,
    /// 360×480 cover that Thunderstore provides for mod managers.
    pub cover_url: Option<String>,
    /// 1920×620 banner.
    pub hero_url: Option<String>,
    /// Game whose mods come from Nexus Mods (`games/*.toml` profiles).
    pub nexus: Option<NexusGame>,
    /// Where this game's mod files go.
    #[serde(skip)]
    pub install_rules: InstallRules,
    /// Frameworks that Modalia installs by itself when a mod needs them.
    #[serde(skip)]
    pub frameworks: Vec<Framework>,
    /// Authors' GitHub repos that their mod's description doesn't mention.
    #[serde(skip)]
    pub mirrors: Vec<Mirror>,
    /// Launchers that open before the game (executable names): while they
    /// run, the game is "starting"; they don't count as the game itself.
    #[serde(skip)]
    pub launchers: Vec<String>,
    /// Major versions of the game, oldest to newest: a user who stayed on an
    /// old version gets the mods published before the next one.
    pub versions: Vec<GameVersion>,
    /// Bethesda games: folder in `%LOCALAPPDATA%` where the game reads its
    /// `plugins.txt`, e.g. `Skyrim Special Edition`.
    #[serde(skip)]
    pub plugins: Option<String>,
    /// Mods that only load when the game starts with extra arguments.
    #[serde(skip)]
    pub modded: Option<ModdedLaunch>,
}

/// Mods that only load when the game starts with extra arguments (game
/// profile, `[modded]`): REDmod mods and `-modded` for Cyberpunk 2077.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModdedLaunch {
    /// Added at launch, e.g. `-modded`.
    pub args: Vec<String>,
    /// An enabled mod with a file under one of these folders needs them.
    pub when_prefix: Vec<String>,
    /// What the game needs to load these mods, installed apart (a free
    /// DLC): without it, the player is told where to get it.
    pub tool: Option<ModdedTool>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModdedTool {
    pub name: String,
    /// Its file, relative to the game folder.
    pub path: String,
    pub url: String,
}

impl ModdedLaunch {
    /// Does a file deployed at `target` (relative to the game folder)
    /// need the arguments?
    pub fn needed_by(&self, target: &str) -> bool {
        let target = target.replace('\\', "/").to_ascii_lowercase();
        self.when_prefix
            .iter()
            .any(|p| target.starts_with(&p.to_ascii_lowercase()))
    }
}

/// Game version and its release date (ISO, `2026-10-22`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameVersion {
    pub name: String,
    pub released: String,
}

impl GameProfile {
    /// For a game that stayed on version `name`: date before which its mods
    /// must have been published (release of the next version). `None`:
    /// unknown or latest version, nothing to filter.
    pub fn mods_before(&self, name: &str) -> Option<&str> {
        let at = self.versions.iter().position(|v| v.name == name)?;
        self.versions.get(at + 1).map(|v| v.released.as_str())
    }

    /// `plugins.txt` of a Bethesda game (`loaders::bethesda::sync_plugins`).
    pub fn plugins_file(&self) -> Option<PathBuf> {
        let folder = self.plugins.as_deref()?;
        let local = std::env::var_os("LOCALAPPDATA")?;
        Some(PathBuf::from(local).join(folder).join("plugins.txt"))
    }
}

/// The same mod published by its author on GitHub (game profile). Only a
/// candidate: the file hash is always checked against Nexus before it is
/// used.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mirror {
    pub name: String,
    pub nexus_id: u32,
    pub github: String,
}

/// The game on Nexus Mods.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct NexusGame {
    /// E.g. `cyberpunk2077`, in URLs and `nxm://` links.
    pub domain: String,
    /// Numeric id (GraphQL API), e.g. 3333.
    pub game_id: u32,
}

/// Package that *is* a loader (e.g. `silksong_modding-BepInExPack_Silksong`):
/// it installs at the game root instead of the plugins folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoaderPackage {
    pub package_id: String,
    pub loader: String,
    /// Archive folder whose content goes to the game root.
    pub root_folder: String,
}

#[derive(Debug, Default)]
pub struct GameCatalog {
    games: Vec<GameProfile>,
    by_steam_app_id: HashMap<u32, usize>,
    loader_packages: HashMap<String, LoaderPackage>,
}

impl GameCatalog {
    pub fn from_schema_json(json: &str) -> Result<Self, SchemaError> {
        let (mut games, loaders) = schema::parse(json)?;
        // Hand-written profiles: they win over the schema for the same
        // Steam game.
        let builtin = builtin::profiles();
        games.retain(|g| {
            !builtin.iter().any(|b| {
                b.steam_app_ids
                    .iter()
                    .any(|id| g.steam_app_ids.contains(id))
            })
        });
        games.extend(builtin);
        let mut by_steam_app_id = HashMap::new();
        for (i, game) in games.iter().enumerate() {
            for &app_id in &game.steam_app_ids {
                by_steam_app_id.entry(app_id).or_insert(i);
            }
        }
        Ok(Self {
            games,
            by_steam_app_id,
            loader_packages: loaders
                .into_iter()
                .map(|l| (l.package_id.clone(), l))
                .collect(),
        })
    }

    pub fn games(&self) -> &[GameProfile] {
        &self.games
    }

    pub fn by_steam_app_id(&self, app_id: u32) -> Option<&GameProfile> {
        self.by_steam_app_id.get(&app_id).map(|&i| &self.games[i])
    }

    /// Game of a Thunderstore community.
    pub fn by_community(&self, community: &str) -> Option<&GameProfile> {
        self.games.iter().find(|g| g.community == community)
    }

    pub fn by_id(&self, id: &str) -> Option<&GameProfile> {
        self.games.iter().find(|g| g.id == id)
    }

    /// Is the package `namespace-name` a loader?
    pub fn loader_package(&self, package_id: &str) -> Option<&LoaderPackage> {
        self.loader_packages.get(package_id)
    }

    /// Recognizes a game installed outside Steam from its folder: the Unity
    /// data folder first (nearly unique), then the executables. An
    /// ambiguous match returns nothing rather than guessing.
    pub fn identify_folder(&self, dir: &Path) -> Option<&GameProfile> {
        let names: Vec<String> = fs::read_dir(dir)
            .ok()?
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_lowercase())
            .collect();
        let has = |name: &str| names.iter().any(|n| n == &name.to_lowercase());

        let by_data = self
            .games
            .iter()
            .filter(|g| g.data_folder_name.as_deref().is_some_and(has))
            .collect::<Vec<_>>();
        if !by_data.is_empty() {
            return single(by_data);
        }
        // An executable in a subfolder (`bin/x64/…`) is checked on
        // disk.
        let exe_found = |exe: &String| {
            if exe.contains('/') {
                dir.join(exe).is_file()
            } else {
                has(exe)
            }
        };
        single(
            self.games
                .iter()
                .filter(|g| g.exe_names.iter().any(exe_found))
                .collect(),
        )
    }
}

impl GameCatalog {
    /// Like `identify_folder`, tolerating the parent folder: people often
    /// pick `D:/Games` instead of `D:/Games/Silksong`. Returns the profile
    /// and the real game folder.
    pub fn locate_game(&self, dir: &Path) -> Option<(&GameProfile, PathBuf)> {
        if let Some(profile) = self.identify_folder(dir) {
            return Some((profile, dir.to_path_buf()));
        }
        let found: Vec<_> = fs::read_dir(dir)
            .ok()?
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter_map(|e| Some((self.identify_folder(&e.path())?, e.path())))
            .collect();
        single(found)
    }
}

/// Id of a manually added game: its Steam id if it has one (images and
/// state shared with the Steam version), else a stable id derived from the
/// profile, with the high bit set so it never collides with a real Steam
/// id.
pub fn manual_app_id(profile: &GameProfile) -> u32 {
    if let Some(&id) = profile.steam_app_ids.first() {
        return id;
    }
    // FNV-1a: stable from one Rust version to the next, unlike the
    // standard library's hasher.
    let hash = profile.id.bytes().fold(0x811c_9dc5u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    });
    hash | 0x8000_0000
}

fn single<T>(items: Vec<T>) -> Option<T> {
    let mut items = items.into_iter();
    let first = items.next()?;
    items.next().is_none().then_some(first)
}

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("couldn't download the Thunderstore game list")]
    Download(#[source] reqwest::Error),
    #[error("the Thunderstore game list is unreadable")]
    Schema(#[from] SchemaError),
    #[error("couldn't write the game list cache")]
    Cache(#[source] io::Error),
}

/// Loads the catalog: the cache if it is younger than `max_age`, else a
/// download (and a cache update). Offline, a stale cache beats nothing.
pub async fn load_catalog(
    client: &reqwest::Client,
    url: &str,
    cache_file: &Path,
    max_age: Duration,
) -> Result<GameCatalog, CatalogError> {
    let cached = fs::read_to_string(cache_file).ok();
    let fresh = fs::metadata(cache_file)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|age| age < max_age);

    if fresh && let Some(Ok(catalog)) = cached.as_deref().map(GameCatalog::from_schema_json) {
        return Ok(catalog);
    }

    match download_schema(client, url).await {
        Ok(json) => {
            // Validate before overwriting a cache that works.
            let catalog = GameCatalog::from_schema_json(&json)?;
            write_atomically(cache_file, &json).map_err(CatalogError::Cache)?;
            Ok(catalog)
        }
        Err(error) => cached
            .as_deref()
            .and_then(|json| GameCatalog::from_schema_json(json).ok())
            .ok_or(CatalogError::Download(error)),
    }
}

async fn download_schema(client: &reqwest::Client, url: &str) -> Result<String, reqwest::Error> {
    client
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await
}

fn write_atomically(path: &Path, contents: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, contents)?;
    fs::rename(tmp, path)
}

//! Catalog of moddable games (Thunderstore schema, hand-written profiles)
//! and mod commands. Each command goes to the game's source: Thunderstore,
//! or Nexus Mods for the `nexus:<domain>` communities.

use std::path::Path;
use std::sync::{Arc, Mutex as SyncMutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mod_engine::HttpClient;
use mod_engine::games::auto::{self, NexusGames};
use mod_engine::games::{GameCatalog, GameProfile, SCHEMA_URL, load_catalog};
use mod_engine::sources::github::GitHub;
use mod_engine::sources::nexus::{self, FoundGame, Nexus, mod_key};
use mod_engine::sources::thunderstore::{MOD_MANAGERS, Thunderstore};
use mod_engine::sources::{Mod, ModSource, ModSummary, Page, SearchQuery, Sort};
use serde::Serialize;
use tauri::async_runtime::Mutex;
use tauri::{AppHandle, Manager, State};

/// The schema rarely moves: one update a day is enough.
const SCHEMA_MAX_AGE: Duration = Duration::from_secs(24 * 3600);

pub struct Catalog {
    pub(crate) client: HttpClient,
    pub(crate) thunderstore: Thunderstore,
    pub(crate) nexus: Nexus,
    pub(crate) github: GitHub,
    games: Mutex<Option<Arc<GameCatalog>>>,
    /// Automatically recognized games: Nexus answers, kept on disk.
    nexus_games: SyncMutex<NexusGames>,
}

impl Catalog {
    /// `cache_dir`: where to keep GitHub answers (hourly limit).
    pub fn new(cache_dir: &std::path::Path) -> Self {
        let client = mod_engine::http_client();
        Self {
            thunderstore: Thunderstore::new(client.clone()),
            nexus: Nexus::new(client.clone()),
            github: GitHub::new(client.clone())
                .with_cache_file(cache_dir.join("github-releases.json"))
                .with_token(crate::mods::dev_env("GITHUB_TOKEN")),
            client,
            games: Mutex::new(None),
            nexus_games: SyncMutex::new(NexusGames::load(&cache_dir.join("nexus-games.json"))),
        }
    }

    /// Nexus games with these names (`games::auto`): the kept answer if it
    /// is still valid, else one request for all the others. Nexus
    /// unreachable: the last known answer.
    pub async fn find_nexus_games(&self, names: &[String]) -> Vec<Option<FoundGame>> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or_default();
        let ask: Vec<String> = {
            let known = self.nexus_games.lock().unwrap();
            names
                .iter()
                .filter(|n| known.fresh(n, now).is_none())
                .cloned()
                .collect()
        };
        if !ask.is_empty() {
            match self.nexus.find_games(&ask).await {
                Ok(found) => {
                    let mut known = self.nexus_games.lock().unwrap();
                    for (name, game) in ask.iter().zip(found) {
                        known.record(name, game, now);
                    }
                    if let Err(error) = known.save() {
                        log::warn!("couldn't save the Nexus game list: {error}");
                    }
                }
                Err(error) => log::warn!("couldn't look up games on Nexus: {error}"),
            }
        }
        let known = self.nexus_games.lock().unwrap();
        names
            .iter()
            .map(|n| match known.fresh(n, now) {
                Some(found) => found.cloned(),
                None => known.last_known(n).cloned(),
            })
            .collect()
    }

    /// Automatic profile of a manually added game (`auto-<domain>`), from
    /// the Nexus game already found.
    pub fn auto_profile(&self, profile_id: &str, game_dir: &Path) -> Option<GameProfile> {
        let domain = auto::domain_of(profile_id)?;
        let found = self.nexus_games.lock().unwrap().by_domain(domain).cloned()?;
        auto::unreal_profile(&found, game_dir)
    }

    /// Catalog loaded once per session (disk cache, else network). `None`
    /// if neither is available: the app goes on with engine detection.
    pub async fn games(&self, app: &AppHandle) -> Option<Arc<GameCatalog>> {
        let mut games = self.games.lock().await;
        if games.is_none() {
            let cache = app
                .path()
                .app_cache_dir()
                .ok()?
                .join("thunderstore-schema.json");
            match load_catalog(&self.client, SCHEMA_URL, &cache, SCHEMA_MAX_AGE).await {
                Ok(catalog) => *games = Some(Arc::new(catalog)),
                Err(error) => log::warn!("game catalog unavailable: {error}"),
            }
        }
        games.clone()
    }
}

/// A game's mods for the Discover grid. Loaders and mod managers are
/// removed: Modalia installs the former by itself and replaces the
/// latter.
#[tauri::command]
pub async fn search_mods(
    app: AppHandle,
    catalog: State<'_, Catalog>,
    community: String,
    text: String,
    sort: Sort,
    page: u32,
) -> Result<Page<ModSummary>, String> {
    let query = SearchQuery {
        text,
        sort,
        page,
        ..Default::default()
    };
    if nexus::domain_of(&community).is_some() {
        return catalog
            .nexus
            .search(&community, &query)
            .await
            .map_err(crate::support::logged);
    }
    let (games, page) = tokio::join!(
        catalog.games(&app),
        catalog.thunderstore.search(&community, &query),
    );
    let mut page = page.map_err(crate::support::logged)?;
    page.items.retain(|m| {
        !MOD_MANAGERS.contains(&m.id.as_str())
            && games
                .as_ref()
                .is_none_or(|g| g.loader_package(&m.id).is_none())
    });
    Ok(page)
}

/// A mod's page for the detail view.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModDetails {
    #[serde(flatten)]
    r#mod: Mod,
    /// Dependencies of the latest version, ready to display.
    dependencies: Vec<DependencyInfo>,
    /// Installs without going through the source's site (all of
    /// Thunderstore; on Nexus, the frameworks, taken from GitHub).
    direct: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyInfo {
    id: String,
    name: String,
    author: String,
    min_version: String,
    /// The game's loader (e.g. BepInEx): installed apart, at the game root.
    loader: bool,
    /// Installed automatically with the mod. Otherwise (Nexus requirement
    /// for a free account), the user has to install it themselves.
    automatic: bool,
}

#[tauri::command]
pub async fn get_mod(
    app: AppHandle,
    catalog: State<'_, Catalog>,
    community: String,
    id: String,
) -> Result<ModDetails, String> {
    if let Some(domain) = nexus::domain_of(&community) {
        return nexus_mod(&app, &catalog, &community, domain, &id).await;
    }
    let (games, found) = tokio::join!(
        catalog.games(&app),
        catalog.thunderstore.get(&community, &id),
    );
    let found = found.map_err(crate::support::logged)?;
    let dependencies = found
        .latest()
        .map(|v| v.dependencies.as_slice())
        .unwrap_or_default()
        .iter()
        .map(|d| {
            let (author, name) = d.id.split_once('-').unwrap_or(("", &d.id));
            DependencyInfo {
                name: name.replace('_', " "),
                author: author.to_owned(),
                min_version: d.min_version.clone(),
                loader: games
                    .as_ref()
                    .is_some_and(|g| g.loader_package(&d.id).is_some()),
                automatic: true,
                id: d.id.clone(),
            }
        })
        .collect();
    Ok(ModDetails {
        r#mod: found,
        dependencies,
        direct: true,
    })
}

/// A Nexus mod's page: its requirements come with their names. The game's
/// frameworks install by themselves (from GitHub), the other requirements
/// don't (free account).
async fn nexus_mod(
    app: &AppHandle,
    catalog: &Catalog,
    community: &str,
    domain: &str,
    id: &str,
) -> Result<ModDetails, String> {
    let mod_id = nexus::parse_mod_key(id).ok_or("This mod doesn't exist on Nexus Mods.")?;
    let (games, found, info) = tokio::join!(
        catalog.games(app),
        catalog.nexus.get(community, id),
        catalog.nexus.mod_info(domain, mod_id),
    );
    let found = found.map_err(crate::support::logged)?;
    let frameworks = games
        .as_ref()
        .and_then(|g| g.games().iter().find(|p| p.community == community))
        .map(|p| p.frameworks.clone())
        .unwrap_or_default();
    let dependencies = info
        .map(|i| i.requirements)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|r| {
            let required = r.mod_id?;
            let framework = frameworks.iter().any(|f| f.nexus_id == Some(required));
            Some(DependencyInfo {
                id: mod_key(required),
                name: r.name,
                author: String::new(),
                min_version: String::new(),
                loader: framework,
                automatic: framework,
            })
        })
        .collect();
    Ok(ModDetails {
        r#mod: found,
        dependencies,
        direct: frameworks.iter().any(|f| f.nexus_id == Some(mod_id)),
    })
}

/// Description in raw Markdown: the UI sanitizes it before display.
#[tauri::command]
pub async fn mod_description(
    catalog: State<'_, Catalog>,
    community: String,
    id: String,
    version: String,
) -> Result<String, String> {
    let result = if nexus::domain_of(&community).is_some() {
        catalog.nexus.description(&community, &id, &version).await
    } else {
        catalog
            .thunderstore
            .description(&community, &id, &version)
            .await
    };
    result.map_err(crate::support::logged)
}

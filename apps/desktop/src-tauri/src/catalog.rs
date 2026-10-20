//! Catalogue des jeux moddables (schéma Thunderstore) et commandes de mods.

use std::sync::Arc;
use std::time::Duration;

use mod_engine::HttpClient;
use mod_engine::games::{GameCatalog, SCHEMA_URL, load_catalog};
use mod_engine::sources::thunderstore::{MOD_MANAGERS, Thunderstore};
use mod_engine::sources::{Mod, ModSource, ModSummary, Page, SearchQuery, Sort};
use serde::Serialize;
use tauri::async_runtime::Mutex;
use tauri::{AppHandle, Manager, State};

/// Le schéma bouge peu : une mise à jour par jour suffit.
const SCHEMA_MAX_AGE: Duration = Duration::from_secs(24 * 3600);

pub struct Catalog {
    client: HttpClient,
    thunderstore: Thunderstore,
    games: Mutex<Option<Arc<GameCatalog>>>,
}

impl Catalog {
    pub fn new() -> Self {
        let client = mod_engine::http_client();
        Self {
            thunderstore: Thunderstore::new(client.clone()),
            client,
            games: Mutex::new(None),
        }
    }

    /// Catalogue chargé une fois par session (cache disque, sinon réseau).
    /// `None` si ni l'un ni l'autre n'est disponible : l'appli continue avec
    /// la détection par moteur.
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
                Err(error) => eprintln!("game catalog unavailable: {error}"),
            }
        }
        games.clone()
    }
}

/// Mods d'un jeu pour la grille Découvrir. Les loaders et les gestionnaires
/// de mods sont retirés : Modalia installe les premiers tout seul et
/// remplace les seconds.
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
    let (games, page) = tokio::join!(
        catalog.games(&app),
        catalog.thunderstore.search(&community, &query),
    );
    let mut page = page.map_err(|e| e.to_string())?;
    page.items.retain(|m| {
        !MOD_MANAGERS.contains(&m.id.as_str())
            && games
                .as_ref()
                .is_none_or(|g| g.loader_package(&m.id).is_none())
    });
    Ok(page)
}

/// Fiche d'un mod pour la page de détail.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModDetails {
    #[serde(flatten)]
    r#mod: Mod,
    /// Dépendances de la dernière version, prêtes à afficher.
    dependencies: Vec<DependencyInfo>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyInfo {
    id: String,
    name: String,
    author: String,
    min_version: String,
    /// Loader du jeu (ex. BepInEx) : installé à part, à la racine du jeu.
    loader: bool,
}

#[tauri::command]
pub async fn get_mod(
    app: AppHandle,
    catalog: State<'_, Catalog>,
    community: String,
    id: String,
) -> Result<ModDetails, String> {
    let (games, found) = tokio::join!(
        catalog.games(&app),
        catalog.thunderstore.get(&community, &id),
    );
    let found = found.map_err(|e| e.to_string())?;
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
                id: d.id.clone(),
            }
        })
        .collect();
    Ok(ModDetails {
        r#mod: found,
        dependencies,
    })
}

/// Description en Markdown brut : l'interface la nettoie avant affichage.
#[tauri::command]
pub async fn mod_description(
    catalog: State<'_, Catalog>,
    id: String,
    version: String,
) -> Result<String, String> {
    catalog
        .thunderstore
        .description(&id, &version)
        .await
        .map_err(|e| e.to_string())
}

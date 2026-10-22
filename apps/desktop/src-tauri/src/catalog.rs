//! Catalogue des jeux moddables (schéma Thunderstore, profils écrits à la
//! main) et commandes de mods. Chaque commande s'adresse à la source du jeu :
//! Thunderstore, ou Nexus Mods pour les communautés `nexus:<domaine>`.

use std::sync::Arc;
use std::time::Duration;

use mod_engine::HttpClient;
use mod_engine::games::{GameCatalog, SCHEMA_URL, load_catalog};
use mod_engine::sources::github::GitHub;
use mod_engine::sources::nexus::{self, Nexus, mod_key};
use mod_engine::sources::thunderstore::{MOD_MANAGERS, Thunderstore};
use mod_engine::sources::{Mod, ModSource, ModSummary, Page, SearchQuery, Sort};
use serde::Serialize;
use tauri::async_runtime::Mutex;
use tauri::{AppHandle, Manager, State};

/// Le schéma bouge peu : une mise à jour par jour suffit.
const SCHEMA_MAX_AGE: Duration = Duration::from_secs(24 * 3600);

pub struct Catalog {
    pub(crate) client: HttpClient,
    pub(crate) thunderstore: Thunderstore,
    pub(crate) nexus: Nexus,
    pub(crate) github: GitHub,
    games: Mutex<Option<Arc<GameCatalog>>>,
}

impl Catalog {
    /// `cache_dir` : où garder les réponses de GitHub (limite horaire).
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
    if nexus::domain_of(&community).is_some() {
        return catalog
            .nexus
            .search(&community, &query)
            .await
            .map_err(|e| e.to_string());
    }
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
    /// S'installe sans passer par le site de la source (tout Thunderstore ;
    /// sur Nexus, les frameworks, pris sur GitHub).
    direct: bool,
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
    /// Installée automatiquement avec le mod. Sinon (prérequis Nexus pour
    /// un compte gratuit), l'utilisateur doit l'installer lui-même.
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

/// Fiche d'un mod Nexus : ses prérequis viennent avec leur nom. Les
/// frameworks du jeu s'installent tout seuls (depuis GitHub), les autres
/// prérequis non (compte gratuit).
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
    let found = found.map_err(|e| e.to_string())?;
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
            let framework = frameworks.iter().any(|f| f.nexus_id == required);
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
        direct: frameworks.iter().any(|f| f.nexus_id == mod_id),
    })
}

/// Description en Markdown brut : l'interface la nettoie avant affichage.
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
    result.map_err(|e| e.to_string())
}

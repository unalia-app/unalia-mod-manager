mod catalog;

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use mod_engine::artwork::{self, ArtworkKind};
use mod_engine::detect::{self, Engine, SteamGame};
use mod_engine::games::GameProfile;
use serde::Serialize;
use tauri::http::{Request, Response, StatusCode, header};
use tauri::{AppHandle, Manager, State, UriSchemeContext, UriSchemeResponder, Wry};

use crate::catalog::Catalog;

/// Ce que l'appli sait de la bibliothèque Steam pendant la session.
#[derive(Default)]
struct Library {
    steam_root: Mutex<Option<PathBuf>>,
    /// Accents déjà calculés (`None` : jeu sans accent, accent neutre).
    accents: Mutex<HashMap<u32, Option<String>>>,
}

impl Library {
    fn steam_root(&self) -> Option<PathBuf> {
        self.steam_root.lock().unwrap().clone()
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GameScan {
    /// `false` si aucune installation de Steam n'a été trouvée : l'accueil
    /// propose alors l'ajout manuel d'un dossier.
    steam_found: bool,
    games: Vec<GameEntry>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GameEntry {
    #[serde(flatten)]
    game: SteamGame,
    /// Profil Thunderstore du jeu, s'il est connu.
    profile: Option<GameProfile>,
    /// Modalia sait-il modder ce jeu ?
    moddable: bool,
}

#[tauri::command]
async fn list_games(
    app: AppHandle,
    library: State<'_, Library>,
    catalog: State<'_, Catalog>,
) -> Result<GameScan, String> {
    let root = steam_root();
    *library.steam_root.lock().unwrap() = root.clone();

    // Scan local et catalogue (cache ou réseau) en parallèle.
    let scan = tauri::async_runtime::spawn_blocking(move || {
        root.map(|r| detect::scan_steam(&r)).transpose()
    });
    let (scan, games_catalog) = tokio::join!(scan, catalog.games(&app));
    let Some(games) = scan
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?
    else {
        return Ok(GameScan {
            steam_found: false,
            games: Vec::new(),
        });
    };

    Ok(GameScan {
        steam_found: true,
        games: games
            .into_iter()
            .map(|game| {
                let profile = games_catalog
                    .as_ref()
                    .and_then(|c| c.by_steam_app_id(game.app_id))
                    .cloned();
                // Sans catalogue (premier lancement hors ligne), on se rabat
                // sur la détection du moteur.
                let moddable = match (&games_catalog, &profile) {
                    (Some(_), Some(p)) => p.supported,
                    (Some(_), None) => false,
                    (None, _) => game.engine == Engine::Unity,
                };
                GameEntry {
                    game,
                    profile,
                    moddable,
                }
            })
            .collect(),
    })
}

/// Couleur d'accent CSS du jeu, extraite de sa jaquette.
#[tauri::command]
async fn game_accent(app_id: u32, library: State<'_, Library>) -> Result<Option<String>, String> {
    if let Some(cached) = library.accents.lock().unwrap().get(&app_id) {
        return Ok(cached.clone());
    }
    let Some(root) = library.steam_root() else {
        return Ok(None);
    };

    let accent = tauri::async_runtime::spawn_blocking(move || {
        artwork::game_accent(&root, app_id).map(|a| a.css())
    })
    .await
    .map_err(|e| e.to_string())?;

    library
        .accents
        .lock()
        .unwrap()
        .insert(app_id, accent.clone());
    Ok(accent)
}

/// En dev, `MODALIA_STEAM_ROOT` remplace la recherche automatique (ex. pour
/// pointer sur `fixtures/steam/root`). Ignorée dans les builds de release.
fn steam_root() -> Option<PathBuf> {
    if cfg!(debug_assertions)
        && let Some(root) = std::env::var_os("MODALIA_STEAM_ROOT")
    {
        return Some(PathBuf::from(root));
    }
    detect::find_steam_root()
}

/// Protocole `artwork://localhost/<appid>/<type>` (Windows :
/// `http://artwork.localhost/...`). Ne sert que les images du cache de Steam :
/// l'interface ne manipule jamais de chemin de fichier.
fn artwork_protocol(
    ctx: UriSchemeContext<'_, Wry>,
    request: Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    let root = ctx.app_handle().state::<Library>().steam_root();
    let path = request.uri().path().to_owned();

    tauri::async_runtime::spawn_blocking(move || {
        let file = root
            .zip(parse_artwork_path(&path))
            .and_then(|(root, (app_id, kind))| artwork::find_artwork(&root, app_id, kind));
        let response = match file.and_then(|f| Some((fs::read(&f).ok()?, f))) {
            Some((bytes, file)) => Response::builder()
                .header(header::CONTENT_TYPE, mime(&file))
                .header(header::CACHE_CONTROL, "max-age=3600")
                .body(bytes),
            None => Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Vec::new()),
        };
        responder.respond(response.expect("valid HTTP response"));
    });
}

fn parse_artwork_path(path: &str) -> Option<(u32, ArtworkKind)> {
    let mut segments = path.trim_matches('/').split('/');
    let app_id = segments.next()?.parse().ok()?;
    let kind = segments.next()?.parse().ok()?;
    segments.next().is_none().then_some((app_id, kind))
}

fn mime(file: &std::path::Path) -> &'static str {
    match file.extension().and_then(|e| e.to_str()) {
        Some("png") => "image/png",
        _ => "image/jpeg",
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(Library::default())
        .manage(Catalog::new())
        .register_asynchronous_uri_scheme_protocol("artwork", artwork_protocol)
        .invoke_handler(tauri::generate_handler![
            list_games,
            game_accent,
            catalog::search_mods,
            catalog::get_mod,
            catalog::mod_description,
        ])
        .run(tauri::generate_context!())
        .expect("Modalia failed to start");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_artwork_urls() {
        assert_eq!(
            parse_artwork_path("/1030300/cover"),
            Some((1030300, ArtworkKind::Cover))
        );
        assert_eq!(parse_artwork_path("/1030300/cover/extra"), None);
        assert_eq!(parse_artwork_path("/../cover"), None);
        assert_eq!(parse_artwork_path("/1030300/../../etc"), None);
        assert_eq!(parse_artwork_path("/"), None);
    }
}

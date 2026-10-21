mod catalog;
mod mods;

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use mod_engine::artwork::{self, ArtworkKind};
use mod_engine::detect::{self, Engine, SteamGame, detect_engine};
use mod_engine::download;
use mod_engine::games::{GameProfile, manual_app_id};
use mod_engine::state::ManualGame;
use serde::Serialize;
use tauri::http::{Request, Response, StatusCode, header};
use tauri::{AppHandle, Manager, State, UriSchemeContext, UriSchemeResponder, Wry};

use crate::catalog::Catalog;
use crate::mods::Mods;

/// Ce que l'appli sait de la bibliothèque de jeux pendant la session.
#[derive(Default)]
struct Library {
    steam_root: Mutex<Option<PathBuf>>,
    /// Accents déjà calculés (`None` : jeu sans accent, accent neutre).
    accents: Mutex<HashMap<u32, Option<String>>>,
    /// Jeux moddables du dernier scan. Les commandes d'installation ne
    /// reçoivent qu'un identifiant de l'interface : le dossier du jeu vient
    /// toujours d'ici, jamais de la webview.
    games: Mutex<HashMap<u32, KnownGame>>,
    /// Images publiées par Thunderstore pour chaque jeu connu (jaquette,
    /// bannière) : repli quand le cache de Steam n'a rien, notamment pour
    /// les jeux ajoutés à la main.
    remote_art: Mutex<HashMap<u32, RemoteArt>>,
    /// Une image téléchargée à la fois : deux demandes simultanées de la
    /// même image n'écrivent pas le même fichier en parallèle.
    art_downloads: tauri::async_runtime::Mutex<()>,
}

#[derive(Clone)]
struct KnownGame {
    dir: PathBuf,
    community: String,
    /// Ajouté à la main : se lance par son exécutable, pas par Steam.
    manual: bool,
    /// Exécutables connus du catalogue pour ce jeu.
    exe_names: Vec<String>,
    /// Profil du jeu : source des mods (Nexus), frameworks.
    profile: GameProfile,
}

#[derive(Clone, Default)]
struct RemoteArt {
    cover: Option<String>,
    hero: Option<String>,
}

impl Library {
    fn steam_root(&self) -> Option<PathBuf> {
        self.steam_root.lock().unwrap().clone()
    }

    fn game(&self, app_id: u32) -> Option<KnownGame> {
        self.games.lock().unwrap().get(&app_id).cloned()
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
    /// `steam`, ou `manual` pour un jeu ajouté à la main.
    source: &'static str,
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
    mods: State<'_, Mods>,
) -> Result<GameScan, String> {
    let root = steam_root();
    *library.steam_root.lock().unwrap() = root.clone();

    // Scan local et catalogue (cache ou réseau) en parallèle.
    let scan = tauri::async_runtime::spawn_blocking(move || {
        root.map(|r| detect::scan_steam(&r)).transpose()
    });
    let (scan, games_catalog) = tokio::join!(scan, catalog.games(&app));
    let steam = scan
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    let steam_found = steam.is_some();

    let mut entries: Vec<GameEntry> = steam
        .unwrap_or_default()
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
                source: "steam",
                profile,
                moddable,
            }
        })
        .collect();

    // Jeux ajoutés à la main, sauf ceux que Steam connaît déjà.
    let manual = mods.state().lock().await.manual_games().unwrap_or_default();
    for ManualGame {
        app_id,
        profile_id,
        path,
        ..
    } in manual
    {
        let Some(profile) = games_catalog
            .as_ref()
            .and_then(|c| c.by_id(&profile_id))
            .cloned()
        else {
            continue;
        };
        let dir = PathBuf::from(&path);
        if entries.iter().any(|e| e.game.app_id == app_id) || !dir.is_dir() {
            continue;
        }
        entries.push(GameEntry {
            game: SteamGame {
                app_id,
                name: profile.name.clone(),
                last_updated: exe_modified(&dir, &profile.exe_names),
                engine: detect_engine(&dir),
                library: dir.clone(),
                install_dir: dir,
            },
            source: "manual",
            moddable: profile.supported,
            profile: Some(profile),
        });
    }
    entries.sort_by_cached_key(|e| (e.game.name.to_lowercase(), e.game.app_id));

    *library.games.lock().unwrap() = entries
        .iter()
        .filter(|e| e.moddable)
        .filter_map(|e| {
            let profile = e.profile.as_ref()?;
            Some((
                e.game.app_id,
                KnownGame {
                    dir: e.game.install_dir.clone(),
                    community: profile.community.clone(),
                    manual: e.source == "manual",
                    exe_names: profile.exe_names.clone(),
                    profile: profile.clone(),
                },
            ))
        })
        .collect();
    *library.remote_art.lock().unwrap() = entries
        .iter()
        .filter_map(|e| {
            let p = e.profile.as_ref()?;
            Some((
                e.game.app_id,
                RemoteArt {
                    cover: p.cover_url.clone(),
                    hero: p.hero_url.clone(),
                },
            ))
        })
        .collect();

    Ok(GameScan {
        steam_found,
        games: entries,
    })
}

/// Ajoute un jeu installé hors de Steam, à partir du dossier choisi par
/// l'utilisateur. Le jeu est reconnu par ses fichiers grâce au catalogue de
/// Thunderstore. Renvoie l'identifiant du jeu.
#[tauri::command]
async fn add_manual_game(
    app: AppHandle,
    catalog: State<'_, Catalog>,
    mods: State<'_, Mods>,
    path: String,
) -> Result<u32, String> {
    let dir = PathBuf::from(&path);
    if !dir.is_dir() {
        return Err("This folder doesn't exist anymore.".into());
    }
    let games = catalog.games(&app).await.ok_or(
        "Modalia needs to be online once to recognize games.\nThe Thunderstore game list couldn't be downloaded.",
    )?;
    let (profile, game_dir) = games.locate_game(&dir).ok_or(
        "Modalia doesn't recognize this game.\nPick the folder that contains the game's .exe. Modalia knows the games listed on Thunderstore, and Cyberpunk 2077.",
    )?;
    if !profile.supported {
        return Err(format!(
            "{} uses {}, which Modalia doesn't support yet.",
            profile.name, profile.loader
        ));
    }

    let app_id = manual_app_id(profile);
    mods.state()
        .lock()
        .await
        .save_manual_game(&ManualGame {
            app_id,
            profile_id: profile.id.clone(),
            path: game_dir.to_string_lossy().into_owned(),
            added_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or_default(),
        })
        .map_err(|e| e.to_string())?;
    Ok(app_id)
}

/// Hors de Steam, la date de la dernière mise à jour est celle de
/// l'exécutable du jeu.
fn exe_modified(dir: &std::path::Path, exe_names: &[String]) -> Option<i64> {
    exe_names
        .iter()
        .filter_map(|name| fs::metadata(dir.join(name)).ok()?.modified().ok())
        .max()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
}

/// Couleur d'accent CSS du jeu, extraite de sa jaquette.
#[tauri::command]
async fn game_accent(
    app: AppHandle,
    app_id: u32,
    library: State<'_, Library>,
) -> Result<Option<String>, String> {
    if let Some(cached) = library.accents.lock().unwrap().get(&app_id) {
        return Ok(cached.clone());
    }
    let root = library.steam_root();
    let mut accent = tauri::async_runtime::spawn_blocking(move || {
        root.and_then(|root| artwork::game_accent(&root, app_id).map(|a| a.css()))
    })
    .await
    .map_err(|e| e.to_string())?;
    // Rien dans le cache de Steam : jaquette publiée par Thunderstore.
    if accent.is_none()
        && let Some(cover) = artwork_file(&app, app_id, ArtworkKind::Cover).await
    {
        accent = tauri::async_runtime::spawn_blocking(move || {
            artwork::accent_from_path(&cover).map(|a| a.css())
        })
        .await
        .map_err(|e| e.to_string())?;
    }

    library
        .accents
        .lock()
        .unwrap()
        .insert(app_id, accent.clone());
    Ok(accent)
}

/// Image d'un jeu : cache de Steam d'abord, sinon l'image publiée par
/// Thunderstore pour les gestionnaires de mods, téléchargée une fois dans
/// le cache de l'appli.
async fn artwork_file(app: &AppHandle, app_id: u32, kind: ArtworkKind) -> Option<PathBuf> {
    let library = app.state::<Library>();
    if let Some(root) = library.steam_root()
        && let Some(file) = artwork::find_artwork(&root, app_id, kind)
    {
        return Some(file);
    }
    let remote = library.remote_art.lock().unwrap().get(&app_id).cloned()?;
    let (url, name) = match kind {
        ArtworkKind::Cover => (remote.cover?, "cover"),
        ArtworkKind::Hero | ArtworkKind::HeroBlur => (remote.hero?, "hero"),
        ArtworkKind::Header | ArtworkKind::Logo => return None,
    };
    let extension = url
        .rsplit('.')
        .next()
        .filter(|e| e.len() <= 4 && e.chars().all(|c| c.is_ascii_alphanumeric()))
        .unwrap_or("img");
    let file = app
        .path()
        .app_cache_dir()
        .ok()?
        .join("artwork")
        .join(format!("{app_id}-{name}.{extension}"));
    let _guard = library.art_downloads.lock().await;
    if !file.is_file() {
        let client = app.state::<Catalog>().client.clone();
        // Une image : une seule connexion, 20 Mo au plus.
        let options = download::Options {
            max_bytes: 20 * 1024 * 1024,
            connections: 1,
            space_factor: 1.0,
            ..download::Options::default()
        };
        download::download(&client, &url, &file, &options, |_, _| {})
            .await
            .ok()?;
    }
    Some(file)
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
/// `http://artwork.localhost/...`). Ne sert que les images des jeux (cache de
/// Steam ou repli Thunderstore) : l'interface ne manipule jamais de chemin.
fn artwork_protocol(
    ctx: UriSchemeContext<'_, Wry>,
    request: Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    let app = ctx.app_handle().clone();
    let path = request.uri().path().to_owned();

    tauri::async_runtime::spawn(async move {
        let file = match parse_artwork_path(&path) {
            Some((app_id, kind)) => artwork_file(&app, app_id, kind).await,
            None => None,
        };
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
        Some("webp") => "image/webp",
        _ => "image/jpeg",
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // En premier : un lien `nxm://` ouvert pendant que Modalia tourne
        // relance l'appli, qui passe le lien à la fenêtre déjà ouverte.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(Library::default())
        .manage(Catalog::new())
        .setup(|app| {
            app.manage(mods::Mods::open(app.handle())?);
            mods::nexus_setup(app.handle());
            tauri::async_runtime::spawn(mods::watch(app.handle().clone()));
            Ok(())
        })
        .register_asynchronous_uri_scheme_protocol("artwork", artwork_protocol)
        .invoke_handler(tauri::generate_handler![
            list_games,
            add_manual_game,
            game_accent,
            catalog::search_mods,
            catalog::get_mod,
            catalog::mod_description,
            mods::installed_mods,
            mods::install_mod,
            mods::set_mod_enabled,
            mods::uninstall_mod,
            mods::restore_mod,
            mods::cancel_pending,
            mods::check_updates,
            mods::game_status,
            mods::play_game,
            mods::nexus_account,
            mods::nexus_connect,
            mods::nexus_disconnect,
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

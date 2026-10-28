mod app_update;
mod catalog;
mod mods;
mod support;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use mod_engine::artwork::{self, ArtworkKind};
use mod_engine::detect::{self, Engine, SteamGame, detect_anti_cheat, detect_engine, shortcuts};
use mod_engine::download;
use mod_engine::games::{GameProfile, auto, manual_app_id};
use mod_engine::launch::{self, LaunchTarget};
use mod_engine::state::ManualGame;
use serde::Serialize;
use tauri::http::{Request, Response, StatusCode, header};
use tauri::{AppHandle, Manager, State, UriSchemeContext, UriSchemeResponder, Wry};

use crate::catalog::Catalog;
use crate::mods::Mods;

/// What the app knows about the game library during the session.
#[derive(Default)]
struct Library {
    steam_root: Mutex<Option<PathBuf>>,
    /// Accents already computed (`None`: game without an accent, neutral accent).
    accents: Mutex<HashMap<u32, Option<String>>>,
    /// Moddable games from the last scan. Install commands only receive an
    /// id from the UI: the game folder always comes from here, never from
    /// the webview.
    games: Mutex<HashMap<u32, KnownGame>>,
    /// Images published by Thunderstore for each known game (cover,
    /// banner): fallback when Steam's cache has nothing, notably for
    /// manually added games.
    remote_art: Mutex<HashMap<u32, RemoteArt>>,
    /// One image downloaded at a time: two simultaneous requests for the
    /// same image don't write the same file in parallel.
    art_downloads: tauri::async_runtime::Mutex<()>,
}

#[derive(Clone)]
struct KnownGame {
    dir: PathBuf,
    community: String,
    /// Added by hand: launches through its executable, not through Steam.
    manual: bool,
    /// Executables known to the catalog for this game.
    exe_names: Vec<String>,
    /// Game profile: mod source (Nexus), frameworks.
    profile: GameProfile,
    /// Non-Steam game added to Steam: URL that launches it through Steam
    /// (and its overlay).
    steam_shortcut: Option<String>,
    /// Game version picked by the user (stayed on an old one).
    game_version: Option<String>,
}

impl KnownGame {
    /// Game that stayed on an old version: its mods must have been
    /// published before this date.
    fn mods_before(&self) -> Option<&str> {
        self.profile.mods_before(self.game_version.as_deref()?)
    }
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
    /// `false` if no Steam install was found: the home screen then offers
    /// to add a folder by hand.
    steam_found: bool,
    games: Vec<GameEntry>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GameEntry {
    #[serde(flatten)]
    game: SteamGame,
    /// `steam`, or `manual` for a manually added game.
    source: &'static str,
    /// The game's Thunderstore profile, if known.
    profile: Option<GameProfile>,
    /// Can Modalia mod this game?
    moddable: bool,
    /// Manually added game that the user also added to Steam.
    steam_shortcut: bool,
    /// Play goes through Steam (overlay, FPS counter): a per-game setting,
    /// on by default when the shortcut exists.
    launch_via_steam: bool,
    /// Old version of the game the user stayed on (one of the profile's
    /// `versions`); `None`: the latest.
    game_version: Option<String>,
    /// Anti-cheat found in the game (e.g. `Easy Anti-Cheat`): mods stay
    /// allowed, the UI warns about the ban risk.
    anti_cheat: Option<&'static str>,
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

    // Local scan and catalog (cache or network) in parallel.
    let scan = tauri::async_runtime::spawn_blocking(move || {
        root.map(|r| detect::scan_steam(&r)).transpose()
    });
    let (scan, games_catalog) = tokio::join!(scan, catalog.games(&app));
    let steam = scan
        .map_err(crate::support::logged)?
        .map_err(crate::support::logged)?;
    let steam_found = steam.is_some();

    let mut entries: Vec<GameEntry> = steam
        .unwrap_or_default()
        .into_iter()
        .map(|game| {
            let profile = games_catalog
                .as_ref()
                .and_then(|c| c.by_steam_app_id(game.app_id))
                .cloned();
            // Without a catalog (first launch offline), fall back on engine
            // detection.
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
                steam_shortcut: false,
                launch_via_steam: false,
                game_version: None,
                anti_cheat: None,
            }
        })
        .collect();

    // Unreal games without a profile: recognized from their name on Nexus.
    let unknown: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter(|(_, e)| e.profile.is_none() && e.game.engine == Engine::Unreal)
        .map(|(i, _)| i)
        .collect();
    let names: Vec<String> = unknown.iter().map(|&i| entries[i].game.name.clone()).collect();
    for (i, found) in unknown.into_iter().zip(catalog.find_nexus_games(&names).await) {
        let entry = &mut entries[i];
        if let Some(profile) = found.and_then(|f| auto::unreal_profile(&f, &entry.game.install_dir)) {
            entry.moddable = true;
            entry.profile = Some(profile);
        }
    }

    // Manually added games, except the ones Steam already knows. If they
    // were added to Steam as non-Steam games, Play can go through Steam.
    let shortcuts = library
        .steam_root()
        .map(|root| shortcuts::steam_shortcuts(&root))
        .unwrap_or_default();
    let (manual, launch_settings) = {
        let state = mods.state().lock().await;
        let manual = state.manual_games().unwrap_or_default();
        let settings: HashMap<u32, bool> = manual
            .iter()
            .map(|g| {
                let off = state
                    .setting(&launch_setting(g.app_id))
                    .ok()
                    .flatten()
                    .is_some_and(|v| v == "0");
                (g.app_id, !off)
            })
            .collect();
        (manual, settings)
    };
    for ManualGame {
        app_id,
        profile_id,
        path,
        ..
    } in manual
    {
        let dir = PathBuf::from(&path);
        let Some(profile) = games_catalog
            .as_ref()
            .and_then(|c| c.by_id(&profile_id))
            .cloned()
            .or_else(|| catalog.auto_profile(&profile_id, &dir))
        else {
            continue;
        };
        if entries.iter().any(|e| e.game.app_id == app_id) || !dir.is_dir() {
            continue;
        }
        let has_shortcut = shortcuts::shortcut_for(&shortcuts, &dir).is_some();
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
            steam_shortcut: has_shortcut,
            launch_via_steam: has_shortcut && launch_settings.get(&app_id).copied().unwrap_or(true),
            game_version: None,
            anti_cheat: None,
            profile: Some(profile),
        });
    }
    for entry in entries.iter_mut().filter(|e| e.moddable) {
        entry.anti_cheat = detect_anti_cheat(&entry.game.install_dir);
    }
    entries.sort_by_cached_key(|e| (e.game.name.to_lowercase(), e.game.app_id));
    {
        let state = mods.state().lock().await;
        for entry in &mut entries {
            let versions = entry.profile.as_ref().map(|p| &p.versions);
            entry.game_version = state
                .setting(&version_setting(entry.game.app_id))
                .ok()
                .flatten()
                .filter(|v| versions.is_some_and(|all| all.iter().any(|known| &known.name == v)));
        }
    }

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
                    steam_shortcut: e
                        .launch_via_steam
                        .then(|| shortcuts::shortcut_for(&shortcuts, &e.game.install_dir))
                        .flatten()
                        .map(|s| s.launch_url()),
                    game_version: e.game_version.clone(),
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

/// Adds a game installed outside Steam, from the folder the user picked.
/// The game is recognized by its files thanks to the Thunderstore catalog
/// and the hand-written profiles, else from its engine and its name on
/// Nexus (`games::auto`). Returns the game id.
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
    let games = catalog.games(&app).await;
    let located = games
        .as_ref()
        .and_then(|g| g.locate_game(&dir))
        .map(|(profile, game_dir)| (profile.clone(), game_dir));
    let (profile, game_dir) = match located {
        Some(found) => found,
        None => find_auto_game(&catalog, &dir).await.map_err(|unreal| match (unreal, &games) {
            (Some(name), _) => format!(
                "Unalia found an Unreal Engine game, but no Nexus Mods page named \"{name}\".\nRename the folder to the game's exact name, then try again."
            ),
            (None, None) => "Unalia needs to be online once to recognize games.\nThe Thunderstore game list couldn't be downloaded.".to_owned(),
            (None, Some(_)) => "Unalia doesn't recognize this game.\nPick the folder that contains the game's .exe. Unalia knows the games listed on Thunderstore, Unreal Engine games on Nexus Mods, Cyberpunk 2077, Skyrim Special Edition and Fallout 4.".to_owned(),
        })?,
    };
    let profile = &profile;
    if !profile.supported {
        return Err(format!(
            "{} uses {}, which Unalia doesn't support yet.",
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
        .map_err(crate::support::logged)?;
    Ok(app_id)
}

/// Unreal game without a profile, in `dir` or one of its folders (people
/// often pick the parent folder), found on Nexus from the name of its
/// folder or executable. Failure: the name looked up, if it really was an
/// Unreal game.
async fn find_auto_game(catalog: &Catalog, dir: &Path) -> Result<(GameProfile, PathBuf), Option<String>> {
    let mut dirs = vec![dir.to_path_buf()];
    dirs.extend(
        fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .map(|e| e.path()),
    );
    let Some(game_dir) = dirs.into_iter().find(|d| detect_engine(d) == Engine::Unreal) else {
        return Err(None);
    };
    let names = auto::candidate_names(&game_dir);
    let found = catalog.find_nexus_games(&names).await;
    found
        .into_iter()
        .flatten()
        .find_map(|f| auto::unreal_profile(&f, &game_dir))
        .map(|profile| (profile, game_dir.clone()))
        .ok_or_else(|| names.first().cloned())
}

/// "Play goes through Steam" setting of a manually added game.
pub(crate) fn launch_setting(app_id: u32) -> String {
    format!("launch_via_steam:{app_id}")
}

/// Ticks or unticks "Launch through Steam" for a manually added game.
#[tauri::command]
async fn set_launch_via_steam(
    library: State<'_, Library>,
    mods: State<'_, Mods>,
    app_id: u32,
    enabled: bool,
) -> Result<(), String> {
    mods.state()
        .lock()
        .await
        .set_setting(
            &launch_setting(app_id),
            Some(if enabled { "1" } else { "0" }),
        )
        .map_err(crate::support::logged)?;
    // Applied right away, without waiting for a new scan.
    let root = library.steam_root();
    if let Some(game) = library.games.lock().unwrap().get_mut(&app_id) {
        game.steam_shortcut = if enabled {
            root.and_then(|root| {
                let all = shortcuts::steam_shortcuts(&root);
                shortcuts::shortcut_for(&all, &game.dir).map(|s| s.launch_url())
            })
        } else {
            None
        };
    }
    Ok(())
}

/// "Game version" setting: the one the user stayed on.
pub(crate) fn version_setting(app_id: u32) -> String {
    format!("game_version:{app_id}")
}

/// Installed game version, when the user went back to an old one (`None`:
/// the latest). The next installs and updates take the files published for
/// it.
#[tauri::command]
async fn set_game_version(
    library: State<'_, Library>,
    mods: State<'_, Mods>,
    app_id: u32,
    version: Option<String>,
) -> Result<(), String> {
    let game = library
        .game(app_id)
        .ok_or("This game isn't in your library anymore.")?;
    if let Some(version) = &version
        && !game.profile.versions.iter().any(|v| &v.name == version)
    {
        return Err(format!(
            "Unalia doesn't know version {version} of this game."
        ));
    }
    mods.state()
        .lock()
        .await
        .set_setting(&version_setting(app_id), version.as_deref())
        .map_err(crate::support::logged)?;
    if let Some(game) = library.games.lock().unwrap().get_mut(&app_id) {
        game.game_version = version;
    }
    Ok(())
}

/// Adds a manually added game to Steam (non-Steam game), with its name and
/// its images, to launch it through Steam and get its overlay. Steam keeps
/// its shortcuts in memory and rewrites them on exit: if it is open, it is
/// closed cleanly (`steam://exit`) then opened again.
#[tauri::command]
async fn add_to_steam(
    app: AppHandle,
    library: State<'_, Library>,
    mods: State<'_, Mods>,
    app_id: u32,
) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;

    let game = library
        .game(app_id)
        .ok_or("This game isn't in your library anymore.")?;
    let root = library
        .steam_root()
        .ok_or("Steam isn't installed on this computer.")?;
    let config = shortcuts::current_user_config(&root)
        .ok_or("Unalia couldn't tell which Steam account to use.\nOpen Steam once and log in, then try again.")?;
    let Some(LaunchTarget::Executable(exe)) = launch::executable_target(&game.dir, &game.exe_names)
    else {
        return Err("Unalia couldn't find the game's executable in its folder.".into());
    };

    // Steam open: close it and wait until it is really gone.
    let was_running = launch::steam_running(&mods.running_exes());
    if was_running {
        app.opener()
            .open_url("steam://exit", None::<&str>)
            .map_err(|e| format!("Couldn't ask Steam to close.\n{e}"))?;
        let mut closed = false;
        for _ in 0..60 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            if !launch::steam_running(&mods.running_exes()) {
                closed = true;
                break;
            }
        }
        if !closed {
            return Err("Steam didn't close. Close it yourself, then try again.".into());
        }
    }

    let added = shortcuts::add_shortcut(
        &config.join("shortcuts.vdf"),
        &shortcuts::NewShortcut {
            name: game.profile.name.clone(),
            start_dir: exe
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| game.dir.clone()),
            exe,
        },
    )
    .map_err(|e| format!("Couldn't add the game to Steam.\n{e}"))?;

    // Its images in the Steam library.
    for (kind, suffix) in [
        (ArtworkKind::Cover, "p"),
        (ArtworkKind::Hero, "_hero"),
        (ArtworkKind::Logo, "_logo"),
        (ArtworkKind::Header, ""),
    ] {
        if let Some(file) = artwork_file(&app, app_id, kind).await {
            let extension = file.extension().and_then(|e| e.to_str()).unwrap_or("jpg");
            let target = shortcuts::grid_image(&config, added.app_id, suffix, extension);
            if let Some(dir) = target.parent() {
                let _ = fs::create_dir_all(dir);
            }
            let _ = fs::copy(&file, target);
        }
    }

    if was_running {
        let _ = app.opener().open_url("steam://open/games", None::<&str>);
    }

    mods.state()
        .lock()
        .await
        .set_setting(&launch_setting(app_id), Some("1"))
        .map_err(crate::support::logged)?;
    if let Some(known) = library.games.lock().unwrap().get_mut(&app_id) {
        known.steam_shortcut = Some(added.launch_url());
    }
    Ok(())
}

/// Outside Steam, the date of the last update is the game executable's.
fn exe_modified(dir: &std::path::Path, exe_names: &[String]) -> Option<i64> {
    exe_names
        .iter()
        .filter_map(|name| fs::metadata(dir.join(name)).ok()?.modified().ok())
        .max()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
}

/// CSS accent color of the game, extracted from its cover.
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
    .map_err(crate::support::logged)?;
    // Nothing in Steam's cache: cover published by Thunderstore.
    if accent.is_none()
        && let Some(cover) = artwork_file(&app, app_id, ArtworkKind::Cover).await
    {
        accent = tauri::async_runtime::spawn_blocking(move || {
            artwork::accent_from_path(&cover).map(|a| a.css())
        })
        .await
        .map_err(crate::support::logged)?;
    }

    library
        .accents
        .lock()
        .unwrap()
        .insert(app_id, accent.clone());
    Ok(accent)
}

/// Image of a game, ready to display. A logo loses its transparent margins
/// (copy made once in the app cache), to fill its slot.
async fn artwork_file(app: &AppHandle, app_id: u32, kind: ArtworkKind) -> Option<PathBuf> {
    let file = source_artwork(app, app_id, kind).await?;
    if kind != ArtworkKind::Logo {
        return Some(file);
    }
    let trimmed = app
        .path()
        .app_cache_dir()
        .ok()?
        .join("artwork")
        .join(format!("{app_id}-logo-trimmed.png"));
    let fresh = |t: &Path| {
        let modified = |p: &Path| fs::metadata(p).and_then(|m| m.modified()).ok();
        modified(t).is_some_and(|t| modified(&file).is_none_or(|s| t >= s))
    };
    if fresh(&trimmed) {
        return Some(trimmed);
    }
    let (source, target) = (file.clone(), trimmed.clone());
    let done = tauri::async_runtime::spawn_blocking(move || {
        fs::create_dir_all(target.parent()?).ok()?;
        artwork::trim_logo(&source, &target).ok()
    })
    .await
    .ok()
    .flatten();
    Some(if done == Some(true) { trimmed } else { file })
}

/// Image of a game: Steam's cache first, else the image published by
/// Thunderstore for mod managers, downloaded once into the app cache.
async fn source_artwork(app: &AppHandle, app_id: u32, kind: ArtworkKind) -> Option<PathBuf> {
    let library = app.state::<Library>();
    if let Some(root) = library.steam_root()
        && let Some(file) = artwork::find_artwork(&root, app_id, kind)
    {
        return Some(file);
    }
    let remote = library
        .remote_art
        .lock()
        .unwrap()
        .get(&app_id)
        .cloned()
        .unwrap_or_default();
    let thunderstore = match kind {
        ArtworkKind::Cover => remote.cover.map(|u| (u, "cover")),
        ArtworkKind::Hero | ArtworkKind::HeroBlur => remote.hero.map(|u| (u, "hero")),
        ArtworkKind::Header | ArtworkKind::Logo => None,
    };
    // Otherwise, the public images of the game's Steam page: useful for a
    // game not opened in Steam yet, or a GOG or Epic copy of a game also
    // sold on Steam.
    let (url, name) = match thunderstore {
        Some(found) => found,
        None => steam_store_art(&app.state::<Catalog>().client, app_id, kind).await?,
    };
    // Without the query (`?t=...` of Steam store images).
    let extension = url
        .split(['?', '#'])
        .next()
        .unwrap_or(&url)
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
        // One image: a single connection, 20 MB at most.
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

/// Public image of a game's Steam page. Games without a Steam id have a
/// derived id with the high bit set (`manual_app_id`): nothing to look up
/// for them.
///
/// Recent games store their images under a hash
/// (`.../apps/<appid>/<hash>/library_hero.jpg`): the public store API
/// gives these paths. It doesn't know the logo, nor some old games: we then
/// fall back on the old URL, without a hash.
async fn steam_store_art(
    client: &mod_engine::HttpClient,
    app_id: u32,
    kind: ArtworkKind,
) -> Option<(String, &'static str)> {
    if app_id & 0x8000_0000 != 0 {
        return None;
    }
    let (asset, file, name) = match kind {
        ArtworkKind::Cover => (Some("library_capsule_2x"), "library_600x900_2x.jpg", "steam-cover"),
        ArtworkKind::Hero | ArtworkKind::HeroBlur => (Some("library_hero"), "library_hero.jpg", "steam-hero"),
        ArtworkKind::Header => (Some("header"), "header.jpg", "steam-header"),
        ArtworkKind::Logo => (None, "logo.png", "steam-logo"),
    };
    if let Some(asset) = asset
        && let Some(url) = store_asset_url(client, app_id, asset).await
    {
        return Some((url, name));
    }
    Some((
        format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{app_id}/{file}"),
        name,
    ))
}

/// URL of a Steam store image (`IStoreBrowseService/GetItems`, no key).
async fn store_asset_url(client: &mod_engine::HttpClient, app_id: u32, asset: &str) -> Option<String> {
    let input = serde_json::json!({
        "ids": [{ "appid": app_id }],
        "context": { "language": "english", "country_code": "US" },
        "data_request": { "include_assets": true },
    });
    let response: serde_json::Value = client
        .get("https://api.steampowered.com/IStoreBrowseService/GetItems/v1/")
        .query(&[("input_json", input.to_string())])
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()?;
    let assets = &response["response"]["store_items"][0]["assets"];
    let format = assets["asset_url_format"].as_str()?;
    let path = assets[asset].as_str()?;
    // A plain relative path: nothing that goes up or changes host.
    if path.contains("..") || path.contains("://") || path.starts_with('/') {
        return None;
    }
    Some(format!(
        "https://shared.akamai.steamstatic.com/store_item_assets/{}",
        format.replace("${FILENAME}", path)
    ))
}

/// In dev, `MODALIA_STEAM_ROOT` replaces the automatic lookup (e.g. to
/// point at `fixtures/steam/root`). Ignored in release builds.
fn steam_root() -> Option<PathBuf> {
    if cfg!(debug_assertions)
        && let Some(root) = std::env::var_os("MODALIA_STEAM_ROOT")
    {
        return Some(PathBuf::from(root));
    }
    detect::find_steam_root()
}

/// Folder of each installed Steam game. `None`: Steam not found or
/// unreadable, we don't know which games are there.
fn steam_dirs() -> Option<HashMap<u32, PathBuf>> {
    let games = detect::scan_steam(&steam_root()?).ok()?;
    Some(games.into_iter().map(|game| (game.app_id, game.install_dir)).collect())
}

/// Protocol `artwork://localhost/<appid>/<type>` (Windows:
/// `http://artwork.localhost/...`). Only serves game images (Steam's cache
/// or Thunderstore fallback): the UI never handles a path.
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
    let context = tauri::generate_context!();
    // On Windows, the WebView creates its folder in the local data as soon
    // as the window opens, before `setup`: the old folder is taken over
    // before that (UI settings included).
    #[cfg(windows)]
    for var in ["LOCALAPPDATA", "APPDATA"] {
        if let Some(base) = std::env::var_os(var) {
            mods::adopt_old_dir(&Path::new(&base).join(&context.config().identifier));
        }
    }
    // Started by the Windows uninstaller with the "delete the data" box
    // ticked: the games get their original state back before the database
    // that lists the mods disappears. No window.
    #[cfg(windows)]
    if std::env::args().any(|arg| arg == "--remove-all-mods") {
        if let Some(base) = std::env::var_os("LOCALAPPDATA") {
            mods::remove_all(&Path::new(&base).join(&context.config().identifier), steam_dirs().unwrap_or_default());
        }
        return;
    }
    tauri::Builder::default()
        // First: an `nxm://` link opened while Modalia runs starts the app
        // again, which hands the link to the window already open.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(support::log_plugin())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(Library::default())
        .manage(app_update::AppUpdate::default())
        .setup(|app| {
            support::log_panics();
            log::info!("Unalia Mod Manager {} started", app.package_info().version);
            app.manage(Catalog::new(&app.path().app_cache_dir()?));
            app.manage(mods::Mods::open(app.handle())?);
            mods::tidy_storage(app.handle());
            mods::nexus_setup(app.handle());
            tauri::async_runtime::spawn(mods::watch(app.handle().clone()));
            Ok(())
        })
        .register_asynchronous_uri_scheme_protocol("artwork", artwork_protocol)
        .invoke_handler(tauri::generate_handler![
            list_games,
            add_manual_game,
            set_launch_via_steam,
            set_game_version,
            add_to_steam,
            game_accent,
            catalog::search_mods,
            catalog::get_mod,
            catalog::mod_description,
            mods::installed_mods,
            mods::install_mod,
            mods::cancel_install,
            mods::paused_downloads,
            mods::discard_download,
            mods::set_mod_enabled,
            mods::uninstall_mod,
            mods::restore_mod,
            mods::revert_mod,
            mods::missing_mods,
            mods::put_back_mods,
            mods::forgotten_games,
            mods::forget_game,
            mods::remove_manual_game,
            mods::reset_preview,
            mods::reset_game,
            mods::cancel_pending,
            mods::check_updates,
            mods::game_status,
            mods::play_game,
            mods::cancel_starting,
            mods::nexus_status,
            mods::nexus_login,
            mods::nexus_connect,
            mods::nexus_disconnect,
            support::diagnostics,
            support::open_logs_folder,
            support::open_license,
            support::can_uninstall,
            support::uninstall_app,
            app_update::check_app_update,
            app_update::install_app_update,
        ])
        .run(context)
        .expect("Unalia Mod Manager failed to start");
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

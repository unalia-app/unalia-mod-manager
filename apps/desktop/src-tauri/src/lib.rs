use std::path::PathBuf;

use mod_engine::detect::{self, SteamGame};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GameScan {
    /// `false` si aucune installation de Steam n'a été trouvée : l'accueil
    /// propose alors l'ajout manuel d'un dossier.
    steam_found: bool,
    games: Vec<SteamGame>,
}

#[tauri::command]
async fn list_games() -> Result<GameScan, String> {
    tauri::async_runtime::spawn_blocking(|| match steam_root() {
        Some(root) => Ok(GameScan {
            steam_found: true,
            games: detect::scan_steam(&root).map_err(|e| e.to_string())?,
        }),
        None => Ok(GameScan {
            steam_found: false,
            games: Vec::new(),
        }),
    })
    .await
    .map_err(|e| e.to_string())?
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![list_games])
        .run(tauri::generate_context!())
        .expect("échec du démarrage de Modalia");
}

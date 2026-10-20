//! Affiche les jeux détectés et leur accent, pour vérifier detect et artwork
//! sur un vrai dossier Steam :
//!
//!     cargo run -p mod-engine --example scan -- [dossier Steam]

use std::path::PathBuf;

use mod_engine::artwork::game_accent;
use mod_engine::detect::{find_steam_root, scan_steam};

fn main() {
    let Some(root) = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .or_else(find_steam_root)
    else {
        eprintln!("Aucun dossier Steam trouvé.");
        std::process::exit(1);
    };

    let games = scan_steam(&root).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });
    for game in games {
        let accent = game_accent(&root, game.app_id).map_or_else(|| "neutre".into(), |a| a.css());
        println!(
            "{:>8}  {:<8}  {:<26}  {}",
            game.app_id,
            format!("{:?}", game.engine),
            accent,
            game.name
        );
    }
}

//! Interroge le vrai Thunderstore : catalogue de jeux, recherche et fiche.
//!
//!     cargo run -p mod-engine --example mods -- [appid Steam] [recherche]

use std::time::Duration;

use mod_engine::games::{SCHEMA_URL, load_catalog};
use mod_engine::sources::thunderstore::Thunderstore;
use mod_engine::sources::{ModSource, SearchQuery};

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let app_id: u32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(1030300);
    let text = args.next().unwrap_or_default();

    let client = mod_engine::http_client();
    let cache = std::env::temp_dir().join("modalia-schema.json");
    let catalog = load_catalog(&client, SCHEMA_URL, &cache, Duration::from_secs(86_400))
        .await
        .expect("catalogue");
    println!("{} jeux dans le catalogue", catalog.games().len());

    let Some(game) = catalog.by_steam_app_id(app_id) else {
        println!("{app_id} : jeu inconnu de Thunderstore");
        return;
    };
    println!(
        "{} : communauté {}, loader {} (géré : {})",
        game.name, game.community, game.loader, game.supported
    );

    let ts = Thunderstore::new(client);
    let page = ts
        .search(
            &game.community,
            &SearchQuery {
                text,
                page: 1,
                ..Default::default()
            },
        )
        .await
        .expect("recherche");
    println!("{} mods, page 1 :", page.total);
    for m in &page.items {
        let loader = if catalog.loader_package(&m.id).is_some() {
            " [loader]"
        } else {
            ""
        };
        println!("  {:>9}  {}{loader}  par {}", m.downloads, m.name, m.author);
    }

    if let Some(first) = page
        .items
        .iter()
        .find(|m| catalog.loader_package(&m.id).is_none() && !m.pinned)
    {
        let m = ts.get(&game.community, &first.id).await.expect("fiche");
        let v = m.latest().unwrap();
        println!(
            "\n{} {} : {} dépendance(s)",
            m.name,
            v.number,
            v.dependencies.len()
        );
        for d in &v.dependencies {
            println!("  - {} ≥ {}", d.id, d.min_version);
        }
    }
}

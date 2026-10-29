//! Tests of the Hexium source against a fake server that replays real
//! answers recorded in `fixtures/hexium` (trimmed to a few mods).

use std::io::Write;
use std::path::Path;

use mockito::{Server, ServerGuard};
use mod_engine::sources::hexium::Hexium;
use mod_engine::sources::thunderstore::Thunderstore;
use mod_engine::sources::{Dependency, ModSource, SearchQuery, Sort};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures")
            .join(name),
    )
    .unwrap()
}

/// The catalog, gzip-encoded like Hexium serves it.
fn gzipped_catalog() -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder
        .write_all(fixture("hexium/chunk-valheim.json").as_bytes())
        .unwrap();
    encoder.finish().unwrap()
}

/// Hexium and the Thunderstore it falls back to, each on its fake server.
async fn source() -> (ServerGuard, ServerGuard, Hexium) {
    let hexium = Server::new_async().await;
    let thunderstore = Server::new_async().await;
    let source = Hexium::with_base_url(
        reqwest::Client::new(),
        &hexium.url(),
        Thunderstore::with_base_url(reqwest::Client::new(), &thunderstore.url()),
    );
    (hexium, thunderstore, source)
}

async fn serve_catalog(server: &mut ServerGuard) -> mockito::Mock {
    server
        .mock("GET", "/c/valheim/api/v1/package-listing-chunk/")
        .with_header("content-type", "application/octet-stream")
        .with_body(gzipped_catalog())
        .create_async()
        .await
}

#[tokio::test]
async fn search_runs_on_the_downloaded_catalog() {
    let (mut server, _ts, hexium) = source().await;
    // Downloaded once for every search of the session.
    let catalog = serve_catalog(&mut server).await.expect(1);

    let page = hexium
        .search("valheim", &SearchQuery::default())
        .await
        .unwrap();
    assert_eq!(page.total, 3);
    assert!(!page.has_more);
    // "Popular": the best rated first.
    let first = &page.items[0];
    assert_eq!(first.id, "Azumatt-AzuCraftyBoxes");
    assert_eq!(first.name, "AzuCraftyBoxes");
    assert_eq!(first.author, "Azumatt");
    assert_eq!(first.source, "hexium");
    assert_eq!(first.downloads, 55713);
    assert!(first.icon_url.as_deref().unwrap().starts_with("https://"));

    let newest = hexium
        .search(
            "valheim",
            &SearchQuery {
                sort: Sort::Newest,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(newest.items[0].id, "CDEVx-CDEV_BeRightBack");

    // Every word must match the name, the author or the description.
    let found = hexium
        .search(
            "valheim",
            &SearchQuery {
                text: "be right".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(found.total, 1);
    assert_eq!(found.items[0].id, "CDEVx-CDEV_BeRightBack");

    let page_two = hexium
        .search(
            "valheim",
            &SearchQuery {
                page: 2,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(page_two.items.is_empty());
    catalog.assert_async().await;
}

#[tokio::test]
async fn get_gives_every_version_with_its_dependencies() {
    let (mut server, _ts, hexium) = source().await;
    serve_catalog(&mut server).await;

    let found = hexium
        .get("valheim", "Azumatt-AzuCraftyBoxes")
        .await
        .unwrap();
    assert_eq!(found.source, "hexium");
    assert!(
        found
            .source_url
            .starts_with("https://valheim.hexium.gg/mods/")
    );
    let versions: Vec<&str> = found.versions.iter().map(|v| v.number.as_str()).collect();
    assert_eq!(versions, ["1.8.28", "1.8.27"]);
    let latest = found.latest().unwrap();
    assert!(latest.download_url.starts_with("https://cdn.hexium.gg/"));
    assert_eq!(
        latest.dependencies,
        [Dependency {
            id: "denikson-BepInExPack_Valheim".into(),
            min_version: "5.4.2360".into(),
        }]
    );
}

#[tokio::test]
async fn mods_missing_from_hexium_come_from_thunderstore() {
    let (mut server, mut ts, hexium) = source().await;
    serve_catalog(&mut server).await;
    let thunderstore = ts
        .mock(
            "GET",
            "/api/experimental/package/shownyoung/KnightInSilksong/",
        )
        .with_body(fixture("thunderstore/package-knightinsilksong.json"))
        .create_async()
        .await;
    ts.mock("GET", "/api/cyberstorm/team/shownyoung/")
        .with_status(404)
        .create_async()
        .await;

    let found = hexium
        .get("valheim", "shownyoung-KnightInSilksong")
        .await
        .unwrap();
    thunderstore.assert_async().await;
    assert_eq!(found.source, "thunderstore");
}

#[tokio::test]
async fn communities_are_asked_once() {
    let (mut server, _ts, hexium) = source().await;
    let mock = server
        .mock("GET", "/api/experimental/community/")
        .with_body(fixture("hexium/communities.json"))
        .expect(1)
        .create_async()
        .await;

    for _ in 0..2 {
        let communities = hexium.communities().await.unwrap();
        assert!(communities.iter().any(|c| c == "valheim"));
    }
    mock.assert_async().await;
}

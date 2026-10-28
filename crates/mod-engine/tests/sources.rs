//! Tests of the Thunderstore source against a fake server that replays
//! real answers recorded in `fixtures/thunderstore`.

use std::path::Path;

use mockito::{Matcher, Server, ServerGuard};
use mod_engine::sources::thunderstore::Thunderstore;
use mod_engine::sources::{Dependency, ModSource, SearchQuery, Sort, SourceError};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/thunderstore")
            .join(name),
    )
    .unwrap()
}

async fn source() -> (ServerGuard, Thunderstore) {
    let server = Server::new_async().await;
    let source = Thunderstore::with_base_url(reqwest::Client::new(), &server.url());
    (server, source)
}

#[tokio::test]
async fn search_maps_the_listing_to_neutral_summaries() {
    let (mut server, ts) = source().await;
    let mock = server
        .mock("GET", "/api/cyberstorm/listing/hollow-knight-silksong/")
        .match_query(Matcher::AllOf(vec![
            // "Popular" by default: the best rated.
            Matcher::UrlEncoded("ordering".into(), "top-rated".into()),
            Matcher::UrlEncoded("page".into(), "1".into()),
        ]))
        .with_body(fixture("listing-silksong.json"))
        .create_async()
        .await;

    let page = ts
        .search("hollow-knight-silksong", &SearchQuery::default())
        .await
        .unwrap();
    mock.assert_async().await;

    assert_eq!(page.total, 437);
    assert!(page.has_more);
    assert_eq!(page.items.len(), 6);

    let bepinex = &page.items[1];
    assert_eq!(bepinex.id, "silksong_modding-BepInExPack_Silksong");
    assert_eq!(bepinex.name, "BepInExPack Silksong");
    assert_eq!(bepinex.author, "silksong_modding");
    assert_eq!(bepinex.source, "thunderstore");
    assert!(bepinex.pinned);
    assert!(bepinex.downloads > 0);
    assert!(bepinex.icon_url.as_deref().unwrap().starts_with("https://"));
    assert!(
        bepinex
            .categories
            .iter()
            .all(|c| !c.starts_with("(Deprecated"))
    );
}

#[tokio::test]
async fn search_excludes_libraries_by_default() {
    let (mut server, ts) = source().await;
    let filters = server
        .mock(
            "GET",
            "/api/cyberstorm/community/hollow-knight-silksong/filters/",
        )
        .with_body(fixture("filters-silksong.json"))
        .expect(1)
        .create_async()
        .await;
    let excluded = server
        .mock("GET", "/api/cyberstorm/listing/hollow-knight-silksong/")
        .match_query(Matcher::UrlEncoded(
            "excluded_categories".into(),
            "1615".into(),
        ))
        .with_body(fixture("listing-silksong.json"))
        .expect(2)
        .create_async()
        .await;

    // Two searches: the filters are only requested once.
    for _ in 0..2 {
        ts.search("hollow-knight-silksong", &SearchQuery::default())
            .await
            .unwrap();
    }
    filters.assert_async().await;
    excluded.assert_async().await;
}

#[tokio::test]
async fn libraries_can_be_included_and_filters_are_optional() {
    let (mut server, ts) = source().await;
    // No filter route: the search works anyway, without a filter.
    let plain = server
        .mock("GET", "/api/cyberstorm/listing/hollow-knight-silksong/")
        .match_query(Matcher::Any)
        .match_request(|req| !req.path_and_query().contains("excluded_categories"))
        .with_body(fixture("listing-silksong.json"))
        .expect(2)
        .create_async()
        .await;

    ts.search("hollow-knight-silksong", &SearchQuery::default())
        .await
        .unwrap();
    let all = SearchQuery {
        include_libraries: true,
        ..Default::default()
    };
    ts.search("hollow-knight-silksong", &all).await.unwrap();
    plain.assert_async().await;
}

#[tokio::test]
async fn search_sends_text_sort_and_page() {
    let (mut server, ts) = source().await;
    let mock = server
        .mock("GET", "/api/cyberstorm/listing/hollow-knight-silksong/")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("q".into(), "skip intro".into()),
            Matcher::UrlEncoded("ordering".into(), "last-updated".into()),
            Matcher::UrlEncoded("page".into(), "3".into()),
        ]))
        .with_body(r#"{"count":0,"next":null,"previous":null,"results":[]}"#)
        .create_async()
        .await;

    let query = SearchQuery {
        text: "  skip intro ".into(),
        sort: Sort::Updated,
        page: 3,
        ..Default::default()
    };
    let page = ts.search("hollow-knight-silksong", &query).await.unwrap();
    mock.assert_async().await;
    assert!(page.items.is_empty());
    assert!(!page.has_more);
}

#[tokio::test]
async fn get_returns_the_latest_version_and_its_dependencies() {
    let (mut server, ts) = source().await;
    server
        .mock(
            "GET",
            "/api/experimental/package/shownyoung/KnightInSilksong/",
        )
        .with_body(fixture("package-knightinsilksong.json"))
        .create_async()
        .await;

    let m = ts
        .get("hollow-knight-silksong", "shownyoung-KnightInSilksong")
        .await
        .unwrap();

    assert_eq!(m.id, "shownyoung-KnightInSilksong");
    assert_eq!(m.name, "KnightInSilksong");
    assert_eq!(m.author, "shownyoung");
    assert_eq!(m.categories, ["Mods"]);
    assert_eq!(m.downloads, None, "total_downloads is -1 in the response");
    assert!(
        m.source_url
            .ends_with("/c/hollow-knight-silksong/p/shownyoung/KnightInSilksong/")
    );
    assert_eq!(
        m.website_url.as_deref(),
        Some("https://github.com/MCXGK3/KnightInSilkSong")
    );

    let latest = m.latest().unwrap();
    assert_eq!(latest.number, "0.6.7");
    assert!(
        latest
            .download_url
            .starts_with("https://thunderstore.io/package/download/")
    );
    assert_eq!(latest.sha256, None);
    assert_eq!(
        latest.dependencies,
        [
            Dependency {
                id: "silksong_modding-BepInExPack_Silksong".into(),
                min_version: "1.0.3".into()
            },
            Dependency {
                id: "silksong_modding-SilksongPrepatcher".into(),
                min_version: "1.3.0".into()
            },
            Dependency {
                id: "silksong_modding-I18N".into(),
                min_version: "1.0.3".into()
            },
        ]
    );
}

#[tokio::test]
async fn description_returns_the_raw_markdown() {
    let (mut server, ts) = source().await;
    server
        .mock(
            "GET",
            "/api/experimental/package/Bigfootmech/Silksong_Skip_Intro/0.1.0/readme/",
        )
        .with_body(fixture("readme-skip-intro.json"))
        .create_async()
        .await;

    let markdown = ts
        .description(
            "hollow-knight-silksong",
            "Bigfootmech-Silksong_Skip_Intro",
            "0.1.0",
        )
        .await
        .unwrap();
    assert!(markdown.contains("Skip the Team Cherry"));
    // Thunderstore's Markdown contains raw HTML: the UI has to sanitize
    // it, the engine passes it on as is.
    assert!(markdown.contains("<h1>"));
}

#[tokio::test]
async fn missing_mods_are_not_found() {
    let (mut server, ts) = source().await;
    server
        .mock("GET", "/api/experimental/package/ghost/Nothing/")
        .with_status(404)
        .create_async()
        .await;

    let err = ts
        .get("hollow-knight-silksong", "ghost-Nothing")
        .await
        .unwrap_err();
    assert!(matches!(err, SourceError::NotFound { .. }), "{err:?}");
}

#[tokio::test]
async fn malformed_ids_never_reach_the_network() {
    let (server, ts) = source().await;
    for id in ["../../admin", "a-b/../c", "nodash", "ns-na me"] {
        let err = ts.get("hollow-knight-silksong", id).await.unwrap_err();
        assert!(matches!(err, SourceError::NotFound { .. }), "{id}: {err:?}");
    }
    let err = ts
        .search("../x", &SearchQuery::default())
        .await
        .unwrap_err();
    assert!(matches!(err, SourceError::NotFound { .. }));
    drop(server);
}

#[tokio::test]
async fn server_errors_and_garbage_are_reported() {
    let (mut server, ts) = source().await;
    server
        .mock("GET", "/api/experimental/package/a/b/")
        .with_status(503)
        .create_async()
        .await;
    server
        .mock("GET", "/api/experimental/package/c/d/")
        .with_body("<html>Cloudflare</html>")
        .create_async()
        .await;

    let down = ts.get("x", "a-b").await.unwrap_err();
    assert!(matches!(down, SourceError::Network { .. }), "{down:?}");
    assert_eq!(down.to_string(), "Thunderstore can't be reached");

    let garbage = ts.get("x", "c-d").await.unwrap_err();
    assert!(
        matches!(garbage, SourceError::Unexpected { .. }),
        "{garbage:?}"
    );
}

#[tokio::test]
async fn insecure_download_urls_are_refused() {
    let (mut server, ts) = source().await;
    let ts = ts.require_https(true);
    let body = fixture("package-skip-intro.json").replace(
        "https://thunderstore.io/package/download",
        "http://evil.example",
    );
    server
        .mock(
            "GET",
            "/api/experimental/package/Bigfootmech/Silksong_Skip_Intro/",
        )
        .with_body(body)
        .create_async()
        .await;

    let err = ts
        .get("hollow-knight-silksong", "Bigfootmech-Silksong_Skip_Intro")
        .await
        .unwrap_err();
    assert!(matches!(err, SourceError::Unexpected { .. }), "{err:?}");
}

#[tokio::test]
async fn donation_links_come_from_the_author_team() {
    let (mut server, ts) = source().await;
    let body = fixture("package-skip-intro.json")
        .replace("\"Bigfootmech\"", "\"MorningRain\"")
        .replace("Bigfootmech-", "MorningRain-");
    server
        .mock(
            "GET",
            "/api/experimental/package/MorningRain/Silksong_Skip_Intro/",
        )
        .with_body(body)
        .create_async()
        .await;
    server
        .mock("GET", "/api/cyberstorm/team/MorningRain/")
        .with_body(fixture("team-morningrain.json"))
        .create_async()
        .await;

    let m = ts
        .get("hollow-knight-silksong", "MorningRain-Silksong_Skip_Intro")
        .await
        .unwrap();
    assert_eq!(
        m.donation_url.as_deref(),
        Some("https://ko-fi.com/morning_rain")
    );
}

#[tokio::test]
async fn a_missing_team_never_breaks_the_mod_page() {
    let (mut server, ts) = source().await;
    server
        .mock(
            "GET",
            "/api/experimental/package/shownyoung/KnightInSilksong/",
        )
        .with_body(fixture("package-knightinsilksong.json"))
        .create_async()
        .await;
    // No team route: 501 from the fake server.
    let m = ts
        .get("hollow-knight-silksong", "shownyoung-KnightInSilksong")
        .await
        .unwrap();
    assert_eq!(m.donation_url, None);
}

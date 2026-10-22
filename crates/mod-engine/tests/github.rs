//! Releases GitHub : cache, revalidation par ETag et limite horaire, contre
//! un faux GitHub. Sans compte, GitHub n'accorde que 60 requêtes par heure.

use std::time::Duration;

use mockito::Matcher;
use mod_engine::sources::SourceError;
use mod_engine::sources::github::GitHub;
use serde_json::json;

const PATH: &str = "/repos/wopss/RED4ext/releases/latest";

fn release(tag: &str) -> String {
    json!({
        "tag_name": tag,
        "published_at": "2026-10-22T00:00:00Z",
        "assets": [{ "name": format!("red4ext-{}.zip", tag.trim_start_matches('v')),
                     "browser_download_url": "http://example.test/r.zip", "size": 1 }]
    })
    .to_string()
}

#[tokio::test]
async fn a_fresh_answer_is_served_from_the_cache() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", PATH)
        .with_body(release("v1.30.0"))
        .expect(1)
        .create_async()
        .await;
    let github = GitHub::with_url(reqwest::Client::new(), &server.url());

    for _ in 0..3 {
        assert_eq!(
            github
                .latest_release("wopss/RED4ext")
                .await
                .unwrap()
                .version,
            "1.30.0"
        );
    }
    mock.assert_async().await;
}

#[tokio::test]
async fn an_old_answer_is_revalidated_with_its_etag() {
    let mut server = mockito::Server::new_async().await;
    let first = server
        .mock("GET", PATH)
        .match_header("if-none-match", Matcher::Missing)
        .with_header("etag", "\"abc\"")
        .with_body(release("v1.30.0"))
        .expect(1)
        .create_async()
        .await;
    let unchanged = server
        .mock("GET", PATH)
        .match_header("if-none-match", "\"abc\"")
        .with_status(304)
        .expect(1)
        .create_async()
        .await;
    let github = GitHub::with_url(reqwest::Client::new(), &server.url()).with_ttl(Duration::ZERO);

    github.latest_release("wopss/RED4ext").await.unwrap();
    let again = github.latest_release("wopss/RED4ext").await.unwrap();
    assert_eq!(again.version, "1.30.0");
    first.assert_async().await;
    unchanged.assert_async().await;
}

#[tokio::test]
async fn the_hourly_limit_serves_the_last_known_release_or_says_when_to_retry() {
    let mut server = mockito::Server::new_async().await;
    let reset = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 47 * 60;
    server
        .mock("GET", PATH)
        .with_status(403)
        .with_header("x-ratelimit-remaining", "0")
        .with_header("x-ratelimit-reset", &reset.to_string())
        .with_body(r#"{"message":"API rate limit exceeded"}"#)
        .create_async()
        .await;

    // Rien en cache : on dit quand réessayer.
    let github = GitHub::with_url(reqwest::Client::new(), &server.url()).with_ttl(Duration::ZERO);
    let err = github.latest_release("wopss/RED4ext").await.unwrap_err();
    match err {
        SourceError::RateLimited { minutes, .. } => {
            assert!((46..=48).contains(&minutes), "{minutes}")
        }
        other => panic!("{other:?}"),
    }

    // Une release connue (sur disque) : elle sert en attendant.
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("github.json");
    let mut healthy = mockito::Server::new_async().await;
    healthy
        .mock("GET", PATH)
        .with_body(release("v1.30.0"))
        .create_async()
        .await;
    GitHub::with_url(reqwest::Client::new(), &healthy.url())
        .with_cache_file(file.clone())
        .latest_release("wopss/RED4ext")
        .await
        .unwrap();
    let limited = GitHub::with_url(reqwest::Client::new(), &server.url())
        .with_ttl(Duration::ZERO)
        .with_cache_file(file);
    assert_eq!(
        limited
            .latest_release("wopss/RED4ext")
            .await
            .unwrap()
            .version,
        "1.30.0"
    );
}

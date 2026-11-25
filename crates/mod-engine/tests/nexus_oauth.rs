//! Nexus OAuth login, against a fake Nexus that follows the guide it
//! publishes. The test plays the browser.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use mockito::{Matcher, Mock, ServerGuard};
use mod_engine::sources::SourceError;
use mod_engine::sources::nexus::Nexus;
use mod_engine::sources::nexus_oauth::{OAuth, OAuthError, Session, Tokens};
use reqwest::Url;
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::sync::oneshot;

const VERSION: &str = "9.8.7";

fn jwt(name: &str, roles: &[&str]) -> String {
    let claims = json!({ "sub": "1", "user": { "username": name, "membership_roles": roles } });
    format!("e30.{}.c2ln", URL_SAFE_NO_PAD.encode(claims.to_string()))
}

fn reply(access: &str, refresh: &str, expires_in: i64) -> String {
    json!({
        "access_token": access, "refresh_token": refresh,
        "expires_in": expires_in, "token_type": "Bearer",
    })
    .to_string()
}

fn field(name: &str, value: &str) -> Matcher {
    Matcher::UrlEncoded(name.into(), value.into())
}

fn oauth(server: &ServerGuard) -> OAuth {
    OAuth::with_url(reqwest::Client::new(), &server.url(), "unalia", VERSION)
}

/// What the session asked to keep: `None` is a forgotten login.
type Kept = Arc<Mutex<Vec<Option<String>>>>;

fn session(server: &ServerGuard, refresh: &str) -> (Arc<Session>, Kept) {
    let kept = Kept::default();
    let list = Arc::clone(&kept);
    let session = Session::new(
        oauth(server),
        Tokens::from_refresh(refresh.into()),
        move |tokens| {
            list.lock()
                .unwrap()
                .push(tokens.map(|t| t.refresh_token().to_owned()));
        },
    );
    (session, kept)
}

fn nexus(server: &ServerGuard, session: &Arc<Session>) -> Nexus {
    let nexus = Nexus::with_urls(
        reqwest::Client::new(),
        &format!("{}/graphql", server.url()),
        &format!("{}/v1", server.url()),
    )
    .with_app_version(VERSION);
    nexus.set_session(Some(Arc::clone(session)));
    nexus
}

async fn renewal(server: &mut ServerGuard, refresh: &str, status: usize, body: &str) -> Mock {
    server
        .mock("POST", "/token")
        .match_body(Matcher::AllOf(vec![
            field("grant_type", "refresh_token"),
            field("client_id", "unalia"),
            field("refresh_token", refresh),
        ]))
        .with_status(status)
        .with_body(body)
        .create_async()
        .await
}

/// Starts a login and gives the parameters of the page it opens.
async fn start(
    oauth: OAuth,
) -> (
    HashMap<String, String>,
    tokio::task::JoinHandle<Result<Tokens, OAuthError>>,
) {
    let (send, opened) = oneshot::channel();
    let login = tokio::spawn(async move {
        oauth
            .login(move |url| {
                let _ = send.send(url);
            })
            .await
    });
    let page = Url::parse(&opened.await.unwrap()).unwrap();
    assert_eq!(page.path(), "/authorize");
    (page.query_pairs().into_owned().collect(), login)
}

async fn browse(url: &str) -> reqwest::StatusCode {
    reqwest::get(url).await.unwrap().status()
}

#[tokio::test]
async fn the_tokens_come_back_once_the_user_authorises() {
    let mut server = mockito::Server::new_async().await;
    let (page, login) = start(oauth(&server)).await;
    assert_eq!(page["client_id"], "unalia");
    assert_eq!(page["response_type"], "code");
    assert_eq!(page["code_challenge_method"], "S256");
    assert_eq!(page["scope"], "");
    let redirect = page["redirect_uri"].clone();
    assert!(redirect.starts_with("http://127.0.0.1:"), "{redirect}");

    // The verifier sent at the end must be the one of the challenge.
    let challenge = page["code_challenge"].clone();
    let exchange = server
        .mock("POST", "/token")
        .match_header("application-name", "Unalia Mod Manager")
        .match_header("application-version", VERSION)
        .match_request(move |request| {
            let body = request.utf8_lossy_body().unwrap().into_owned();
            let form: HashMap<String, String> = Url::parse(&format!("http://form/?{body}"))
                .unwrap()
                .query_pairs()
                .into_owned()
                .collect();
            let verifier = &form["code_verifier"];
            form["grant_type"] == "authorization_code"
                && form["code"] == "the code"
                && form["client_id"] == "unalia"
                && form["redirect_uri"] == redirect
                && (43..=128).contains(&verifier.len())
                && URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())) == challenge
        })
        .with_body(reply(&jwt("Reverss", &["member"]), "refresh-1", 21600))
        .create_async()
        .await;
    let status = browse(&format!(
        "{}?code=the+code&state={}",
        page["redirect_uri"], page["state"]
    ))
    .await;
    assert_eq!(status, 200);
    let tokens = login.await.unwrap().unwrap();
    assert_eq!(tokens.refresh_token(), "refresh-1");
    exchange.assert_async().await;

    // Nothing secret in a log.
    let debug = format!("{tokens:?}");
    assert!(!debug.contains("refresh-1") && !debug.contains("e30"), "{debug}");

    let account = Session::new(oauth(&server), tokens, |_| {})
        .account()
        .await
        .unwrap();
    assert_eq!(account.name, "Reverss");
    assert!(!account.premium);
}

#[tokio::test]
async fn only_the_answer_to_this_login_counts() {
    let mut server = mockito::Server::new_async().await;
    let (page, login) = start(oauth(&server)).await;
    let (redirect, state) = (&page["redirect_uri"], &page["state"]);
    let port = Url::parse(redirect).unwrap().port().unwrap();

    // A connection the browser opens in advance, and never uses.
    let _idle = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    assert_eq!(browse(&format!("http://127.0.0.1:{port}/favicon.ico")).await, 404);
    assert_eq!(browse(&format!("{redirect}?code=theirs&state=other")).await, 404);
    assert_eq!(browse(&format!("{redirect}?code=theirs")).await, 404);
    assert!(!login.is_finished());

    server
        .mock("POST", "/token")
        .match_body(field("code", "ours"))
        .with_body(reply(&jwt("Reverss", &[]), "refresh-1", 21600))
        .create_async()
        .await;
    assert_eq!(browse(&format!("{redirect}?code=ours&state={state}")).await, 200);
    login.await.unwrap().unwrap();
}

#[tokio::test]
async fn a_refusal_is_reported() {
    let server = mockito::Server::new_async().await;
    let (page, login) = start(oauth(&server)).await;
    let callback = format!("{}?state={}", page["redirect_uri"], page["state"]);
    browse(&format!("{callback}&error=access_denied")).await;
    let error = login.await.unwrap().unwrap_err();
    assert!(matches!(error, OAuthError::Declined), "{error:?}");

    let (page, login) = start(oauth(&server)).await;
    let callback = format!("{}?state={}", page["redirect_uri"], page["state"]);
    browse(&format!("{callback}&error=invalid_scope&error_description=Bad+scope")).await;
    let error = login.await.unwrap().unwrap_err();
    assert!(matches!(error, OAuthError::Refused(ref m) if m == "Bad scope"), "{error:?}");
}

#[tokio::test]
async fn downloads_use_the_login_and_the_app_version() {
    let mut server = mockito::Server::new_async().await;
    let access = jwt("Reverss", &["member", "premium"]);
    let renewed = renewal(&mut server, "refresh-1", 200, &reply(&access, "refresh-2", 21600))
        .await
        .expect(1);
    let download = server
        .mock("GET", "/v1/games/cyberpunk2077/mods/1/files/2/download_link.json")
        .match_header("authorization", format!("Bearer {access}").as_str())
        .match_header("apikey", Matcher::Missing)
        .match_header("application-name", "Unalia Mod Manager")
        .match_header("application-version", VERSION)
        .with_body(json!([{ "URI": "http://cdn/file.zip" }]).to_string())
        .expect(2)
        .create_async()
        .await;
    let (session, kept) = session(&server, "refresh-1");
    let nexus = nexus(&server, &session);

    // Two downloads at once: a refresh token only works once.
    let (a, b) = tokio::join!(
        nexus.download_url("cyberpunk2077", 1, 2, None),
        nexus.download_url("cyberpunk2077", 1, 2, None),
    );
    assert_eq!(a.unwrap(), "http://cdn/file.zip");
    assert_eq!(b.unwrap(), "http://cdn/file.zip");
    assert!(nexus.account().await.unwrap().premium);
    renewed.assert_async().await;
    download.assert_async().await;
    assert_eq!(*kept.lock().unwrap(), [Some("refresh-2".to_owned())]);
}

#[tokio::test]
async fn metadata_requests_send_the_app_version() {
    let mut server = mockito::Server::new_async().await;
    let graphql = server
        .mock("POST", "/graphql")
        .match_header("application-name", "Unalia Mod Manager")
        .match_header("application-version", VERSION)
        .with_body(json!({ "data": { "g0": { "nodes": [] } } }).to_string())
        .create_async()
        .await;
    let (session, _) = session(&server, "refresh-1");
    nexus(&server, &session)
        .find_games(&["Some Game".into()])
        .await
        .unwrap();
    graphql.assert_async().await;
}

#[tokio::test]
async fn a_login_revoked_on_nexus_is_forgotten() {
    let mut server = mockito::Server::new_async().await;
    let body = json!({ "error": "invalid_grant" }).to_string();
    renewal(&mut server, "refresh-1", 400, &body).await;
    let (session, kept) = session(&server, "refresh-1");
    let nexus = nexus(&server, &session);
    let error = nexus.account().await.unwrap_err();
    assert!(matches!(error, SourceError::Unauthorized { .. }), "{error:?}");
    assert!(!nexus.connected());
    assert_eq!(*kept.lock().unwrap(), [None]);
}

#[tokio::test]
async fn a_passing_trouble_keeps_the_login() {
    let mut server = mockito::Server::new_async().await;
    let down = renewal(&mut server, "refresh-1", 503, "").await;
    let (session, kept) = session(&server, "refresh-1");
    let nexus = nexus(&server, &session);
    let error = nexus.account().await.unwrap_err();
    assert!(matches!(error, SourceError::Network { .. }), "{error:?}");
    assert!(nexus.connected());
    assert!(kept.lock().unwrap().is_empty());

    down.remove_async().await;
    let body = reply(&jwt("Reverss", &[]), "refresh-2", 21600);
    renewal(&mut server, "refresh-1", 200, &body).await;
    assert_eq!(nexus.account().await.unwrap().name, "Reverss");
}

#[tokio::test]
async fn a_refused_access_token_is_renewed_next_time() {
    let mut server = mockito::Server::new_async().await;
    let first = jwt("Reverss", &[]);
    let body = reply(&first, "refresh-2", 21600);
    renewal(&mut server, "refresh-1", 200, &body).await;
    server
        .mock("GET", Matcher::Regex("download_link".into()))
        .with_status(401)
        .create_async()
        .await;
    let (session, _) = session(&server, "refresh-1");
    let nexus = nexus(&server, &session);
    let error = nexus.download_url("skyrim", 1, 2, None).await.unwrap_err();
    assert!(matches!(error, SourceError::Unauthorized { .. }), "{error:?}");

    let body = reply(&jwt("Renewed", &[]), "refresh-3", 21600);
    let again = renewal(&mut server, "refresh-2", 200, &body).await;
    assert_eq!(nexus.account().await.unwrap().name, "Renewed");
    again.assert_async().await;
}

#[tokio::test]
async fn logging_out_revokes_the_login_even_offline() {
    let mut server = mockito::Server::new_async().await;
    let revoke = server
        .mock("POST", "/revoke")
        .match_body(Matcher::AllOf(vec![
            field("client_id", "unalia"),
            field("token", "refresh-1"),
        ]))
        .with_body("{}")
        .create_async()
        .await;
    let (online, kept) = session(&server, "refresh-1");
    online.log_out().await.unwrap();
    revoke.assert_async().await;
    assert_eq!(*kept.lock().unwrap(), [None]);
    // A download that was waiting doesn't bring the login back.
    let error = online.access_token().await.unwrap_err();
    assert!(matches!(error, SourceError::Unauthorized { .. }), "{error:?}");

    revoke.remove_async().await;
    let (offline, kept) = session(&server, "refresh-1");
    offline.log_out().await.unwrap_err();
    assert_eq!(*kept.lock().unwrap(), [None]);
}

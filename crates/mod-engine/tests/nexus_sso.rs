//! Connexion SSO de Nexus, contre un faux serveur WebSocket qui suit le
//! protocole documenté par Nexus.

use futures_util::{SinkExt, StreamExt};
use mod_engine::sources::nexus_sso::{self, SsoError};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

/// Serveur d'une seule session : vérifie l'annonce, confirme la session,
/// puis envoie `final_reply` une fois la page « ouverte ».
async fn fake_sso(final_reply: Value) -> (String, tokio::task::JoinHandle<Value>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
        let hello: Value = match socket.next().await.unwrap().unwrap() {
            Message::Text(text) => serde_json::from_str(&text).unwrap(),
            other => panic!("{other:?}"),
        };
        socket
            .send(Message::text(
                json!({ "success": true, "data": { "connection_token": "abc" }, "error": null })
                    .to_string(),
            ))
            .await
            .unwrap();
        socket
            .send(Message::text(final_reply.to_string()))
            .await
            .unwrap();
        hello
    });
    (url, server)
}

#[tokio::test]
async fn the_api_key_comes_back_once_the_user_authorises() {
    let (url, server) =
        fake_sso(json!({ "success": true, "data": { "api_key": "secret-key" }, "error": null }))
            .await;
    let mut opened = None;
    let key = nexus_sso::login(&url, "modalia", |page| opened = Some(page))
        .await
        .unwrap();
    assert_eq!(key, "secret-key");

    let hello = server.await.unwrap();
    let id = hello["id"].as_str().unwrap();
    assert_eq!(hello["protocol"], 2);
    assert!(hello["token"].is_null());
    assert_eq!(id.len(), 36, "uuid");
    assert_eq!(
        opened.unwrap(),
        format!("https://www.nexusmods.com/sso?id={id}&application=modalia")
    );
}

#[tokio::test]
async fn a_refusal_is_reported() {
    let (url, _server) =
        fake_sso(json!({ "success": false, "data": null, "error": "Unknown application" })).await;
    let err = nexus_sso::login(&url, "modalia", |_| {}).await.unwrap_err();
    assert!(
        matches!(err, SsoError::Refused(ref m) if m == "Unknown application"),
        "{err:?}"
    );
}

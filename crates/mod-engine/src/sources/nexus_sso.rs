//! Connexion « Log in with Nexus » (SSO), celle des gestionnaires de mods
//! approuvés par Nexus : pas de clé à copier.
//!
//! 1. Modalia ouvre un WebSocket vers `sso.nexusmods.com` et s'annonce avec
//!    un identifiant de session tiré au hasard.
//! 2. Le navigateur ouvre la page d'autorisation de Nexus pour cette
//!    session ; l'utilisateur clique « Authorise ».
//! 3. Nexus renvoie la clé API par le WebSocket.
//!
//! Nexus réserve ce mode aux applications qu'il a approuvées (identifiant
//! d'application). Sans approbation, la clé API personnelle reste la seule
//! voie autorisée.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::json;
use tokio_tungstenite::tungstenite::Message;

pub const SSO_URL: &str = "wss://sso.nexusmods.com";
pub const AUTHORIZE_URL: &str = "https://www.nexusmods.com/sso";

/// Le temps de se connecter à Nexus dans le navigateur et de valider.
const WAIT_FOR_USER: Duration = Duration::from_secs(5 * 60);
/// Le serveur ferme une connexion muette : on le relance régulièrement.
const PING_EVERY: Duration = Duration::from_secs(30);

#[derive(Debug, thiserror::Error)]
pub enum SsoError {
    #[error("couldn't reach Nexus Mods")]
    Network(#[source] Box<tokio_tungstenite::tungstenite::Error>),
    #[error("Nexus Mods refused the login: {0}")]
    Refused(String),
    #[error("the login wasn't confirmed in time")]
    Timeout,
    #[error("Nexus Mods closed the login before it was confirmed")]
    Closed,
}

impl From<tokio_tungstenite::tungstenite::Error> for SsoError {
    fn from(error: tokio_tungstenite::tungstenite::Error) -> Self {
        Self::Network(Box::new(error))
    }
}

#[derive(Deserialize)]
struct Reply {
    #[serde(default)]
    success: bool,
    #[serde(default)]
    data: Option<ReplyData>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Deserialize)]
struct ReplyData {
    connection_token: Option<String>,
    api_key: Option<String>,
}

/// Page d'autorisation de la session `id` pour l'application `application`.
pub fn authorize_url(id: &str, application: &str) -> String {
    format!("{AUTHORIZE_URL}?id={id}&application={application}")
}

/// Mène la connexion jusqu'au bout et renvoie la clé API. `open` reçoit
/// l'adresse de la page d'autorisation, à ouvrir dans le navigateur.
pub async fn login(
    sso_url: &str,
    application: &str,
    open: impl FnOnce(String),
) -> Result<String, SsoError> {
    tokio::time::timeout(WAIT_FOR_USER, run(sso_url, application, open))
        .await
        .map_err(|_| SsoError::Timeout)?
}

async fn run(
    sso_url: &str,
    application: &str,
    open: impl FnOnce(String),
) -> Result<String, SsoError> {
    let id = uuid::Uuid::new_v4().to_string();
    let (mut socket, _) = tokio_tungstenite::connect_async(sso_url).await?;
    socket
        .send(Message::text(
            json!({ "id": id, "token": null, "protocol": 2 }).to_string(),
        ))
        .await?;

    let mut open = Some(open);
    let mut ping = tokio::time::interval(PING_EVERY);
    ping.tick().await;
    loop {
        tokio::select! {
            _ = ping.tick() => socket.send(Message::Ping(Vec::new().into())).await?,
            message = socket.next() => {
                let Some(message) = message else {
                    return Err(SsoError::Closed);
                };
                let text = match message? {
                    Message::Text(text) => text,
                    Message::Close(_) => return Err(SsoError::Closed),
                    _ => continue,
                };
                let Ok(reply) = serde_json::from_str::<Reply>(&text) else {
                    continue;
                };
                if !reply.success {
                    return Err(SsoError::Refused(reply.error.unwrap_or_default()));
                }
                let Some(data) = reply.data else { continue };
                if let Some(key) = data.api_key {
                    let _ = socket.close(None).await;
                    return Ok(key);
                }
                // Session enregistrée : l'utilisateur peut valider.
                if data.connection_token.is_some()
                    && let Some(open) = open.take()
                {
                    open(authorize_url(&id, application));
                }
            }
        }
    }
}

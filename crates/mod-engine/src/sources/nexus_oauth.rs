//! "Log in with Nexus" (OAuth 2.0 with PKCE), the login Nexus asks of
//! public apps: <https://modding.wiki/en/api/oauth2-guide>.
//!
//! 1. Modalia listens on a free port of `127.0.0.1`.
//! 2. The browser opens Nexus's authorization page; the user clicks
//!    "Authorise".
//! 3. Nexus sends the browser back to that port with a code, exchanged for
//!    an access token (a few hours) and a refresh token.
//!
//! No secret in the app: PKCE ties the code to the login that asked for it,
//! `state` ties the answer to it. No scope is asked: the access token alone
//! says who the user is.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use reqwest::{Client, StatusCode, Url};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio::task::JoinSet;

use super::SourceError;
use super::nexus::{APPLICATION_NAME, Account};

pub const OAUTH_URL: &str = "https://users.nexusmods.com/oauth";
const NAME: &str = "Nexus Mods";

/// Port of the address Nexus sends the browser back to. 0: any free one, as
/// RFC 8252 allows for loopback addresses.
const CALLBACK_PORT: u16 = 0;
const CALLBACK_PATH: &str = "/callback";

/// Time to log in to Nexus in the browser and confirm.
const WAIT_FOR_USER: Duration = Duration::from_secs(5 * 60);
/// Time for the browser to send its request, once connected.
const WAIT_FOR_REQUEST: Duration = Duration::from_secs(10);
/// A callback is a short request: no need to read more.
const MAX_REQUEST: usize = 16 * 1024;
/// Seconds before its end when an access token is already renewed.
const RENEW_EARLY: i64 = 60;

#[derive(Debug, thiserror::Error)]
pub enum OAuthError {
    #[error("couldn't wait for the answer of Nexus Mods on this computer")]
    Listen(#[source] std::io::Error),
    #[error("you declined the login on Nexus Mods")]
    Declined,
    #[error("Nexus Mods refused the login: {0}")]
    Refused(String),
    #[error("the login wasn't confirmed in time")]
    Timeout,
    #[error(transparent)]
    Source(#[from] SourceError),
}

/// Tokens of a logged-in user. `Debug` hides them: they never reach a log.
#[derive(Clone, PartialEq, Eq)]
pub struct Tokens {
    access: String,
    refresh: String,
    /// Seconds since the Unix epoch.
    expires_at: i64,
}

impl std::fmt::Debug for Tokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tokens")
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

impl Tokens {
    /// Login kept from an earlier run: the first request gets an access
    /// token.
    pub fn from_refresh(refresh: String) -> Self {
        Self {
            access: String::new(),
            refresh,
            expires_at: 0,
        }
    }

    /// The only part worth keeping on disk.
    pub fn refresh_token(&self) -> &str {
        &self.refresh
    }

    fn expired(&self) -> bool {
        now() + RENEW_EARLY >= self.expires_at
    }
}

#[derive(Deserialize)]
struct TokenReply {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: i64,
}

#[derive(Deserialize, Default)]
struct ErrorReply {
    #[serde(default)]
    error: String,
    error_description: Option<String>,
}

/// Content of the access token (JWT) that we read.
#[derive(Deserialize)]
struct Claims {
    user: User,
}

#[derive(Deserialize)]
struct User {
    username: String,
    #[serde(default)]
    membership_roles: Vec<String>,
}

/// The account an access token belongs to. The signature isn't checked: the
/// token comes straight from Nexus over HTTPS, and only Nexus grants
/// anything from it.
fn account_of(access: &str) -> Result<Account, SourceError> {
    let claims = access
        .split('.')
        .nth(1)
        .and_then(|part| URL_SAFE_NO_PAD.decode(part.trim_end_matches('=')).ok())
        .and_then(|json| serde_json::from_slice::<Claims>(&json).ok())
        .ok_or_else(|| unexpected("unreadable access token"))?;
    Ok(Account {
        premium: claims
            .user
            .membership_roles
            .iter()
            .any(|role| matches!(role.as_str(), "premium" | "lifetimepremium")),
        name: claims.user.username,
        avatar_url: None,
    })
}

/// Modalia as an OAuth client of Nexus.
#[derive(Debug, Clone)]
pub struct OAuth {
    client: Client,
    url: String,
    client_id: String,
    app_version: String,
}

impl OAuth {
    /// `client_id`: given by Nexus to the apps it registered.
    pub fn new(client: Client, client_id: &str, app_version: &str) -> Self {
        Self::with_url(client, OAUTH_URL, client_id, app_version)
    }

    /// For tests: fake server.
    pub fn with_url(client: Client, url: &str, client_id: &str, app_version: &str) -> Self {
        Self {
            client,
            url: url.trim_end_matches('/').to_owned(),
            client_id: client_id.to_owned(),
            app_version: app_version.to_owned(),
        }
    }

    /// Runs the login to the end. `open` receives the URL of the
    /// authorization page, to open in the browser.
    pub async fn login(&self, open: impl FnOnce(String)) -> Result<Tokens, OAuthError> {
        let listener = TcpListener::bind(("127.0.0.1", CALLBACK_PORT))
            .await
            .map_err(OAuthError::Listen)?;
        let port = listener.local_addr().map_err(OAuthError::Listen)?.port();
        let redirect = format!("http://127.0.0.1:{port}{CALLBACK_PATH}");
        let state = random();
        let verifier = format!("{}{}", random(), random());
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let page = Url::parse_with_params(
            &format!("{}/authorize", self.url),
            [
                ("client_id", self.client_id.as_str()),
                ("response_type", "code"),
                ("scope", ""),
                ("redirect_uri", &redirect),
                ("state", &state),
                ("code_challenge_method", "S256"),
                ("code_challenge", &challenge),
            ],
        )
        .expect("valid URL");
        open(page.into());

        let code = tokio::time::timeout(WAIT_FOR_USER, wait_for_code(&listener, &state))
            .await
            .map_err(|_| OAuthError::Timeout)??;
        // The answer is in: the port closes.
        drop(listener);
        self.token(
            &[
                ("grant_type", "authorization_code"),
                ("redirect_uri", &redirect),
                ("client_id", &self.client_id),
                ("code", &code),
                ("code_verifier", &verifier),
            ],
            None,
        )
        .await
    }

    async fn renew(&self, tokens: &Tokens) -> Result<Tokens, OAuthError> {
        self.token(
            &[
                ("grant_type", "refresh_token"),
                ("client_id", &self.client_id),
                ("refresh_token", &tokens.refresh),
            ],
            Some(&tokens.refresh),
        )
        .await
    }

    /// Logs the app out on Nexus's side too.
    pub async fn revoke(&self, tokens: &Tokens) -> Result<(), SourceError> {
        self.post(
            "revoke",
            &[
                ("client_id", &self.client_id),
                ("token", &tokens.refresh),
                ("token_type_hint", "refresh_token"),
            ],
        )
        .await?
        .error_for_status()
        .map_err(network)?;
        Ok(())
    }

    /// `previous`: refresh token to keep if Nexus doesn't send a new one.
    async fn token(
        &self,
        form: &[(&str, &str)],
        previous: Option<&str>,
    ) -> Result<Tokens, OAuthError> {
        let response = self.post("token", form).await?;
        // The answers OAuth defines for a refusal. Anything else (429, 503…)
        // is a passing trouble.
        if matches!(
            response.status(),
            StatusCode::BAD_REQUEST | StatusCode::UNAUTHORIZED
        ) {
            let reply: ErrorReply = response.json().await.unwrap_or_default();
            return Err(OAuthError::Refused(
                reply.error_description.unwrap_or(reply.error),
            ));
        }
        let body = response
            .error_for_status()
            .map_err(network)?
            .text()
            .await
            .map_err(network)?;
        // Without serde's message: it could quote a token.
        let reply: TokenReply =
            serde_json::from_str(&body).map_err(|_| unexpected("unreadable token"))?;
        let refresh = reply
            .refresh_token
            .or_else(|| previous.map(str::to_owned))
            .ok_or_else(|| unexpected("no refresh token"))?;
        Ok(Tokens {
            access: reply.access_token,
            refresh,
            expires_at: now() + reply.expires_in,
        })
    }

    async fn post(
        &self,
        path: &str,
        form: &[(&str, &str)],
    ) -> Result<reqwest::Response, SourceError> {
        self.client
            .post(format!("{}/{path}", self.url))
            .header("Application-Name", APPLICATION_NAME)
            .header("Application-Version", &self.app_version)
            .form(form)
            .send()
            .await
            .map_err(network)
    }
}

/// Called with the new tokens, to keep them. `None`: Nexus ended the login.
type Keep = Box<dyn Fn(Option<&Tokens>) + Send + Sync>;

/// A logged-in user: always gives a valid access token.
pub struct Session {
    oauth: OAuth,
    /// The lock spans a renewal: a refresh token only works once. `None`:
    /// logged out, nothing must be kept anymore.
    tokens: Mutex<Option<Tokens>>,
    keep: Keep,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session").finish_non_exhaustive()
    }
}

impl Session {
    pub fn new(
        oauth: OAuth,
        tokens: Tokens,
        keep: impl Fn(Option<&Tokens>) + Send + Sync + 'static,
    ) -> Arc<Self> {
        Arc::new(Self {
            oauth,
            tokens: Mutex::new(Some(tokens)),
            keep: Box::new(keep),
        })
    }

    /// `Unauthorized`: Nexus ended the login (revoked on its site, too old).
    pub async fn access_token(self: &Arc<Self>) -> Result<String, SourceError> {
        let session = Arc::clone(self);
        // A task of its own: a canceled install must not lose the answer,
        // the previous refresh token no longer works.
        tokio::spawn(async move { session.renewed().await })
            .await
            .unwrap_or_else(|_| Err(unexpected("login renewal interrupted")))
    }

    async fn renewed(&self) -> Result<String, SourceError> {
        let mut slot = self.tokens.lock().await;
        let tokens = slot
            .as_ref()
            .ok_or(SourceError::Unauthorized { source_name: NAME })?;
        if !tokens.expired() {
            return Ok(tokens.access.clone());
        }
        match self.oauth.renew(tokens).await {
            Ok(new) => {
                (self.keep)(Some(&new));
                let access = new.access.clone();
                *slot = Some(new);
                Ok(access)
            }
            Err(OAuthError::Refused(_)) => {
                (self.keep)(None);
                *slot = None;
                Err(SourceError::Unauthorized { source_name: NAME })
            }
            Err(OAuthError::Source(error)) => Err(error),
            Err(error) => Err(unexpected(&error.to_string())),
        }
    }

    pub async fn account(self: &Arc<Self>) -> Result<Account, SourceError> {
        account_of(&self.access_token().await?)
    }

    /// Nexus refused the access token before its end: the next request asks
    /// for a new one, and finds out if the login was revoked.
    pub async fn expire(&self) {
        if let Some(tokens) = &mut *self.tokens.lock().await {
            tokens.expires_at = 0;
        }
    }

    /// Ends the login here, then on Nexus's side: if Nexus can't be reached,
    /// this computer is logged out all the same.
    pub async fn log_out(&self) -> Result<(), SourceError> {
        let tokens = {
            let mut slot = self.tokens.lock().await;
            (self.keep)(None);
            slot.take()
        };
        match tokens {
            Some(tokens) => self.oauth.revoke(&tokens).await,
            None => Ok(()),
        }
    }
}

/// Waits for the browser to come back with the answer of this login. The
/// rest (a request for an icon, a connection opened in advance, an answer
/// without our `state`) doesn't end the wait.
async fn wait_for_code(listener: &TcpListener, state: &str) -> Result<String, OAuthError> {
    let mut requests: JoinSet<Option<Result<String, OAuthError>>> = JoinSet::new();
    loop {
        tokio::select! {
            connection = listener.accept() => {
                let (stream, _) = connection.map_err(OAuthError::Listen)?;
                let state = state.to_owned();
                requests.spawn(async move { answer(stream, &state).await });
            }
            Some(done) = requests.join_next() => {
                if let Ok(Some(result)) = done {
                    return result;
                }
            }
        }
    }
}

/// Reads a request and answers the browser. `None`: not the one we wait for.
async fn answer(mut stream: TcpStream, state: &str) -> Option<Result<String, OAuthError>> {
    let target = tokio::time::timeout(WAIT_FOR_REQUEST, read_target(&mut stream))
        .await
        .ok()
        .flatten()?;
    let result = parse_callback(&target, state);
    let (status, text) = match &result {
        Some(Ok(_)) => ("200 OK", "You can close this page and go back to Unalia."),
        Some(Err(_)) => ("200 OK", "Login canceled. You can close this page."),
        None => ("404 Not Found", "Nothing here."),
    };
    let page = format!(
        "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><title>Unalia Mod Manager</title>\
         <body style=\"font-family:system-ui;text-align:center;margin-top:20vh\"><p>{text}</p></body></html>"
    );
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n\r\n{page}",
        page.len()
    );
    // The page only informs: the login goes on without it.
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
    result
}

/// Path and query of a `GET` request.
async fn read_target(stream: &mut TcpStream) -> Option<String> {
    let mut request = Vec::new();
    let mut buffer = [0; 1024];
    while !request.windows(4).any(|w| w == b"\r\n\r\n") {
        let read = stream.read(&mut buffer).await.ok()?;
        if read == 0 || request.len() + read > MAX_REQUEST {
            return None;
        }
        request.extend_from_slice(&buffer[..read]);
    }
    let request = String::from_utf8_lossy(&request);
    let target = request.strip_prefix("GET ")?.split_whitespace().next()?;
    Some(target.to_owned())
}

/// The code, or why there is none. `None`: not the answer to this login.
fn parse_callback(target: &str, state: &str) -> Option<Result<String, OAuthError>> {
    let url = Url::parse(&format!("http://127.0.0.1{target}")).ok()?;
    if url.path() != CALLBACK_PATH {
        return None;
    }
    let param = |name: &str| {
        url.query_pairs()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.into_owned())
    };
    if param("state").as_deref() != Some(state) {
        return None;
    }
    if let Some(code) = param("code").filter(|code| !code.is_empty()) {
        return Some(Ok(code));
    }
    let error = param("error")?;
    Some(Err(if error == "access_denied" {
        OAuthError::Declined
    } else {
        OAuthError::Refused(param("error_description").unwrap_or(error))
    }))
}

/// 122 random bits from the system, in 32 characters.
fn random() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

fn network(error: reqwest::Error) -> SourceError {
    SourceError::Network {
        source_name: NAME,
        error,
    }
}

fn unexpected(detail: &str) -> SourceError {
    SourceError::Unexpected {
        source_name: NAME,
        detail: detail.to_owned(),
    }
}

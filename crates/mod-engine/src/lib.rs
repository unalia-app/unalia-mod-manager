//! Moteur de Modalia, indépendant de l'interface.
//!
//! Voir `SPEC.md` (section 6.3) pour le rôle de chaque brique.

pub mod artwork;
pub mod detect;
pub mod games;
pub mod sources;

pub use reqwest::Client as HttpClient;

/// Client HTTP partagé par les briques qui parlent aux sources. Il
/// s'identifie, comme le demandent les API publiques.
pub fn http_client() -> HttpClient {
    reqwest::Client::builder()
        .user_agent(concat!(
            "Modalia/",
            env!("CARGO_PKG_VERSION"),
            " (+https://gitlab.com/reverss/modalia)"
        ))
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("client HTTP")
}

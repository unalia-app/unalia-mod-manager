//! Moteur de Modalia, indépendant de l'interface.
//!
//! Voir `SPEC.md` (section 6.3) pour le rôle de chaque brique.

pub mod advice;
pub mod archive;
pub mod artwork;
pub mod deploy;
pub mod detect;
pub mod download;
pub mod foreign;
pub mod games;
pub mod installer;
pub mod launch;
pub mod loaders;
pub mod resolver;
pub mod sources;
pub mod state;
pub mod store;

pub use reqwest::Client as HttpClient;

/// Client HTTP partagé par les briques qui parlent aux sources. Il
/// s'identifie, comme le demandent les API publiques.
pub fn http_client() -> HttpClient {
    reqwest::Client::builder()
        .user_agent(concat!(
            "UnaliaModManager/",
            env!("CARGO_PKG_VERSION"),
            " (+https://gitlab.com/reverss/modalia)"
        ))
        // Pas de délai total : un gros mod met des minutes à arriver. On
        // coupe seulement une connexion qui ne répond plus.
        .connect_timeout(std::time::Duration::from_secs(15))
        .read_timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("client HTTP")
}

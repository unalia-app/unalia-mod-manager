//! Modalia's engine, independent of the UI.
//!
//! See `ARCHITECTURE.md` for the role of each building block.

pub mod advice;
pub mod archive;
pub mod artwork;
pub mod deploy;
pub mod detect;
pub mod download;
pub mod foreign;
pub mod games;
pub mod ini;
pub mod installer;
pub mod launch;
pub mod loaders;
pub mod resolver;
pub mod sources;
pub mod state;
pub mod store;

pub use reqwest::Client as HttpClient;

/// HTTP client shared by the blocks that talk to the sources. It identifies
/// itself, as the public APIs ask.
pub fn http_client() -> HttpClient {
    reqwest::Client::builder()
        .user_agent(concat!(
            "UnaliaModManager/",
            env!("CARGO_PKG_VERSION"),
            " (+https://github.com/unalia-app/unalia-mod-manager)"
        ))
        // No total timeout: a big mod takes minutes to arrive. We only
        // drop a connection that stopped answering.
        .connect_timeout(std::time::Duration::from_secs(15))
        .read_timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("client HTTP")
}

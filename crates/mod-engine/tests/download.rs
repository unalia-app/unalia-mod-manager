//! Téléchargements de gros fichiers contre un vrai serveur HTTP local qui
//! gère les requêtes de plage et sait couper la connexion en plein envoi.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::thread;

use mod_engine::download::{self, DownloadError, Options};

/// Comportement du faux serveur.
#[derive(Clone, Copy)]
struct Behavior {
    /// Accepte les requêtes de plage.
    ranges: bool,
    /// Les requêtes n° `from..to` (la sonde est la n° 0) sont coupées après
    /// `after` octets.
    drop_requests: (usize, usize),
    drop_after: u64,
}

impl Default for Behavior {
    fn default() -> Self {
        Self {
            ranges: true,
            drop_requests: (0, 0),
            drop_after: 0,
        }
    }
}

struct Server {
    url: String,
    requests: Arc<AtomicUsize>,
    /// Octets de contenu réellement envoyés.
    served: Arc<AtomicU64>,
}

fn serve(data: Arc<Vec<u8>>, behavior: Behavior) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/mod.zip", listener.local_addr().unwrap());
    let requests = Arc::new(AtomicUsize::new(0));
    let served = Arc::new(AtomicU64::new(0));
    let (r, s) = (requests.clone(), served.clone());
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let (data, r, s) = (data.clone(), r.clone(), s.clone());
            thread::spawn(move || handle(stream, &data, behavior, &r, &s));
        }
    });
    Server {
        url,
        requests,
        served,
    }
}

fn handle(
    mut stream: TcpStream,
    data: &[u8],
    behavior: Behavior,
    requests: &AtomicUsize,
    served: &AtomicU64,
) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut range = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
            let (start, end) = value.split_once('-').unwrap();
            let start: u64 = start.parse().unwrap();
            let end: u64 = if end.is_empty() {
                data.len() as u64 - 1
            } else {
                end.parse().unwrap()
            };
            range = Some((start, end.min(data.len() as u64 - 1)));
        }
    }
    let index = requests.fetch_add(1, Ordering::SeqCst);
    let total = data.len() as u64;
    let (status, start, end) = match range {
        Some((start, end)) if behavior.ranges => ("206 Partial Content", start, end),
        _ => ("200 OK", 0, total - 1),
    };
    let body = &data[start as usize..=end as usize];
    let mut head = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    if status.starts_with("206") {
        head.push_str(&format!(
            "Content-Range: bytes {start}-{end}/{total}\r\nAccept-Ranges: bytes\r\n"
        ));
    }
    head.push_str("\r\n");
    let _ = stream.write_all(head.as_bytes());

    let (from, to) = behavior.drop_requests;
    let limit = if (from..to).contains(&index) {
        (behavior.drop_after as usize).min(body.len())
    } else {
        body.len()
    };
    if stream.write_all(&body[..limit]).is_ok() {
        served.fetch_add(limit as u64, Ordering::SeqCst);
    }
    let _ = stream.flush();
    // Coupure : on ferme sans avoir tout envoyé.
}

fn data(size: usize) -> Arc<Vec<u8>> {
    // Contenu non trivial pour qu'un morceau mal placé se voie.
    Arc::new((0..size).map(|i| (i * 31 % 251) as u8).collect())
}

fn options(connections: usize) -> Options {
    Options {
        connections,
        parallel_from: 64 * 1024,
        retries: 3,
        ..Options::default()
    }
}

fn sha256(bytes: &[u8]) -> String {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x");
    std::fs::write(&path, bytes).unwrap();
    download::sha256_file(&path).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn large_files_download_in_parallel_parts() {
    let content = data(3 * 1024 * 1024 + 17);
    let server = serve(content.clone(), Behavior::default());
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("mod.zip");
    let mut last = 0;

    let done = download::download(
        &reqwest::Client::new(),
        &server.url,
        &dest,
        &options(4),
        |received, total| {
            assert!(received >= last, "la progression ne recule jamais");
            assert_eq!(total, Some(content.len() as u64));
            last = received;
        },
    )
    .await
    .unwrap();

    assert_eq!(std::fs::read(&dest).unwrap(), *content);
    assert_eq!(done.sha256, sha256(&content));
    assert_eq!(last, content.len() as u64);
    assert_eq!(
        server.requests.load(Ordering::SeqCst),
        5,
        "une sonde puis 4 morceaux"
    );
    assert!(!dest.with_extension("part").exists());
    assert!(!dest.with_extension("part.json").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn dropped_connections_are_retried_where_they_stopped() {
    let content = data(2 * 1024 * 1024);
    // Les 4 premiers morceaux sont coupés à 100 Ko : chacun doit reprendre.
    let server = serve(
        content.clone(),
        Behavior {
            drop_requests: (1, 5),
            drop_after: 100 * 1024,
            ..Behavior::default()
        },
    );
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("mod.zip");

    download::download(
        &reqwest::Client::new(),
        &server.url,
        &dest,
        &options(4),
        |_, _| {},
    )
    .await
    .unwrap();

    assert_eq!(std::fs::read(&dest).unwrap(), *content);
    // Rien n'a été retéléchargé deux fois (à la sonde près).
    assert!(server.served.load(Ordering::SeqCst) <= content.len() as u64 + 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn servers_without_ranges_get_a_single_stream() {
    let content = data(512 * 1024);
    let server = serve(
        content.clone(),
        Behavior {
            ranges: false,
            ..Behavior::default()
        },
    );
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("mod.zip");

    download::download(
        &reqwest::Client::new(),
        &server.url,
        &dest,
        &options(4),
        |_, _| {},
    )
    .await
    .unwrap();

    assert_eq!(std::fs::read(&dest).unwrap(), *content);
    assert_eq!(server.requests.load(Ordering::SeqCst), 1, "la sonde suffit");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_interrupted_download_resumes_on_the_next_try() {
    let content = data(2 * 1024 * 1024);
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("mod.zip");

    // Premier essai : toutes les connexions coupent, sans nouvelle tentative
    // (Modalia fermé, réseau perdu…).
    let flaky = serve(
        content.clone(),
        Behavior {
            drop_requests: (1, usize::MAX),
            drop_after: 300 * 1024,
            ..Behavior::default()
        },
    );
    let interrupted = Options {
        retries: 0,
        ..options(4)
    };
    let err = download::download(
        &reqwest::Client::new(),
        &flaky.url,
        &dest,
        &interrupted,
        |_, _| {},
    )
    .await
    .unwrap_err();
    assert!(
        matches!(err, DownloadError::Network(_) | DownloadError::Protocol(_)),
        "{err:?}"
    );
    assert!(
        dest.with_extension("part").exists(),
        "la partie reçue est gardée"
    );
    assert!(dest.with_extension("part.json").exists());

    // Second essai : seul ce qui manquait est demandé.
    let healthy = serve(content.clone(), Behavior::default());
    download::download(
        &reqwest::Client::new(),
        &healthy.url,
        &dest,
        &options(4),
        |_, _| {},
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), *content);
    let resent = healthy.served.load(Ordering::SeqCst);
    assert!(
        resent < content.len() as u64 - 1024 * 1024,
        "{resent} octets renvoyés : la reprise doit éviter le déjà-reçu"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_wrong_fingerprint_leaves_nothing_behind() {
    let content = data(256 * 1024);
    let server = serve(content.clone(), Behavior::default());
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("mod.zip");
    let options = Options {
        expected_sha256: Some("00".repeat(32)),
        ..options(4)
    };

    let err = download::download(
        &reqwest::Client::new(),
        &server.url,
        &dest,
        &options,
        |_, _| {},
    )
    .await
    .unwrap_err();
    assert!(matches!(err, DownloadError::HashMismatch { .. }), "{err:?}");
    assert!(!dest.exists());
    assert!(!dest.with_extension("part").exists());
    assert!(!dest.with_extension("part.json").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn oversized_files_are_refused_before_downloading() {
    let content = data(256 * 1024);
    let server = serve(content.clone(), Behavior::default());
    let dir = tempfile::tempdir().unwrap();
    let options = Options {
        max_bytes: 1024,
        ..options(4)
    };

    let err = download::download(
        &reqwest::Client::new(),
        &server.url,
        &dir.path().join("mod.zip"),
        &options,
        |_, _| {},
    )
    .await
    .unwrap_err();
    assert!(matches!(err, DownloadError::TooLarge), "{err:?}");
    assert_eq!(
        server.served.load(Ordering::SeqCst),
        1,
        "seule la sonde a été envoyée"
    );
}

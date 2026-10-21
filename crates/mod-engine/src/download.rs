//! Téléchargement d'une archive de mod, rapide et robuste aux gros fichiers.
//!
//! - **Plusieurs connexions** : si le serveur accepte les requêtes de plage
//!   (`Range`), un gros fichier est découpé en morceaux téléchargés en
//!   parallèle, comme le font les gestionnaires de téléchargement. Sinon,
//!   une seule connexion.
//! - **Reprise** : l'avancement de chaque morceau est noté à côté du fichier
//!   (`.part.json`). Une coupure réseau est retentée automatiquement, et un
//!   téléchargement interrompu (Modalia fermé) reprend où il en était.
//! - **Annulable** : un drapeau partagé (`Cancel`) arrête tout au prochain
//!   bloc reçu. Ce qui est déjà arrivé est gardé pour reprendre plus tard.
//! - **Espace disque** vérifié avant de commencer.
//! - **Empreinte SHA-256** toujours calculée et conservée, vérifiée si la
//!   source en publie une (SPEC §7). Le fichier n'apparaît sous son nom final
//!   qu'une fois complet et vérifié.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::StatusCode;
use reqwest::header::{CONTENT_RANGE, RANGE};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::HttpClient;

/// Drapeau d'annulation partagé entre l'interface et le travail en cours.
#[derive(Debug, Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

fn check(cancel: Option<&Cancel>) -> Result<(), DownloadError> {
    match cancel {
        Some(c) if c.is_cancelled() => Err(DownloadError::Cancelled),
        _ => Ok(()),
    }
}

/// Aucune archive de mod légitime n'approche cette taille.
pub const MAX_DOWNLOAD_BYTES: u64 = 128 * 1024 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct Options {
    pub expected_sha256: Option<String>,
    pub max_bytes: u64,
    /// Connexions simultanées au plus (1 : jamais de parallèle).
    pub connections: usize,
    /// En dessous, une seule connexion suffit.
    pub parallel_from: u64,
    /// Espace libre exigé, en multiple de la taille de l'archive (l'archive,
    /// son extraction, une marge).
    pub space_factor: f64,
    /// Nouvelles tentatives par morceau après une coupure.
    pub retries: u32,
    /// Annulation demandée par l'utilisateur.
    pub cancel: Option<Cancel>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            expected_sha256: None,
            max_bytes: MAX_DOWNLOAD_BYTES,
            connections: 6,
            parallel_from: 32 * 1024 * 1024,
            space_factor: 2.2,
            retries: 5,
            cancel: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Downloaded {
    pub path: PathBuf,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("the download failed")]
    Network(#[from] reqwest::Error),
    #[error("the file is too large to be a mod")]
    TooLarge,
    #[error("not enough disk space: {needed} needed, {available} free")]
    NoSpace { needed: String, available: String },
    #[error("the downloaded file doesn't match its fingerprint")]
    HashMismatch { expected: String, actual: String },
    #[error("the server sent an unexpected response")]
    Protocol(String),
    #[error("couldn't save the download")]
    Io(#[from] io::Error),
    #[error("the download was cancelled")]
    Cancelled,
}

/// Télécharge `url` vers `dest`. `on_progress(reçu, total)` est appelé au
/// fil de l'eau (`total` : taille annoncée par le serveur, si connue).
pub async fn download(
    client: &HttpClient,
    url: &str,
    dest: &Path,
    options: &Options,
    mut on_progress: impl FnMut(u64, Option<u64>),
) -> Result<Downloaded, DownloadError> {
    check(options.cancel.as_ref())?;
    if let Some(dir) = dest.parent() {
        fs::create_dir_all(dir)?;
    }
    let partial = dest.with_extension("part");
    let journal = dest.with_extension("part.json");

    // Sonde : le premier octet suffit à savoir si le serveur découpe, et la
    // taille totale. Les redirections (CDN) sont suivies une fois ici.
    let probe = client
        .get(url)
        .header(RANGE, "bytes=0-0")
        .send()
        .await?
        .error_for_status()?;
    let ranged = probe.status() == StatusCode::PARTIAL_CONTENT;
    let total = if ranged {
        total_from_content_range(&probe)
    } else {
        probe.content_length()
    };
    let final_url = probe.url().to_string();

    if total.is_some_and(|t| t > options.max_bytes) {
        return Err(DownloadError::TooLarge);
    }
    if let Some(total) = total {
        ensure_space(dest, (total as f64 * options.space_factor) as u64)?;
    }

    let outcome = match total {
        Some(total) if ranged && options.connections > 1 && total >= options.parallel_from => {
            drop(probe);
            parallel(
                client,
                &final_url,
                &partial,
                &journal,
                total,
                options,
                &mut on_progress,
            )
            .await
        }
        Some(total) if ranged => {
            drop(probe);
            resumable(
                client,
                &final_url,
                &partial,
                total,
                options,
                &mut on_progress,
            )
            .await
        }
        // Pas de découpage possible : la réponse de la sonde est déjà le
        // fichier entier, on la lit d'une traite.
        _ => {
            let _ = fs::remove_file(&partial);
            single(probe, &partial, total, options, &mut on_progress).await
        }
    };
    let size = match outcome {
        Ok(size) => size,
        // Coupure ou annulation : on garde `.part` et son journal pour
        // reprendre.
        Err(error @ (DownloadError::Network(_) | DownloadError::Cancelled)) => return Err(error),
        Err(error) => {
            let _ = fs::remove_file(&partial);
            let _ = fs::remove_file(&journal);
            return Err(error);
        }
    };

    let sha256 = {
        let path = partial.clone();
        tokio::task::spawn_blocking(move || sha256_file(&path))
            .await
            .map_err(|e| DownloadError::Io(io::Error::other(e)))??
    };
    if let Some(expected) = &options.expected_sha256
        && !expected.eq_ignore_ascii_case(&sha256)
    {
        let _ = fs::remove_file(&partial);
        let _ = fs::remove_file(&journal);
        return Err(DownloadError::HashMismatch {
            expected: expected.clone(),
            actual: sha256,
        });
    }
    fs::rename(&partial, dest)?;
    let _ = fs::remove_file(&journal);
    Ok(Downloaded {
        path: dest.to_path_buf(),
        sha256,
        size,
    })
}

/// Réponse sans découpage : un seul flux, du début à la fin.
async fn single(
    mut response: reqwest::Response,
    partial: &Path,
    total: Option<u64>,
    options: &Options,
    on_progress: &mut impl FnMut(u64, Option<u64>),
) -> Result<u64, DownloadError> {
    let mut file = File::create(partial)?;
    let mut received = 0u64;
    while let Some(chunk) = response.chunk().await? {
        check(options.cancel.as_ref())?;
        received += chunk.len() as u64;
        // On ne croit pas Content-Length : on compte ce qui arrive.
        if received > options.max_bytes {
            return Err(DownloadError::TooLarge);
        }
        file.write_all(&chunk)?;
        on_progress(received, total);
    }
    file.sync_all()?;
    Ok(received)
}

/// Une connexion, mais qui reprend là où elle s'est arrêtée.
async fn resumable(
    client: &HttpClient,
    url: &str,
    partial: &Path,
    total: u64,
    options: &Options,
    on_progress: &mut impl FnMut(u64, Option<u64>),
) -> Result<u64, DownloadError> {
    let mut attempts = 0;
    loop {
        let start = fs::metadata(partial)
            .map(|m| m.len())
            .unwrap_or(0)
            .min(total);
        if start == total {
            return Ok(total);
        }
        let result = async {
            let mut response = client
                .get(url)
                .header(RANGE, format!("bytes={start}-"))
                .send()
                .await?
                .error_for_status()?;
            let mut file = OpenOptions::new().create(true).append(true).open(partial)?;
            // Le serveur ignore la plage et renvoie tout : on repart de zéro.
            if response.status() != StatusCode::PARTIAL_CONTENT {
                file.set_len(0)?;
            }
            let mut received = fs::metadata(partial)?.len();
            while let Some(chunk) = response.chunk().await? {
                check(options.cancel.as_ref())?;
                received += chunk.len() as u64;
                if received > options.max_bytes {
                    return Err(DownloadError::TooLarge);
                }
                file.write_all(&chunk)?;
                on_progress(received, Some(total));
            }
            file.sync_all()?;
            Ok(received)
        }
        .await;
        match result {
            Ok(received) if received >= total => return Ok(received),
            Ok(_) | Err(DownloadError::Network(_)) if attempts < options.retries => {
                attempts += 1;
                tokio::time::sleep(backoff(attempts)).await;
            }
            Ok(received) => {
                return Err(DownloadError::Protocol(format!(
                    "the server stopped at {received} of {total} bytes"
                )));
            }
            Err(error) => return Err(error),
        }
    }
}

/// Journal de reprise d'un téléchargement en morceaux.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Journal {
    total: u64,
    segments: Vec<Segment>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Segment {
    /// Prochain octet à écrire.
    next: u64,
    /// Dernier octet du morceau (inclus).
    end: u64,
}

impl Journal {
    fn new(total: u64, connections: usize) -> Self {
        let size = total.div_ceil(connections as u64).max(1);
        let segments = (0..connections as u64)
            .map(|i| i * size)
            .take_while(|&start| start < total)
            .map(|start| Segment {
                next: start,
                end: (start + size).min(total) - 1,
            })
            .collect();
        Self { total, segments }
    }

    fn received(&self) -> u64 {
        let remaining: u64 = self
            .segments
            .iter()
            .map(|s| (s.end + 1).saturating_sub(s.next))
            .sum();
        self.total - remaining
    }

    fn load(path: &Path, total: u64) -> Option<Self> {
        let journal: Self = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
        (journal.total == total).then_some(journal)
    }

    fn save(&self, path: &Path) -> io::Result<()> {
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, serde_json::to_vec(self).map_err(io::Error::other)?)?;
        fs::rename(tmp, path)
    }
}

/// Plusieurs connexions, chacune sur son morceau, écrivant directement à sa
/// place dans le fichier.
async fn parallel(
    client: &HttpClient,
    url: &str,
    partial: &Path,
    journal_path: &Path,
    total: u64,
    options: &Options,
    on_progress: &mut impl FnMut(u64, Option<u64>),
) -> Result<u64, DownloadError> {
    let resumed = partial
        .is_file()
        .then(|| Journal::load(journal_path, total))
        .flatten();
    let journal = match resumed {
        Some(journal) => journal,
        None => {
            // Fichier réservé à sa taille finale : chaque morceau écrit chez lui.
            File::create(partial)?.set_len(total)?;
            Journal::new(total, options.connections)
        }
    };
    journal.save(journal_path)?;
    let mut received = journal.received();
    on_progress(received, Some(total));

    let journal = Arc::new(Mutex::new(journal));
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<u64>();
    let mut tasks = tokio::task::JoinSet::new();
    let count = journal.lock().unwrap().segments.len();
    for index in 0..count {
        let (client, url, partial, journal, tx) = (
            client.clone(),
            url.to_owned(),
            partial.to_path_buf(),
            journal.clone(),
            tx.clone(),
        );
        let (retries, cancel) = (options.retries, options.cancel.clone());
        tasks.spawn(async move {
            fetch_segment(
                &client,
                &url,
                &partial,
                &journal,
                index,
                (retries, cancel.as_ref()),
                &tx,
            )
            .await
        });
    }
    drop(tx);

    // Progression et journal tenus à jour au fil des morceaux reçus.
    let mut since_save = 0u64;
    while let Some(bytes) = rx.recv().await {
        received += bytes;
        since_save += bytes;
        on_progress(received, Some(total));
        if since_save >= 8 * 1024 * 1024 {
            since_save = 0;
            let _ = journal.lock().unwrap().clone().save(journal_path);
        }
    }

    let mut failure = None;
    while let Some(result) = tasks.join_next().await {
        match result {
            Ok(Ok(())) => {}
            Ok(Err(error)) => failure = failure.or(Some(error)),
            Err(error) => failure = failure.or(Some(DownloadError::Io(io::Error::other(error)))),
        }
    }
    let _ = journal.lock().unwrap().clone().save(journal_path);
    match failure {
        Some(error) => Err(error),
        None => Ok(total),
    }
}

async fn fetch_segment(
    client: &HttpClient,
    url: &str,
    partial: &Path,
    journal: &Mutex<Journal>,
    index: usize,
    (retries, cancel): (u32, Option<&Cancel>),
    progress: &tokio::sync::mpsc::UnboundedSender<u64>,
) -> Result<(), DownloadError> {
    let mut attempts = 0;
    loop {
        let Segment { next, end } = journal.lock().unwrap().segments[index].clone();
        if next > end {
            return Ok(());
        }
        let result = async {
            let mut response = client
                .get(url)
                .header(RANGE, format!("bytes={next}-{end}"))
                .send()
                .await?
                .error_for_status()?;
            if response.status() != StatusCode::PARTIAL_CONTENT {
                return Err(DownloadError::Protocol(
                    "the server stopped honoring ranges".into(),
                ));
            }
            let mut file = OpenOptions::new().write(true).open(partial)?;
            file.seek(SeekFrom::Start(next))?;
            let mut position = next;
            while let Some(chunk) = response.chunk().await? {
                check(cancel)?;
                // Un serveur qui en envoie plus que demandé n'écrase pas le
                // morceau voisin.
                let room = (end + 1 - position) as usize;
                let chunk = &chunk[..chunk.len().min(room)];
                file.write_all(chunk)?;
                position += chunk.len() as u64;
                journal.lock().unwrap().segments[index].next = position;
                let _ = progress.send(chunk.len() as u64);
                if position > end {
                    break;
                }
            }
            file.flush()?;
            Ok(position > end)
        }
        .await;
        match result {
            Ok(true) => return Ok(()),
            Ok(false) | Err(DownloadError::Network(_)) if attempts < retries => {
                attempts += 1;
                tokio::time::sleep(backoff(attempts)).await;
            }
            Ok(false) => {
                return Err(DownloadError::Protocol(
                    "a part of the file never arrived".into(),
                ));
            }
            Err(error) => return Err(error),
        }
    }
}

fn backoff(attempt: u32) -> Duration {
    Duration::from_millis(250 * 2u64.pow(attempt.min(5)))
}

/// `Content-Range: bytes 0-0/12345` → 12345.
fn total_from_content_range(response: &reqwest::Response) -> Option<u64> {
    response
        .headers()
        .get(CONTENT_RANGE)?
        .to_str()
        .ok()?
        .rsplit('/')
        .next()?
        .trim()
        .parse()
        .ok()
}

fn ensure_space(dest: &Path, needed: u64) -> Result<(), DownloadError> {
    let dir = dest.parent().unwrap_or(dest);
    // Impossible à mesurer (système exotique) : on tente quand même.
    let Ok(available) = fs4::available_space(dir) else {
        return Ok(());
    };
    if available < needed {
        return Err(DownloadError::NoSpace {
            needed: human_size(needed),
            available: human_size(available),
        });
    }
    Ok(())
}

/// 10_737_418_240 → « 10.0 GB ».
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Empreinte d'un fichier déjà présent (archive en cache).
pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        match file.read(&mut buffer)? {
            0 => break,
            n => hasher.update(&buffer[..n]),
        }
    }
    Ok(hex(&hasher.finalize()))
}

/// Empreinte MD5 d'un fichier : celle que Nexus sait retrouver
/// (`md5_search`). Jamais utilisée pour la sécurité, seulement pour
/// reconnaître un fichier identique publié ailleurs.
pub fn md5_file(path: &Path) -> io::Result<String> {
    use md5::{Digest as _, Md5};
    let mut file = File::open(path)?;
    let mut hasher = Md5::new();
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        match file.read(&mut buffer)? {
            0 => break,
            n => hasher.update(&buffer[..n]),
        }
    }
    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_cover_the_whole_file_without_overlap() {
        for (total, n) in [(100, 6), (5, 6), (1, 3), (1024 * 1024 * 1024 + 7, 6)] {
            let journal = Journal::new(total, n);
            assert_eq!(journal.segments.first().unwrap().next, 0);
            assert_eq!(journal.segments.last().unwrap().end, total - 1);
            for pair in journal.segments.windows(2) {
                assert_eq!(pair[0].end + 1, pair[1].next, "{total} / {n}");
            }
            assert_eq!(journal.received(), 0);
        }
    }

    #[test]
    fn sizes_read_like_a_human_would() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(10 * 1024 * 1024 * 1024), "10.0 GB");
        assert_eq!(human_size(1536), "1.5 KB");
    }
}

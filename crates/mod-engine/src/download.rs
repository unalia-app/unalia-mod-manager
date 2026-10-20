//! Téléchargement d'une archive de mod, avec son empreinte SHA-256.
//!
//! L'empreinte est vérifiée si la source en publie une (SPEC §7) et toujours
//! conservée : elle permettra de repérer un fichier modifié hors de Modalia.
//! Le fichier n'apparaît sous son nom final qu'une fois complet et vérifié.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::HttpClient;

/// Aucune archive de mod légitime n'approche cette taille.
pub const MAX_DOWNLOAD_BYTES: u64 = 2 * 1024 * 1024 * 1024;

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
    #[error("the downloaded file doesn't match its fingerprint")]
    HashMismatch { expected: String, actual: String },
    #[error("couldn't save the download")]
    Io(#[from] io::Error),
}

/// Télécharge `url` vers `dest`. `on_progress(reçu, total)` est appelé à
/// chaque morceau (`total` : taille annoncée par le serveur, si connue).
pub async fn download(
    client: &HttpClient,
    url: &str,
    dest: &Path,
    expected_sha256: Option<&str>,
    max_bytes: u64,
    mut on_progress: impl FnMut(u64, Option<u64>),
) -> Result<Downloaded, DownloadError> {
    let mut response = client.get(url).send().await?.error_for_status()?;
    let total = response.content_length();
    if total.is_some_and(|t| t > max_bytes) {
        return Err(DownloadError::TooLarge);
    }

    if let Some(dir) = dest.parent() {
        fs::create_dir_all(dir)?;
    }
    let partial = dest.with_extension("part");
    let result = async {
        let mut file = File::create(&partial)?;
        let mut hasher = Sha256::new();
        let mut received = 0u64;
        while let Some(chunk) = response.chunk().await? {
            received += chunk.len() as u64;
            // On ne croit pas Content-Length : on compte ce qui arrive.
            if received > max_bytes {
                return Err(DownloadError::TooLarge);
            }
            hasher.update(&chunk);
            file.write_all(&chunk)?;
            on_progress(received, total);
        }
        file.sync_all()?;
        Ok((hex(&hasher.finalize()), received))
    }
    .await;

    let (sha256, size) = match result {
        Ok(done) => done,
        Err(error) => {
            let _ = fs::remove_file(&partial);
            return Err(error);
        }
    };
    if let Some(expected) = expected_sha256
        && !expected.eq_ignore_ascii_case(&sha256)
    {
        let _ = fs::remove_file(&partial);
        return Err(DownloadError::HashMismatch {
            expected: expected.to_owned(),
            actual: sha256,
        });
    }
    fs::rename(&partial, dest)?;
    Ok(Downloaded {
        path: dest.to_path_buf(),
        sha256,
        size,
    })
}

/// Empreinte d'un fichier déjà présent (archive en cache).
pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 64 * 1024];
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

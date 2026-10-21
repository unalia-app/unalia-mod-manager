//! Extraction sûre des archives de mods (SPEC §7). Formats : `.zip` et
//! `.7z`, reconnus à leur signature et non à leur extension.
//!
//! Une archive téléchargée est une donnée hostile jusqu'à preuve du
//! contraire. On refuse :
//! - les chemins qui sortent du dossier cible (« zip slip ») : `..`, chemins
//!   absolus, lettres de lecteur, y compris écrits avec des antislashs ;
//! - les liens symboliques ;
//! - les archives bombes : taille totale, nombre d'entrées et ratio de
//!   compression sont mesurés pendant l'extraction, jamais crus sur parole ;
//! - toute archive qui échoue en cours de route : rien n'est laissé à moitié
//!   extrait.
//!
//! Rien n'est jamais exécuté : on écrit des fichiers, c'est tout.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Taille décompressée totale maximale.
    pub max_total_bytes: u64,
    pub max_entries: usize,
    /// Ratio décompressé / compressé maximal par entrée.
    pub max_ratio: u64,
}

impl Default for Limits {
    fn default() -> Self {
        // Assez pour les plus gros mods (textures 4K, gros packs), le ratio
        // de compression reste la vraie défense contre les archives bombes.
        Self {
            max_total_bytes: 256 * 1024 * 1024 * 1024,
            max_entries: 500_000,
            max_ratio: 200,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ArchiveError {
    #[error("the archive contains an unsafe path: {0}")]
    UnsafePath(String),
    #[error("the archive contains a link: {0}")]
    Link(String),
    #[error("the archive is too large once extracted")]
    TooLarge,
    #[error("the archive contains too many files")]
    TooManyEntries,
    #[error("the archive is suspiciously compressed: {0}")]
    SuspiciousRatio(String),
    #[error("the archive is damaged")]
    Corrupt(#[from] zip::result::ZipError),
    #[error("the archive is damaged")]
    SevenZip(#[from] sevenz_rust2::Error),
    #[error("{0} archives aren't supported yet")]
    Unsupported(&'static str),
    #[error("couldn't extract the archive")]
    Io(#[from] io::Error),
}

/// Extrait `archive` dans `dest`, qui ne doit pas exister. Renvoie les
/// fichiers extraits, en chemins relatifs avec des `/`.
///
/// L'extraction se fait dans un dossier temporaire voisin, renommé en `dest`
/// seulement si tout a réussi.
pub fn extract(archive: &Path, dest: &Path, limits: Limits) -> Result<Vec<String>, ArchiveError> {
    extract_with_progress(archive, dest, limits, |_, _| {})
}

/// Comme `extract`, avec `on_progress(octets extraits, total annoncé)` :
/// le total vient de l'archive (pour l'affichage seulement, les limites se
/// fondent sur ce qui est réellement écrit).
pub fn extract_with_progress(
    archive: &Path,
    dest: &Path,
    limits: Limits,
    mut on_progress: impl FnMut(u64, u64),
) -> Result<Vec<String>, ArchiveError> {
    let staging = dest.with_extension("partial");
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(&staging)?;

    match extract_into(archive, &staging, limits, &mut on_progress) {
        Ok(files) => {
            fs::rename(&staging, dest)?;
            Ok(files)
        }
        Err(error) => {
            let _ = fs::remove_dir_all(&staging);
            Err(error)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Zip,
    SevenZip,
}

/// Format d'après les premiers octets du fichier.
fn format(archive: &Path) -> Result<Format, ArchiveError> {
    let mut magic = [0u8; 6];
    let read = File::open(archive)?.read(&mut magic)?;
    match &magic[..read] {
        [b'P', b'K', ..] => Ok(Format::Zip),
        [b'7', b'z', 0xBC, 0xAF, 0x27, 0x1C] => Ok(Format::SevenZip),
        [b'R', b'a', b'r', b'!', ..] => Err(ArchiveError::Unsupported("RAR")),
        // Inconnu : on laisse le lecteur zip le refuser comme abîmé.
        _ => Ok(Format::Zip),
    }
}

fn extract_into(
    archive: &Path,
    dir: &Path,
    limits: Limits,
    on_progress: &mut impl FnMut(u64, u64),
) -> Result<Vec<String>, ArchiveError> {
    match format(archive)? {
        Format::Zip => extract_zip(archive, dir, limits, on_progress),
        Format::SevenZip => extract_7z(archive, dir, limits, on_progress),
    }
}

fn extract_zip(
    archive: &Path,
    dir: &Path,
    limits: Limits,
    on_progress: &mut impl FnMut(u64, u64),
) -> Result<Vec<String>, ArchiveError> {
    let mut zip = zip::ZipArchive::new(File::open(archive)?)?;
    if zip.len() > limits.max_entries {
        return Err(ArchiveError::TooManyEntries);
    }
    let announced: u64 = (0..zip.len())
        .filter_map(|i| zip.by_index_raw(i).ok().map(|e| e.size()))
        .sum();

    let mut files = Vec::new();
    let mut total: u64 = 0;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        // Un nom illisible (pas UTF-8) : archive considérée comme abîmée.
        let raw = entry.name()?.into_owned();
        if entry.is_symlink() {
            return Err(ArchiveError::Link(raw));
        }
        let Some(relative) = safe_relative_path(&raw) else {
            return Err(ArchiveError::UnsafePath(raw));
        };
        let target = dir.join(&relative);

        if entry.is_dir() {
            fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }

        // On compte ce qui sort vraiment du décompresseur, borné juste
        // au-dessus de ce qui reste autorisé.
        let remaining = limits.max_total_bytes.saturating_sub(total);
        let mut out = File::create(&target)?;
        let written = io::copy(
            &mut (&mut entry).take(remaining.saturating_add(1)),
            &mut out,
        )?;
        if written > remaining {
            return Err(ArchiveError::TooLarge);
        }
        let compressed = entry.compressed_size().max(1);
        // Les tout petits fichiers ont des ratios extrêmes sans danger.
        if written > 1024 * 1024 && written / compressed > limits.max_ratio {
            return Err(ArchiveError::SuspiciousRatio(raw));
        }
        total += written;
        on_progress(total, announced.max(total));
        files.push(to_slash(&relative));
    }
    files.sort();
    // Une archive peut contenir deux fois le même nom : le dernier gagne.
    files.dedup();
    Ok(files)
}

/// Attribut Windows des points d'analyse (liens symboliques, jonctions).
const REPARSE_POINT: u32 = 0x400;

/// Un `.7z` est souvent « solide » : les fichiers sont compressés d'un seul
/// bloc, sans taille compressée par fichier. Le ratio se mesure donc sur
/// l'archive entière.
fn extract_7z(
    archive: &Path,
    dir: &Path,
    limits: Limits,
    on_progress: &mut impl FnMut(u64, u64),
) -> Result<Vec<String>, ArchiveError> {
    let compressed = fs::metadata(archive)?.len().max(1);
    let mut reader = sevenz_rust2::ArchiveReader::open(archive, sevenz_rust2::Password::empty())?;
    let entries = &reader.archive().files;
    if entries.len() > limits.max_entries {
        return Err(ArchiveError::TooManyEntries);
    }
    let announced: u64 = entries.iter().map(|e| e.size()).sum();

    let mut files = Vec::new();
    let mut total: u64 = 0;
    // L'erreur de sécurité est gardée à part : le lecteur ne transporte que
    // les siennes.
    let mut refused: Option<ArchiveError> = None;
    let result = reader.for_each_entries(|entry, data| {
        let raw = entry.name().to_owned();
        if entry.is_anti_item() {
            return Ok(true);
        }
        if entry.has_windows_attributes && entry.windows_attributes() & REPARSE_POINT != 0 {
            refused = Some(ArchiveError::Link(raw));
            return Ok(false);
        }
        let Some(relative) = safe_relative_path(&raw) else {
            refused = Some(ArchiveError::UnsafePath(raw));
            return Ok(false);
        };
        let target = dir.join(&relative);
        if entry.is_directory() {
            fs::create_dir_all(&target)?;
            return Ok(true);
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let remaining = limits.max_total_bytes.saturating_sub(total);
        let mut out = File::create(&target)?;
        let written = io::copy(&mut data.take(remaining.saturating_add(1)), &mut out)?;
        if written > remaining {
            refused = Some(ArchiveError::TooLarge);
            return Ok(false);
        }
        total += written;
        if total > 1024 * 1024 && total / compressed > limits.max_ratio {
            refused = Some(ArchiveError::SuspiciousRatio(raw));
            return Ok(false);
        }
        on_progress(total, announced.max(total));
        files.push(to_slash(&relative));
        Ok(true)
    });
    if let Some(error) = refused {
        return Err(error);
    }
    result?;
    files.sort();
    files.dedup();
    Ok(files)
}

/// Chemin relatif sûr, ou `None`. Les antislashs comptent comme séparateurs
/// (archives créées sous Windows), et tout ce qui pourrait sortir du dossier
/// cible est refusé.
pub fn safe_relative_path(name: &str) -> Option<PathBuf> {
    let normalized = name.replace('\\', "/");
    if normalized.starts_with('/') || normalized.contains(':') || normalized.contains('\0') {
        return None;
    }
    let mut path = PathBuf::new();
    for part in normalized.split('/').filter(|p| !p.is_empty()) {
        match part {
            ".." => return None,
            "." => continue,
            _ => path.push(part),
        }
    }
    // Garde-fou : le chemin construit ne doit contenir que des noms simples.
    let only_normal = path.components().all(|c| matches!(c, Component::Normal(_)));
    (only_normal && !path.as_os_str().is_empty()).then_some(path)
}

fn to_slash(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_normal_paths_and_windows_separators() {
        assert_eq!(
            safe_relative_path("plugins/Mod.dll"),
            Some(PathBuf::from("plugins/Mod.dll"))
        );
        assert_eq!(
            safe_relative_path("plugins\\Mod.dll"),
            Some(PathBuf::from("plugins/Mod.dll"))
        );
        assert_eq!(safe_relative_path("./a//b/"), Some(PathBuf::from("a/b")));
    }

    #[test]
    fn rejects_paths_that_escape() {
        for bad in [
            "../evil.dll",
            "plugins/../../evil.dll",
            "..\\..\\evil.dll",
            "/etc/passwd",
            "\\Windows\\evil.dll",
            "C:/Windows/evil.dll",
            "C:evil.dll",
            "a/\0/b",
            "",
            ".",
            "./",
        ] {
            assert_eq!(safe_relative_path(bad), None, "{bad:?}");
        }
    }
}

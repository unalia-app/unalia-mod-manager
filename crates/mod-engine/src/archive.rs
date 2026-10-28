//! Safe extraction of mod archives. Formats: `.zip`, `.7z` and
//! `.rar`, recognized by their signature, not their extension.
//!
//! A downloaded archive is hostile data until proven otherwise. We refuse:
//! - paths that escape the target folder ("zip slip"): `..`, absolute
//!   paths, drive letters, including ones written with backslashes;
//! - symbolic links;
//! - archive bombs: total size, entry count and compression ratio are
//!   measured during extraction, never taken on trust;
//! - any archive that fails halfway: nothing is left half extracted.
//!
//! Nothing is ever executed: we write files, that's all.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use crate::download::Cancel;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Maximum total uncompressed size.
    pub max_total_bytes: u64,
    pub max_entries: usize,
    /// Maximum uncompressed / compressed ratio per entry.
    pub max_ratio: u64,
}

impl Default for Limits {
    fn default() -> Self {
        // Enough for the biggest mods (4K textures, big packs); the
        // compression ratio stays the real defense against archive bombs.
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
    #[error("the archive is damaged")]
    Rar(#[from] unrar::error::UnrarError),
    #[error("{0} archives aren't supported yet")]
    Unsupported(&'static str),
    #[error("couldn't extract the archive")]
    Io(#[from] io::Error),
    #[error("the extraction was cancelled")]
    Cancelled,
}

/// Extracts `archive` into `dest`, which must not exist. Returns the
/// extracted files, as relative paths with `/`.
///
/// Extraction happens in a temporary sibling folder, renamed to `dest` only
/// if everything succeeded.
pub fn extract(archive: &Path, dest: &Path, limits: Limits) -> Result<Vec<String>, ArchiveError> {
    extract_with_progress(archive, dest, limits, |_, _| {})
}

/// Like `extract`, with `on_progress(extracted bytes, announced total)`:
/// the total comes from the archive (for display only; the limits rely on
/// what is actually written).
pub fn extract_with_progress(
    archive: &Path,
    dest: &Path,
    limits: Limits,
    on_progress: impl FnMut(u64, u64),
) -> Result<Vec<String>, ArchiveError> {
    extract_cancellable(archive, dest, limits, None, on_progress)
}

/// Like `extract_with_progress`, stopped at the next file if `cancel` is
/// raised: nothing is left half extracted.
pub fn extract_cancellable(
    archive: &Path,
    dest: &Path,
    limits: Limits,
    cancel: Option<&Cancel>,
    mut on_progress: impl FnMut(u64, u64),
) -> Result<Vec<String>, ArchiveError> {
    let staging = dest.with_extension("partial");
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(&staging)?;

    match extract_into(archive, &staging, limits, cancel, &mut on_progress) {
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
    Rar,
}

/// Format from the first bytes of the file.
fn format(archive: &Path) -> Result<Format, ArchiveError> {
    let mut magic = [0u8; 6];
    let read = File::open(archive)?.read(&mut magic)?;
    match &magic[..read] {
        [b'P', b'K', ..] => Ok(Format::Zip),
        [b'7', b'z', 0xBC, 0xAF, 0x27, 0x1C] => Ok(Format::SevenZip),
        [b'R', b'a', b'r', b'!', ..] => Ok(Format::Rar),
        // Unknown: let the zip reader refuse it as damaged.
        _ => Ok(Format::Zip),
    }
}

fn extract_into(
    archive: &Path,
    dir: &Path,
    limits: Limits,
    cancel: Option<&Cancel>,
    on_progress: &mut impl FnMut(u64, u64),
) -> Result<Vec<String>, ArchiveError> {
    match format(archive)? {
        Format::Zip => extract_zip(archive, dir, limits, cancel, on_progress),
        Format::SevenZip => extract_7z(archive, dir, limits, cancel, on_progress),
        Format::Rar => extract_rar(archive, dir, limits, cancel, on_progress),
    }
}

fn cancelled(cancel: Option<&Cancel>) -> bool {
    cancel.is_some_and(Cancel::is_cancelled)
}

fn extract_zip(
    archive: &Path,
    dir: &Path,
    limits: Limits,
    cancel: Option<&Cancel>,
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
        if cancelled(cancel) {
            return Err(ArchiveError::Cancelled);
        }
        let mut entry = zip.by_index(i)?;
        // An unreadable name (not UTF-8): the archive counts as damaged.
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

        // We count what really comes out of the decompressor, capped just
        // above what is still allowed.
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
        // Tiny files have extreme ratios with no danger.
        if written > 1024 * 1024 && written / compressed > limits.max_ratio {
            return Err(ArchiveError::SuspiciousRatio(raw));
        }
        total += written;
        on_progress(total, announced.max(total));
        files.push(to_slash(&relative));
    }
    files.sort();
    // An archive can hold the same name twice: the last one wins.
    files.dedup();
    Ok(files)
}

/// Windows attribute of reparse points (symbolic links, junctions).
const REPARSE_POINT: u32 = 0x400;

/// A `.7z` is often "solid": files are compressed as a single block, with no
/// compressed size per file. The ratio is therefore measured on the whole
/// archive.
fn extract_7z(
    archive: &Path,
    dir: &Path,
    limits: Limits,
    cancel: Option<&Cancel>,
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
    // The security error is kept aside: the reader only carries its own
    // errors.
    let mut refused: Option<ArchiveError> = None;
    let result = reader.for_each_entries(|entry, data| {
        if cancelled(cancel) {
            refused = Some(ArchiveError::Cancelled);
            return Ok(false);
        }
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

/// `.rar` through the official UnRAR library. It writes each file itself,
/// at the path chosen here: the size is checked before (announced by the
/// header, which the decompressor doesn't exceed) and after (on disk).
/// Compressed sizes are not exposed, so the ratio is measured on the whole
/// archive, as for a `.7z`.
fn extract_rar(
    archive: &Path,
    dir: &Path,
    limits: Limits,
    cancel: Option<&Cancel>,
    on_progress: &mut impl FnMut(u64, u64),
) -> Result<Vec<String>, ArchiveError> {
    let compressed = fs::metadata(archive)?.len().max(1);
    let mut entries = 0;
    let mut announced: u64 = 0;
    let listing = unrar::Archive::new(archive).open_for_listing()?;
    if listing.volume_info() != unrar::VolumeInfo::None {
        return Err(ArchiveError::Unsupported("Multi-part RAR"));
    }
    for entry in listing {
        let entry = entry?;
        if entry.is_encrypted() {
            return Err(ArchiveError::Unsupported("Password-protected"));
        }
        entries += 1;
        if entries > limits.max_entries {
            return Err(ArchiveError::TooManyEntries);
        }
        announced = announced.saturating_add(entry.unpacked_size);
    }

    let mut files = Vec::new();
    let mut total: u64 = 0;
    let mut reader = unrar::Archive::new(archive).open_for_processing()?;
    while let Some(header) = reader.read_header()? {
        if cancelled(cancel) {
            return Err(ArchiveError::Cancelled);
        }
        let entry = header.entry();
        let raw = entry.filename.to_string_lossy().into_owned();
        if entry.file_attr & REPARSE_POINT != 0 {
            return Err(ArchiveError::Link(raw));
        }
        let Some(relative) = safe_relative_path(&raw) else {
            return Err(ArchiveError::UnsafePath(raw));
        };
        let target = dir.join(&relative);
        if entry.is_directory() {
            fs::create_dir_all(&target)?;
            reader = header.skip()?;
            continue;
        }
        let remaining = limits.max_total_bytes.saturating_sub(total);
        if entry.unpacked_size > remaining {
            return Err(ArchiveError::TooLarge);
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        reader = header.extract_to(&target)?;
        // A Unix link (or a junction) doesn't carry the Windows attribute:
        // what was written is what counts.
        let written = fs::symlink_metadata(&target)?;
        if written.file_type().is_symlink() {
            return Err(ArchiveError::Link(raw));
        }
        if written.len() > remaining {
            return Err(ArchiveError::TooLarge);
        }
        total += written.len();
        if total > 1024 * 1024 && total / compressed > limits.max_ratio {
            return Err(ArchiveError::SuspiciousRatio(raw));
        }
        on_progress(total, announced.max(total));
        files.push(to_slash(&relative));
    }
    files.sort();
    files.dedup();
    Ok(files)
}

/// Safe relative path, or `None`. Backslashes count as separators (archives
/// made on Windows), and anything that could escape the target folder is
/// refused.
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
    // Safety net: the built path must only contain plain names.
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

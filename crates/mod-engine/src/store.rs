//! **store** block: each mod file stored only once, under its hash, like
//! pnpm's store.
//!
//! - `files/ab/cdef…`: the content, named by its SHA-256. Two versions, two
//!   mods or two games that have the same file share it.
//! - `packages/<game>/<mod>/<version>.json`: an extracted version, the list
//!   "path in the package to hash". Keeping an old version only costs its
//!   own files, and going back to it downloads nothing again.
//!
//! A file only goes once no package uses it anymore (`gc`).

use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::download::sha256_file;

/// A package that nothing uses yet (prepared during a session, first file
/// of a Nexus multiple choice) is kept this long.
const GRACE: Duration = Duration::from_secs(24 * 60 * 60);

/// Storing a package and cleaning up never overlap: the cleanup could
/// remove a file that a package being written reuses.
static LOCK: Mutex<()> = Mutex::new(());

/// An extracted version of a mod.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Hash of the downloaded archive.
    pub sha256: String,
    /// Path in the package (`/` separator) to content.
    pub files: BTreeMap<String, Entry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub hash: String,
    pub size: u64,
}

/// What the cleanup freed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Freed {
    pub files: usize,
    pub bytes: u64,
}

#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Storage file for a content.
    pub fn blob(&self, hash: &str) -> PathBuf {
        let (head, rest) = hash.split_at(hash.len().min(2));
        self.root.join("files").join(head).join(rest)
    }

    fn manifest_path(&self, key: &str) -> PathBuf {
        self.root.join("packages").join(format!("{key}.json"))
    }

    /// Is the package `key` already stored? Saying so protects it from the
    /// cleanup for another grace period: it is about to be used.
    pub fn has(&self, key: &str) -> bool {
        let path = self.manifest_path(key);
        match File::options().append(true).open(&path) {
            Ok(file) => {
                let _ = file.set_modified(SystemTime::now());
                true
            }
            Err(_) => false,
        }
    }

    pub fn manifest(&self, key: &str) -> io::Result<Manifest> {
        let text = fs::read_to_string(self.manifest_path(key))?;
        serde_json::from_str(&text).map_err(io::Error::other)
    }

    pub fn save_manifest(&self, key: &str, manifest: &Manifest) -> io::Result<()> {
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        self.write_manifest(key, manifest)
    }

    fn write_manifest(&self, key: &str, manifest: &Manifest) -> io::Result<()> {
        let path = self.manifest_path(key);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.partial");
        fs::write(
            &tmp,
            serde_json::to_vec(manifest).map_err(io::Error::other)?,
        )?;
        fs::rename(&tmp, &path)
    }

    /// Empty work folder, on the same drive as the storage: what gets
    /// extracted there joins the storage without a copy.
    pub fn scratch(&self) -> io::Result<PathBuf> {
        let dir = self
            .root
            .join("tmp")
            .join(uuid::Uuid::new_v4().simple().to_string());
        fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// Stores the content of `dir` under `key`, then removes `dir`. A file
    /// already present is not kept twice.
    pub fn ingest(&self, key: &str, dir: &Path, sha256: &str) -> io::Result<Manifest> {
        let mut manifest = Manifest {
            sha256: sha256.to_owned(),
            files: BTreeMap::new(),
        };
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        for (path, file) in walk(dir)? {
            let size = fs::metadata(&file)?.len();
            let entry = self.take(&file, is_editable(&path, size))?;
            manifest.files.insert(path, entry);
        }
        self.write_manifest(key, &manifest)?;
        let _ = fs::remove_dir_all(dir);
        Ok(manifest)
    }

    /// Moves `file` into the storage. With `copy`, the stored content is a
    /// copy: `file` may be linked into a game (old storage), where a config
    /// rewritten in place would also change the stored content.
    fn take(&self, file: &Path, copy: bool) -> io::Result<Entry> {
        let size = fs::metadata(file)?.len();
        let hash = sha256_file(file)?;
        let blob = self.blob(&hash);
        // Same size: the same content is already there. Otherwise, the
        // stored copy was edited in place (before the config file rule): it
        // is replaced by the original.
        if !fs::metadata(&blob).is_ok_and(|m| m.len() == size) {
            if let Some(parent) = blob.parent() {
                fs::create_dir_all(parent)?;
            }
            if copy || fs::rename(file, &blob).is_err() {
                // Copy next to it then rename: a stored file linked into a
                // game is never rewritten in place.
                let tmp = blob.with_extension("partial");
                fs::copy(file, &tmp)?;
                fs::rename(&tmp, &blob)?;
            }
        }
        match fs::remove_file(file) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(Entry { hash, size }),
        }
    }

    /// Stores a copy of `file` (a config edited by the player), which stays
    /// where it is.
    pub fn add_copy(&self, file: &Path) -> io::Result<Entry> {
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = self.scratch()?;
        let copy = dir.join("file");
        fs::copy(file, &copy)?;
        let entry = self.take(&copy, false);
        let _ = fs::remove_dir_all(&dir);
        entry
    }

    /// Removes the packages of `prefix` (a game, or a mod of a game). Their
    /// files go at the next cleanup.
    pub fn remove_packages(&self, prefix: &str) -> io::Result<()> {
        match fs::remove_dir_all(self.root.join("packages").join(prefix)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    /// Space taken by the packages of `prefix`, each file counted once.
    pub fn size_of(&self, prefix: &str) -> u64 {
        let mut seen = HashSet::new();
        let mut size = 0;
        for (_, manifest) in self.manifests(&self.root.join("packages").join(prefix)) {
            for entry in manifest.files.into_values() {
                if seen.insert(entry.hash) {
                    size += entry.size;
                }
            }
        }
        size
    }

    /// Cleanup: removes the packages that nothing keeps (`packages`, keys
    /// recorded in the database) after the grace period, then the files
    /// that no remaining package nor `blobs` (edited configs) uses.
    pub fn gc(&self, packages: &HashSet<String>, blobs: &HashSet<String>) -> io::Result<Freed> {
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let root = self.root.join("packages");
        let now = SystemTime::now();
        let mut used: HashSet<String> = blobs.clone();
        for (path, manifest) in self.manifests(&root) {
            let key = key_of(&root, &path);
            let fresh = fs::metadata(&path)
                .and_then(|m| m.modified())
                .is_ok_and(|at| now.duration_since(at).unwrap_or_default() < GRACE);
            if key.as_ref().is_some_and(|k| packages.contains(k)) || fresh {
                used.extend(manifest.files.into_values().map(|e| e.hash));
            } else {
                fs::remove_file(&path)?;
                remove_empty_parents(&path, &root);
            }
        }
        let mut freed = Freed::default();
        let files = self.root.join("files");
        for (_, blob) in walk(&files).unwrap_or_default() {
            let hash = blob.strip_prefix(&files).ok().map(|p| {
                p.components()
                    .map(|c| c.as_os_str().to_string_lossy())
                    .collect::<String>()
            });
            if hash.is_some_and(|h| used.contains(&h)) {
                continue;
            }
            let size = fs::metadata(&blob).map_or(0, |m| m.len());
            if fs::remove_file(&blob).is_ok() {
                freed.files += 1;
                freed.bytes += size;
                remove_empty_parents(&blob, &files);
            }
        }
        // Leftovers of an interrupted extraction (app closed halfway).
        if let Ok(entries) = fs::read_dir(self.root.join("tmp")) {
            for entry in entries.flatten() {
                let old = entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .is_ok_and(|at| now.duration_since(at).unwrap_or_default() >= GRACE);
                if old {
                    let _ = fs::remove_dir_all(entry.path());
                }
            }
        }
        Ok(freed)
    }

    fn manifests(&self, dir: &Path) -> Vec<(PathBuf, Manifest)> {
        walk(dir)
            .unwrap_or_default()
            .into_iter()
            .filter(|(path, _)| path.ends_with(".json"))
            .filter_map(|(_, file)| {
                let manifest = serde_json::from_str(&fs::read_to_string(&file).ok()?).ok()?;
                Some((file, manifest))
            })
            .collect()
    }
}

/// Key of a package, from the path of its manifest.
fn key_of(root: &Path, manifest: &Path) -> Option<String> {
    let rel = manifest.strip_prefix(root).ok()?;
    let key = slash(rel);
    key.strip_suffix(".json").map(str::to_owned)
}

fn remove_empty_parents(path: &Path, root: &Path) {
    let mut dir = path.parent();
    while let Some(d) = dir {
        if d == root || fs::remove_dir(d).is_err() {
            break;
        }
        dir = d.parent();
    }
}

/// Files under `dir`, sorted: (relative path with `/`, full path).
fn walk(dir: &Path) -> io::Result<Vec<(String, PathBuf)>> {
    fn go(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) -> io::Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                go(root, &path, out)?;
            } else if let Ok(rel) = path.strip_prefix(root) {
                out.push((slash(rel), path));
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    match go(dir, dir, &mut out) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        other => other?,
    }
    out.sort();
    Ok(out)
}

fn slash(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// File that a mod or the game rewrites in place (BepInEx saves its `.cfg`
/// files this way): it is copied into the game, never linked, or the write
/// would change the stored content and every package that shares it.
pub fn is_editable(path: &str, size: u64) -> bool {
    const EXTENSIONS: &[&str] = &[
        "cfg",
        "conf",
        "config",
        "ini",
        "json",
        "jsonc",
        "properties",
        "toml",
        "txt",
        "xml",
        "yaml",
        "yml",
    ];
    size <= 4 * 1024 * 1024
        && Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| EXTENSIONS.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

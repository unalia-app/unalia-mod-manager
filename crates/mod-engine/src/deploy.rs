//! **deploy** block: put a mod's files into the game folder.
//!
//! Mods live in Modalia's storage; the game only sees hard links to them:
//! instant, no duplicate on disk, and no admin rights on Windows (unlike
//! symbolic links). If the storage and the game are on two different
//! drives, a hard link is impossible: we copy. Config files are always
//! copied (`store::is_editable`); what the player changes in them is kept
//! (`capture_edits`).
//!
//! Guarantees:
//! - no existing game file is ever overwritten or modified: a conflict stops
//!   everything before the first write;
//! - all or nothing: on failure, whatever was placed is removed;
//! - every placed file and every created folder is recorded, so that
//!   removal brings the folder back exactly to its original state.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::archive::safe_relative_path;
use crate::download::sha256_file;
use crate::loaders::Placement;
use crate::store::{Manifest, Store, is_editable};

/// Where a mod's files come from: its package in the storage, and the
/// configs the player edited (destination to stored content).
#[derive(Debug, Clone, Copy)]
pub struct Source<'a> {
    pub store: &'a Store,
    pub manifest: &'a Manifest,
    pub edits: &'a BTreeMap<String, String>,
}

impl Source<'_> {
    /// Content to place at `target`: the edited config, else the package
    /// file `source`.
    fn hash(&self, source: &str, target: &str) -> Option<(&str, u64)> {
        let entry = self.manifest.files.get(source)?;
        let hash = self.edits.get(target).unwrap_or(&entry.hash);
        Some((hash, entry.size))
    }
}

/// What a deployment placed in the game (paths relative to the game).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Deployment {
    pub files: Vec<String>,
    /// Folders that didn't exist before, from the highest to the deepest.
    pub created_dirs: Vec<String>,
    /// Files copied because they couldn't be linked (different drives).
    pub copied: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum DeployError {
    #[error("{0} already exists in the game folder")]
    Conflict(String),
    #[error("unsafe destination: {0}")]
    UnsafeTarget(String),
    #[error("{0} is missing from Unalia's storage")]
    MissingSource(String),
    #[error("couldn't write {path}")]
    Io {
        path: String,
        #[source]
        source: io::Error,
    },
}

/// Places the files of `from` into `game_dir` according to `placements`.
pub fn deploy(
    from: Source<'_>,
    game_dir: &Path,
    placements: &[Placement],
) -> Result<Deployment, DeployError> {
    // 1. Check everything before writing anything.
    let mut targets = HashSet::new();
    let mut plan = Vec::with_capacity(placements.len());
    for p in placements {
        let target = safe_relative_path(&p.target)
            .ok_or_else(|| DeployError::UnsafeTarget(p.target.clone()))?;
        let (hash, size) = from
            .hash(&p.source, &p.target)
            .ok_or_else(|| DeployError::MissingSource(p.source.clone()))?;
        let source = from.store.blob(hash);
        if !source.is_file() {
            return Err(DeployError::MissingSource(p.source.clone()));
        }
        let copy = is_editable(&p.target, size);
        // Two package files to the same destination: refuse rather than
        // lose one.
        if !targets.insert(target.clone()) || exists(&game_dir.join(&target)) {
            return Err(DeployError::Conflict(p.target.clone()));
        }
        plan.push((source, target, &p.target, copy));
    }

    // 2. Write, removing everything at the first failure.
    let mut done = Deployment::default();
    for (source, target, name, copy) in plan {
        if let Err(error) = place(game_dir, &source, &target, copy, &mut done) {
            undeploy(game_dir, &done, false);
            return Err(DeployError::Io {
                path: name.clone(),
                source: error,
            });
        }
        done.files.push(name.clone());
    }
    Ok(done)
}

fn place(
    game_dir: &Path,
    source: &Path,
    target: &Path,
    copy: bool,
    done: &mut Deployment,
) -> io::Result<()> {
    let mut dir = PathBuf::new();
    for part in target.parent().into_iter().flat_map(Path::components) {
        dir.push(part);
        let full = game_dir.join(&dir);
        if !full.exists() {
            fs::create_dir(&full)?;
            done.created_dirs.push(slash(&dir));
        }
    }
    let destination = game_dir.join(target);
    if copy {
        fs::copy(source, &destination)?;
    } else if fs::hard_link(source, &destination).is_err() {
        fs::copy(source, &destination)?;
        done.copied += 1;
    }
    Ok(())
}

/// The mod's configs that the player edited in the game, to keep for the
/// next time they are placed: call this before removing them. A config
/// back to the original is no longer an edit; a missing config keeps its
/// last known version.
pub fn capture_edits(
    from: Source<'_>,
    game_dir: &Path,
    placements: &[Placement],
) -> io::Result<BTreeMap<String, String>> {
    let mut edits = from.edits.clone();
    for p in placements {
        let Some(entry) = from.manifest.files.get(&p.source) else {
            continue;
        };
        if !is_editable(&p.target, entry.size) {
            continue;
        }
        let Some(target) = safe_relative_path(&p.target) else {
            continue;
        };
        let path = game_dir.join(target);
        let Ok(hash) = sha256_file(&path) else {
            continue;
        };
        if hash == entry.hash {
            edits.remove(&p.target);
        } else if edits.get(&p.target) != Some(&hash) {
            let kept = from.store.add_copy(&path)?;
            edits.insert(p.target.clone(), kept.hash);
        }
    }
    Ok(edits)
}

/// Removes what `deployment` placed. Created folders are removed if they
/// are empty; with `purge_dirs`, removed with their content (e.g. the logs
/// BepInEx writes into its own folders): they didn't exist before Modalia,
/// so nothing original can be in them.
///
/// Doesn't stop on an error: removing as much as possible beats a half-done
/// removal. Returns the paths that couldn't be removed.
pub fn undeploy(game_dir: &Path, deployment: &Deployment, purge_dirs: bool) -> Vec<String> {
    let mut leftovers = Vec::new();
    for file in &deployment.files {
        match fs::remove_file(game_dir.join(file)) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(_) => leftovers.push(file.clone()),
        }
    }
    for dir in deployment.created_dirs.iter().rev() {
        let full = game_dir.join(dir);
        let result = if purge_dirs {
            fs::remove_dir_all(&full)
        } else {
            fs::remove_dir(&full)
        };
        match result {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            // Folder shared with another mod still installed: it stays.
            Err(_) if !purge_dirs => {}
            Err(_) => leftovers.push(dir.clone()),
        }
    }
    leftovers
}

fn exists(path: &Path) -> bool {
    // symlink_metadata: a broken link also counts as taken.
    fs::symlink_metadata(path).is_ok()
}

fn slash(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

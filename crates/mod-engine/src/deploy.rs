//! Brique **deploy** : poser les fichiers d'un mod dans le dossier du jeu.
//!
//! Les mods vivent dans le stockage de Modalia ; le jeu n'en voit que des
//! liens physiques (hard links) : instantané, sans doublon sur le disque,
//! et sans droits administrateur sous Windows (contrairement aux liens
//! symboliques). Si le stockage et le jeu sont sur deux disques différents,
//! un lien physique est impossible : on copie. Les fichiers de config sont
//! toujours copiés (`store::is_editable`) ; ce que le joueur y change est
//! gardé (`capture_edits`).
//!
//! Garanties (SPEC §2.2) :
//! - aucun fichier existant du jeu n'est jamais écrasé ni modifié : un
//!   conflit arrête tout avant la première écriture ;
//! - tout réussit ou rien ne reste : en cas d'échec, ce qui a été posé est
//!   retiré ;
//! - chaque fichier posé et chaque dossier créé est enregistré, pour que le
//!   retrait rende le dossier exactement dans son état d'origine.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::archive::safe_relative_path;
use crate::download::sha256_file;
use crate::loaders::Placement;
use crate::store::{Manifest, Store, is_editable};

/// D'où viennent les fichiers d'un mod : son paquet dans le stockage, et
/// les configs que le joueur a modifiées (destination → contenu rangé).
#[derive(Debug, Clone, Copy)]
pub struct Source<'a> {
    pub store: &'a Store,
    pub manifest: &'a Manifest,
    pub edits: &'a BTreeMap<String, String>,
}

impl Source<'_> {
    /// Contenu à poser en `target` : la config modifiée, sinon le fichier
    /// `source` du paquet.
    fn hash(&self, source: &str, target: &str) -> Option<(&str, u64)> {
        let entry = self.manifest.files.get(source)?;
        let hash = self.edits.get(target).unwrap_or(&entry.hash);
        Some((hash, entry.size))
    }
}

/// Ce qu'un déploiement a posé dans le jeu (chemins relatifs au jeu).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Deployment {
    pub files: Vec<String>,
    /// Dossiers qui n'existaient pas avant, du plus haut au plus profond.
    pub created_dirs: Vec<String>,
    /// Fichiers copiés faute de pouvoir les lier (disques différents).
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

/// Pose les fichiers de `from` dans `game_dir` selon `placements`.
pub fn deploy(
    from: Source<'_>,
    game_dir: &Path,
    placements: &[Placement],
) -> Result<Deployment, DeployError> {
    // 1. Tout vérifier avant d'écrire quoi que ce soit.
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
        // Deux fichiers du paquet vers la même destination : on refuse
        // plutôt que d'en perdre un.
        if !targets.insert(target.clone()) || exists(&game_dir.join(&target)) {
            return Err(DeployError::Conflict(p.target.clone()));
        }
        plan.push((source, target, &p.target, copy));
    }

    // 2. Écrire, en retirant tout au premier échec.
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

/// Configs du mod que le joueur a modifiées dans le jeu, à garder pour la
/// prochaine fois qu'elles sont posées : à appeler avant de les retirer.
/// Une config revenue à l'original n'est plus une modification ; une
/// config absente garde sa dernière version connue.
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

/// Retire ce que `deployment` a posé. Les dossiers créés sont retirés s'ils
/// sont vides ; avec `purge_dirs`, retirés avec leur contenu (ex. les
/// journaux que BepInEx écrit dans ses propres dossiers) : ils n'existaient
/// pas avant Modalia, rien d'original ne peut s'y trouver.
///
/// Ne s'arrête pas sur une erreur : retirer le plus possible vaut mieux
/// qu'un retrait à moitié fait. Renvoie les chemins qui n'ont pas pu être
/// retirés.
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
            // Dossier partagé avec un autre mod encore installé : il reste.
            Err(_) if !purge_dirs => {}
            Err(_) => leftovers.push(dir.clone()),
        }
    }
    leftovers
}

fn exists(path: &Path) -> bool {
    // symlink_metadata : un lien cassé compte aussi comme occupé.
    fs::symlink_metadata(path).is_ok()
}

fn slash(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

//! Brique **store** : chaque fichier des mods rangé une seule fois, sous son
//! empreinte, comme le store de pnpm.
//!
//! - `files/ab/cdef…` : le contenu, nommé par son SHA-256. Deux versions,
//!   deux mods ou deux jeux qui ont le même fichier le partagent.
//! - `packages/<jeu>/<mod>/<version>.json` : une version extraite, la liste
//!   « chemin dans le paquet → empreinte ». Garder une ancienne version ne
//!   coûte que ses fichiers propres, et y revenir ne retélécharge rien.
//!
//! Un fichier ne part que quand plus aucun paquet ne s'en sert (`gc`).

use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::download::sha256_file;

/// Un paquet que rien n'utilise encore (préparé pendant une partie, premier
/// fichier d'un choix multiple Nexus) est gardé ce temps-là.
const GRACE: Duration = Duration::from_secs(24 * 60 * 60);

/// Ranger un paquet et faire le ménage ne se croisent pas : le ménage
/// pourrait retirer un fichier qu'un paquet en cours d'écriture réutilise.
static LOCK: Mutex<()> = Mutex::new(());

/// Une version extraite d'un mod.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Empreinte de l'archive téléchargée.
    pub sha256: String,
    /// Chemin dans le paquet (séparateur `/`) → contenu.
    pub files: BTreeMap<String, Entry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub hash: String,
    pub size: u64,
}

/// Ce que le ménage a libéré.
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

    /// Fichier du stockage pour un contenu.
    pub fn blob(&self, hash: &str) -> PathBuf {
        let (head, rest) = hash.split_at(hash.len().min(2));
        self.root.join("files").join(head).join(rest)
    }

    fn manifest_path(&self, key: &str) -> PathBuf {
        self.root.join("packages").join(format!("{key}.json"))
    }

    /// Le paquet `key` est-il déjà rangé ? Le dire le protège du ménage
    /// pour un nouveau délai : il va servir.
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

    /// Dossier de travail vide, sur le même disque que le stockage : ce
    /// qu'on y extrait rejoint le stockage sans copie.
    pub fn scratch(&self) -> io::Result<PathBuf> {
        let dir = self
            .root
            .join("tmp")
            .join(uuid::Uuid::new_v4().simple().to_string());
        fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// Range le contenu de `dir` sous `key`, puis retire `dir`. Un fichier
    /// déjà présent n'est pas gardé deux fois.
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

    /// Déplace `file` dans le stockage. Avec `copy`, le contenu rangé est
    /// une copie : `file` peut être lié dans un jeu (ancien stockage), où
    /// une config réécrite sur place changerait aussi le contenu rangé.
    fn take(&self, file: &Path, copy: bool) -> io::Result<Entry> {
        let size = fs::metadata(file)?.len();
        let hash = sha256_file(file)?;
        let blob = self.blob(&hash);
        // Même taille : le même contenu est déjà là. Sinon, la copie rangée
        // a été modifiée sur place (avant la règle des fichiers de config) :
        // elle est remplacée par l'original.
        if !fs::metadata(&blob).is_ok_and(|m| m.len() == size) {
            if let Some(parent) = blob.parent() {
                fs::create_dir_all(parent)?;
            }
            if copy || fs::rename(file, &blob).is_err() {
                // Copie à côté puis renommage : un fichier rangé lié dans un
                // jeu n'est jamais réécrit sur place.
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

    /// Range une copie de `file` (une config modifiée par le joueur), qui
    /// reste où elle est.
    pub fn add_copy(&self, file: &Path) -> io::Result<Entry> {
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = self.scratch()?;
        let copy = dir.join("file");
        fs::copy(file, &copy)?;
        let entry = self.take(&copy, false);
        let _ = fs::remove_dir_all(&dir);
        entry
    }

    /// Retire les paquets de `prefix` (un jeu, ou un mod d'un jeu). Leurs
    /// fichiers partent au prochain ménage.
    pub fn remove_packages(&self, prefix: &str) -> io::Result<()> {
        match fs::remove_dir_all(self.root.join("packages").join(prefix)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    /// Place qu'occupent les paquets de `prefix`, chaque fichier compté une
    /// fois.
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

    /// Ménage : retire les paquets que rien ne garde (`packages`, clés
    /// enregistrées dans la base) passé le délai de grâce, puis les fichiers
    /// qu'aucun paquet restant ni `blobs` (configs modifiées) n'utilise.
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
        // Restes d'une extraction interrompue (appli fermée en plein milieu).
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

/// Clé d'un paquet d'après le chemin de son manifeste.
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

/// Fichiers sous `dir`, triés : (chemin relatif en `/`, chemin complet).
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

/// Fichier qu'un mod ou le jeu réécrit sur place (BepInEx enregistre ses
/// `.cfg` ainsi) : il est copié dans le jeu, jamais lié, sinon l'écriture
/// changerait le contenu rangé et tous les paquets qui le partagent.
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

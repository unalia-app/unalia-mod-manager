//! Brique **state** : ce que Modalia a installé, dans une base SQLite.
//!
//! Pour chaque mod installé dans un jeu : sa version, son état actif ou non,
//! l'empreinte de l'archive, et la liste exacte des fichiers posés dans le
//! jeu (le déploiement), sans laquelle on ne saurait pas tout retirer.
//! Les versions remplacées par une mise à jour restent connues quelque
//! temps : y revenir ne retélécharge rien.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use crate::deploy::Deployment;
use crate::launch::PendingOp;
use crate::loaders::Placement;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledMod {
    /// Jeu, ex. `steam:1030300`.
    pub game: String,
    pub id: String,
    pub name: String,
    pub author: String,
    pub version: String,
    pub icon_url: Option<String>,
    /// Paquet loader : son dossier racine dans l'archive (ex. `BepInExPack`).
    pub loader_root: Option<String>,
    pub enabled: bool,
    /// Demandé par l'utilisateur (sinon : installé comme dépendance).
    pub explicit: bool,
    pub sha256: String,
    /// Secondes depuis l'époque Unix.
    pub installed_at: i64,
    /// Paquet du stockage (`store::Store`) qui contient ses fichiers.
    pub package: String,
    /// Dossier du mod dans l'ancien stockage, d'avant les paquets : vide
    /// une fois migré.
    #[serde(skip)]
    pub legacy_store_dir: String,
    /// Fichiers Nexus installés, dans l'ordre où ils sont posés (le
    /// principal puis ses variantes) : repris par les mises à jour.
    pub files: Vec<u32>,
    #[serde(skip)]
    pub deployment: Deployment,
    /// Où va chaque fichier du stockage : réutilisé tel quel pour
    /// réactiver, annuler un retrait ou revenir en arrière. Vide pour les
    /// mods installés avant son ajout (on recalcule alors).
    #[serde(skip)]
    pub placements: Vec<Placement>,
    /// Fichiers de config que le joueur (ou le mod) a modifiés dans le jeu :
    /// destination → contenu rangé. Remis tels quels à la réactivation.
    #[serde(skip)]
    pub edits: BTreeMap<String, String>,
    /// Versions d'avant, la plus récente en tête.
    pub previous_versions: Vec<PreviousVersion>,
}

/// Version remplacée par une mise à jour, gardée pour y revenir.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviousVersion {
    pub version: String,
    pub package: String,
    /// Secondes depuis l'époque Unix.
    pub installed_at: i64,
    pub replaced_at: i64,
    #[serde(skip)]
    pub sha256: String,
    #[serde(skip)]
    pub files: Vec<u32>,
    #[serde(skip)]
    pub placements: Vec<Placement>,
    #[serde(skip)]
    pub edits: BTreeMap<String, String>,
}

/// Versions précédentes gardées par mod.
pub const KEPT_VERSIONS: usize = 3;

impl InstalledMod {
    pub fn is_loader(&self) -> bool {
        self.loader_root.is_some()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("Unalia's database is unavailable")]
    Db(#[from] rusqlite::Error),
    #[error("Unalia's database is damaged")]
    Corrupt(#[from] serde_json::Error),
}

pub struct State {
    conn: Connection,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS installed_mods (
    game         TEXT NOT NULL,
    id           TEXT NOT NULL,
    name         TEXT NOT NULL,
    author       TEXT NOT NULL,
    version      TEXT NOT NULL,
    icon_url     TEXT,
    loader_root  TEXT,
    enabled      INTEGER NOT NULL,
    explicit     INTEGER NOT NULL,
    sha256       TEXT NOT NULL,
    installed_at INTEGER NOT NULL,
    store_dir    TEXT NOT NULL,
    deployment   TEXT NOT NULL,
    placements   TEXT NOT NULL DEFAULT '[]',
    files        TEXT NOT NULL DEFAULT '[]',
    package      TEXT NOT NULL DEFAULT '',
    edits        TEXT NOT NULL DEFAULT '{}',
    PRIMARY KEY (game, id)
);

CREATE TABLE IF NOT EXISTS previous_versions (
    game         TEXT NOT NULL,
    id           TEXT NOT NULL,
    package      TEXT NOT NULL,
    version      TEXT NOT NULL,
    sha256       TEXT NOT NULL,
    installed_at INTEGER NOT NULL,
    replaced_at  INTEGER NOT NULL,
    files        TEXT NOT NULL,
    placements   TEXT NOT NULL,
    edits        TEXT NOT NULL,
    PRIMARY KEY (game, id, package)
);

CREATE TABLE IF NOT EXISTS pending_ops (
    game TEXT PRIMARY KEY,
    ops  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS manual_games (
    app_id     INTEGER PRIMARY KEY,
    profile_id TEXT NOT NULL,
    path       TEXT NOT NULL,
    added_at   INTEGER NOT NULL
);
";

/// Jeu ajouté à la main (hors launcher), reconnu grâce au catalogue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualGame {
    /// Identifiant Steam du jeu s'il en a un, sinon identifiant dérivé du
    /// profil (voir `games::manual_app_id`).
    pub app_id: u32,
    /// Profil du catalogue (ex. `hollow-knight-silksong`).
    pub profile_id: String,
    pub path: String,
    pub added_at: i64,
}

impl State {
    pub fn open(path: &Path) -> Result<Self, StateError> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        Self::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self, StateError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self, StateError> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(SCHEMA)?;
        migrate(&conn)?;
        Ok(Self { conn })
    }

    /// Mods installés dans `game`, loader en tête puis par nom.
    pub fn list(&self, game: &str) -> Result<Vec<InstalledMod>, StateError> {
        let mut stmt = self.conn.prepare(
            "SELECT * FROM installed_mods WHERE game = ?1
             ORDER BY loader_root IS NULL, name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map(params![game], row)?;
        let mods: Vec<InstalledMod> = rows.map(|r| r?.finish()).collect::<Result<_, _>>()?;
        mods.into_iter().map(|m| self.with_previous(m)).collect()
    }

    fn with_previous(&self, mut m: InstalledMod) -> Result<InstalledMod, StateError> {
        m.previous_versions = self.previous(&m.game, &m.id)?;
        Ok(m)
    }

    pub fn get(&self, game: &str, id: &str) -> Result<Option<InstalledMod>, StateError> {
        self.conn
            .query_row(
                "SELECT * FROM installed_mods WHERE game = ?1 AND id = ?2",
                params![game, id],
                row,
            )
            .optional()?
            .map(|r| self.with_previous(r.finish()?))
            .transpose()
    }

    /// Jeux qui ont au moins un mod installé.
    pub fn games(&self) -> Result<Vec<String>, StateError> {
        let mut stmt = self
            .conn
            .prepare("SELECT DISTINCT game FROM installed_mods ORDER BY game")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// `id` est-il installé dans au moins un jeu ? Le cache des archives
    /// est commun à tous les jeux.
    pub fn installed_anywhere(&self, id: &str) -> Result<bool, StateError> {
        Ok(self
            .conn
            .query_row(
                "SELECT 1 FROM installed_mods WHERE id = ?1 LIMIT 1",
                params![id],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// Enregistre plusieurs mods d'un coup : tout ou rien.
    pub fn save_all(&mut self, mods: &[InstalledMod]) -> Result<(), StateError> {
        let tx = self.conn.transaction()?;
        for m in mods {
            tx.execute(
                "INSERT OR REPLACE INTO installed_mods
                 (game, id, name, author, version, icon_url, loader_root, enabled, explicit,
                  sha256, installed_at, store_dir, deployment, placements, files, package, edits)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                         ?16, ?17)",
                params![
                    m.game,
                    m.id,
                    m.name,
                    m.author,
                    m.version,
                    m.icon_url,
                    m.loader_root,
                    m.enabled,
                    m.explicit,
                    m.sha256,
                    m.installed_at,
                    m.legacy_store_dir,
                    serde_json::to_string(&m.deployment)?,
                    serde_json::to_string(&m.placements)?,
                    serde_json::to_string(&m.files)?,
                    m.package,
                    serde_json::to_string(&m.edits)?,
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn save(&mut self, m: &InstalledMod) -> Result<(), StateError> {
        self.save_all(std::slice::from_ref(m))
    }

    /// Le stockage des mods a été déplacé : réécrit les chemins enregistrés.
    pub fn rebase_store(&mut self, old: &Path, new: &Path) -> Result<(), StateError> {
        self.conn.execute(
            "UPDATE installed_mods SET store_dir = ?2 || substr(store_dir, length(?1) + 1)
             WHERE substr(store_dir, 1, length(?1)) = ?1",
            params![old.to_string_lossy(), new.to_string_lossy()],
        )?;
        Ok(())
    }

    /// Garde `m` comme version précédente de son mod, avec au plus
    /// `KEPT_VERSIONS` autres, les plus récentes.
    pub fn push_previous(&mut self, m: &InstalledMod, replaced_at: i64) -> Result<(), StateError> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT OR REPLACE INTO previous_versions VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                m.game,
                m.id,
                m.package,
                m.version,
                m.sha256,
                m.installed_at,
                replaced_at,
                serde_json::to_string(&m.files)?,
                serde_json::to_string(&m.placements)?,
                serde_json::to_string(&m.edits)?,
            ],
        )?;
        tx.execute(
            "DELETE FROM previous_versions WHERE game = ?1 AND id = ?2 AND package NOT IN
             (SELECT package FROM previous_versions WHERE game = ?1 AND id = ?2
              ORDER BY replaced_at DESC LIMIT ?3)",
            params![m.game, m.id, KEPT_VERSIONS as i64],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Versions précédentes d'un mod, la plus récente en tête.
    pub fn previous(&self, game: &str, id: &str) -> Result<Vec<PreviousVersion>, StateError> {
        let mut stmt = self.conn.prepare(
            "SELECT package, version, sha256, installed_at, replaced_at, files, placements, edits
             FROM previous_versions WHERE game = ?1 AND id = ?2 ORDER BY replaced_at DESC",
        )?;
        let rows = stmt.query_map(params![game, id], |r| {
            Ok((
                PreviousVersion {
                    package: r.get(0)?,
                    version: r.get(1)?,
                    sha256: r.get(2)?,
                    installed_at: r.get(3)?,
                    replaced_at: r.get(4)?,
                    files: Vec::new(),
                    placements: Vec::new(),
                    edits: BTreeMap::new(),
                },
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, String>(7)?,
            ))
        })?;
        rows.map(|r| {
            let (mut v, files, placements, edits) = r?;
            v.files = serde_json::from_str(&files)?;
            v.placements = serde_json::from_str(&placements)?;
            v.edits = serde_json::from_str(&edits)?;
            Ok(v)
        })
        .collect()
    }

    pub fn remove_previous(&mut self, game: &str, id: &str, package: &str) -> Result<(), StateError> {
        self.conn.execute(
            "DELETE FROM previous_versions WHERE game = ?1 AND id = ?2 AND package = ?3",
            params![game, id, package],
        )?;
        Ok(())
    }

    /// Oublie toutes les versions précédentes d'un mod.
    pub fn forget_previous(&mut self, game: &str, id: &str) -> Result<(), StateError> {
        self.conn.execute(
            "DELETE FROM previous_versions WHERE game = ?1 AND id = ?2",
            params![game, id],
        )?;
        Ok(())
    }

    /// Oublie les versions précédentes de tous les mods d'un jeu.
    pub fn forget_game_previous(&mut self, game: &str) -> Result<(), StateError> {
        self.conn.execute(
            "DELETE FROM previous_versions WHERE game = ?1",
            params![game],
        )?;
        Ok(())
    }

    /// Ce que le stockage doit garder, tous jeux confondus : les paquets
    /// des mods installés et de leurs versions précédentes, et les configs
    /// modifiées.
    pub fn referenced(&self) -> Result<(HashSet<String>, HashSet<String>), StateError> {
        let mut packages = HashSet::new();
        let mut blobs = HashSet::new();
        for table in ["installed_mods", "previous_versions"] {
            let mut stmt = self
                .conn
                .prepare(&format!("SELECT package, edits FROM {table}"))?;
            let rows =
                stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            for row in rows {
                let (package, edits) = row?;
                let edits: BTreeMap<String, String> = serde_json::from_str(&edits)?;
                packages.insert(package);
                blobs.extend(edits.into_values());
            }
        }
        Ok((packages, blobs))
    }

    /// Changements en attente de la fermeture du jeu, dans l'ordre.
    pub fn pending(&self, game: &str) -> Result<Vec<PendingOp>, StateError> {
        let ops: Option<String> = self
            .conn
            .query_row(
                "SELECT ops FROM pending_ops WHERE game = ?1",
                params![game],
                |r| r.get(0),
            )
            .optional()?;
        Ok(ops
            .map(|o| serde_json::from_str(&o))
            .transpose()?
            .unwrap_or_default())
    }

    pub fn set_pending(&mut self, game: &str, ops: &[PendingOp]) -> Result<(), StateError> {
        if ops.is_empty() {
            self.conn
                .execute("DELETE FROM pending_ops WHERE game = ?1", params![game])?;
        } else {
            self.conn.execute(
                "INSERT OR REPLACE INTO pending_ops VALUES (?1, ?2)",
                params![game, serde_json::to_string(ops)?],
            )?;
        }
        Ok(())
    }

    pub fn manual_games(&self) -> Result<Vec<ManualGame>, StateError> {
        let mut stmt = self.conn.prepare(
            "SELECT app_id, profile_id, path, added_at FROM manual_games ORDER BY added_at",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(ManualGame {
                app_id: r.get(0)?,
                profile_id: r.get(1)?,
                path: r.get(2)?,
                added_at: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Ajoute (ou remplace, si le jeu change de dossier) un jeu manuel.
    pub fn save_manual_game(&mut self, game: &ManualGame) -> Result<(), StateError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO manual_games VALUES (?1, ?2, ?3, ?4)",
            params![game.app_id, game.profile_id, game.path, game.added_at],
        )?;
        Ok(())
    }

    pub fn remove_manual_game(&mut self, app_id: u32) -> Result<(), StateError> {
        self.conn.execute(
            "DELETE FROM manual_games WHERE app_id = ?1",
            params![app_id],
        )?;
        Ok(())
    }

    /// Réglage de l'appli (ex. la clé API Nexus de l'utilisateur, qui ne
    /// quitte jamais sa machine).
    pub fn setting(&self, key: &str) -> Result<Option<String>, StateError> {
        Ok(self
            .conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![key],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// `None` retire le réglage.
    pub fn set_setting(&mut self, key: &str, value: Option<&str>) -> Result<(), StateError> {
        match value {
            Some(value) => self.conn.execute(
                "INSERT OR REPLACE INTO settings VALUES (?1, ?2)",
                params![key, value],
            )?,
            None => self
                .conn
                .execute("DELETE FROM settings WHERE key = ?1", params![key])?,
        };
        Ok(())
    }

    pub fn remove(&mut self, game: &str, id: &str) -> Result<(), StateError> {
        self.conn.execute(
            "DELETE FROM installed_mods WHERE game = ?1 AND id = ?2",
            params![game, id],
        )?;
        Ok(())
    }
}

/// Ligne brute : le déploiement (JSON) est décodé ensuite, pour que son
/// erreur éventuelle ne soit pas confondue avec une erreur SQL.
struct Row {
    m: InstalledMod,
    deployment: String,
    placements: String,
    files: String,
    edits: String,
}

impl Row {
    fn finish(self) -> Result<InstalledMod, StateError> {
        let mut m = self.m;
        m.deployment = serde_json::from_str(&self.deployment)?;
        m.placements = serde_json::from_str(&self.placements)?;
        m.files = serde_json::from_str(&self.files)?;
        m.edits = serde_json::from_str(&self.edits)?;
        Ok(m)
    }
}

/// Bases créées par une version précédente : colonnes ajoutées depuis.
fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    for (column, default) in [
        ("placements", "'[]'"),
        ("files", "'[]'"),
        ("package", "''"),
        ("edits", "'{}'"),
    ] {
        let present = conn
            .prepare("SELECT 1 FROM pragma_table_info('installed_mods') WHERE name = ?1")?
            .exists([column])?;
        if !present {
            conn.execute_batch(&format!(
                "ALTER TABLE installed_mods ADD COLUMN {column} TEXT NOT NULL DEFAULT {default}"
            ))?;
        }
    }
    Ok(())
}

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Row> {
    Ok(Row {
        m: InstalledMod {
            game: r.get("game")?,
            id: r.get("id")?,
            name: r.get("name")?,
            author: r.get("author")?,
            version: r.get("version")?,
            icon_url: r.get("icon_url")?,
            loader_root: r.get("loader_root")?,
            enabled: r.get("enabled")?,
            explicit: r.get("explicit")?,
            sha256: r.get("sha256")?,
            installed_at: r.get("installed_at")?,
            package: r.get("package")?,
            legacy_store_dir: r.get("store_dir")?,
            files: Vec::new(),
            deployment: Deployment::default(),
            placements: Vec::new(),
            edits: BTreeMap::new(),
            previous_versions: Vec::new(),
        },
        deployment: r.get("deployment")?,
        placements: r.get("placements")?,
        files: r.get("files")?,
        edits: r.get("edits")?,
    })
}

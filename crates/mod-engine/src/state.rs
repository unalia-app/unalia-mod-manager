//! Brique **state** : ce que Modalia a installé, dans une base SQLite.
//!
//! Pour chaque mod installé dans un jeu : sa version, son état actif ou non,
//! l'empreinte de l'archive, et la liste exacte des fichiers posés dans le
//! jeu (le déploiement), sans laquelle on ne saurait pas tout retirer.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use crate::deploy::Deployment;

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
    /// Dossier du mod dans le stockage de Modalia.
    pub store_dir: String,
    #[serde(skip)]
    pub deployment: Deployment,
}

impl InstalledMod {
    pub fn is_loader(&self) -> bool {
        self.loader_root.is_some()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("Modalia's database is unavailable")]
    Db(#[from] rusqlite::Error),
    #[error("Modalia's database is damaged")]
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
    PRIMARY KEY (game, id)
);
";

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
        Ok(Self { conn })
    }

    /// Mods installés dans `game`, loader en tête puis par nom.
    pub fn list(&self, game: &str) -> Result<Vec<InstalledMod>, StateError> {
        let mut stmt = self.conn.prepare(
            "SELECT * FROM installed_mods WHERE game = ?1
             ORDER BY loader_root IS NULL, name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map(params![game], row)?;
        rows.map(|r| r?.finish()).collect()
    }

    pub fn get(&self, game: &str, id: &str) -> Result<Option<InstalledMod>, StateError> {
        self.conn
            .query_row(
                "SELECT * FROM installed_mods WHERE game = ?1 AND id = ?2",
                params![game, id],
                row,
            )
            .optional()?
            .map(Row::finish)
            .transpose()
    }

    /// Enregistre plusieurs mods d'un coup : tout ou rien.
    pub fn save_all(&mut self, mods: &[InstalledMod]) -> Result<(), StateError> {
        let tx = self.conn.transaction()?;
        for m in mods {
            tx.execute(
                "INSERT OR REPLACE INTO installed_mods VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
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
                    m.store_dir,
                    serde_json::to_string(&m.deployment)?,
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn save(&mut self, m: &InstalledMod) -> Result<(), StateError> {
        self.save_all(std::slice::from_ref(m))
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
}

impl Row {
    fn finish(self) -> Result<InstalledMod, StateError> {
        let mut m = self.m;
        m.deployment = serde_json::from_str(&self.deployment)?;
        Ok(m)
    }
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
            store_dir: r.get("store_dir")?,
            deployment: Deployment::default(),
        },
        deployment: r.get("deployment")?,
    })
}

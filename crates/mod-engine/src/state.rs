//! Brique **state** : ce que Modalia a installé, dans une base SQLite.
//!
//! Pour chaque mod installé dans un jeu : sa version, son état actif ou non,
//! l'empreinte de l'archive, et la liste exacte des fichiers posés dans le
//! jeu (le déploiement), sans laquelle on ne saurait pas tout retirer.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use crate::deploy::Deployment;
use crate::launch::PendingOp;

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

CREATE TABLE IF NOT EXISTS pending_ops (
    game TEXT PRIMARY KEY,
    ops  TEXT NOT NULL
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

    /// Le stockage des mods a été déplacé : réécrit les chemins enregistrés.
    pub fn rebase_store(&mut self, old: &Path, new: &Path) -> Result<(), StateError> {
        self.conn.execute(
            "UPDATE installed_mods SET store_dir = ?2 || substr(store_dir, length(?1) + 1)
             WHERE substr(store_dir, 1, length(?1)) = ?1",
            params![old.to_string_lossy(), new.to_string_lossy()],
        )?;
        Ok(())
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

//! **state** block: what Modalia installed, in a SQLite database.
//!
//! For each mod installed in a game: its version, whether it is enabled,
//! the archive hash, and the exact list of files placed in the game (the
//! deployment), without which we couldn't remove everything. Versions
//! replaced by an update stay known for a while: going back to one
//! downloads nothing again.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use crate::deploy::Deployment;
use crate::launch::PendingOp;
use crate::loaders::Placement;
use crate::sources::nexus::Requirement;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledMod {
    /// Game, e.g. `steam:1030300`.
    pub game: String,
    pub id: String,
    pub name: String,
    pub author: String,
    pub version: String,
    pub icon_url: Option<String>,
    /// Loader package: its root folder in the archive (e.g. `BepInExPack`).
    pub loader_root: Option<String>,
    pub enabled: bool,
    /// Requested by the user (else: installed as a dependency).
    pub explicit: bool,
    pub sha256: String,
    /// Seconds since the Unix epoch.
    pub installed_at: i64,
    /// Package in the storage (`store::Store`) that holds its files.
    pub package: String,
    /// The mod's folder in the old storage, from before packages: empty
    /// once migrated.
    #[serde(skip)]
    pub legacy_store_dir: String,
    /// Installed Nexus files, in the order they are placed (the main file
    /// then its variants): picked up again by updates.
    pub files: Vec<u32>,
    /// Source it was installed from (`thunderstore`, `hexium`): its
    /// updates come from there. Empty: the game's source (mods installed
    /// before it was recorded, Nexus games).
    pub source: String,
    /// Place in the game's order: on a file several mods place, the highest
    /// wins. Set by the first install and kept by updates, so an updated
    /// mod doesn't go back over the patch made for it.
    pub priority: i64,
    /// Files actually in the game: not the ones another mod covers.
    #[serde(skip)]
    pub deployment: Deployment,
    /// Mods whose files are in the game in place of some of this one's
    /// (`installer::mark_covered`).
    pub covered_by: Vec<String>,
    /// What it needs and Unalia couldn't install by itself, as of its last
    /// install (`installer::drop_installed_needs` for the UI).
    pub needs: Vec<Requirement>,
    /// Where each storage file goes: reused as is to enable again, undo a
    /// removal or roll back. Empty for mods installed before it was added
    /// (it is computed again then).
    #[serde(skip)]
    pub placements: Vec<Placement>,
    /// Config files that the player (or the mod) edited in the game:
    /// destination to stored content. Put back as is when enabled again.
    #[serde(skip)]
    pub edits: BTreeMap<String, String>,
    /// Previous versions, the most recent first.
    pub previous_versions: Vec<PreviousVersion>,
}

/// Version replaced by an update, kept to go back to.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviousVersion {
    pub version: String,
    pub package: String,
    /// Seconds since the Unix epoch.
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

/// Previous versions kept per mod.
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
    source       TEXT NOT NULL DEFAULT '',
    priority     INTEGER NOT NULL DEFAULT 0,
    needs        TEXT NOT NULL DEFAULT '[]',
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

/// Manually added game (outside a launcher), recognized from the catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualGame {
    /// The game's Steam id if it has one, else an id derived from the
    /// profile (see `games::manual_app_id`).
    pub app_id: u32,
    /// Catalog profile (e.g. `hollow-knight-silksong`).
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

    /// Mods installed in `game`, loader first, then by name.
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

    /// Games that have at least one mod installed.
    pub fn games(&self) -> Result<Vec<String>, StateError> {
        let mut stmt = self
            .conn
            .prepare("SELECT DISTINCT game FROM installed_mods ORDER BY game")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Is `id` installed in at least one game? The archive cache is shared
    /// by every game.
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

    /// Records several mods at once: all or nothing.
    pub fn save_all(&mut self, mods: &[InstalledMod]) -> Result<(), StateError> {
        let tx = self.conn.transaction()?;
        for m in mods {
            tx.execute(
                "INSERT OR REPLACE INTO installed_mods
                 (game, id, name, author, version, icon_url, loader_root, enabled, explicit,
                  sha256, installed_at, store_dir, deployment, placements, files, package, edits,
                  source, priority, needs)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                         ?16, ?17, ?18, ?19, ?20)",
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
                    m.source,
                    m.priority,
                    serde_json::to_string(&m.needs)?,
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn save(&mut self, m: &InstalledMod) -> Result<(), StateError> {
        self.save_all(std::slice::from_ref(m))
    }

    /// Frees place `from` in the game's order, for a mod to go there.
    pub fn make_room(&mut self, game: &str, from: i64) -> Result<(), StateError> {
        self.conn.execute(
            "UPDATE installed_mods SET priority = priority + 1 WHERE game = ?1 AND priority >= ?2",
            params![game, from],
        )?;
        Ok(())
    }

    /// The mod storage moved: rewrites the recorded paths.
    pub fn rebase_store(&mut self, old: &Path, new: &Path) -> Result<(), StateError> {
        self.conn.execute(
            "UPDATE installed_mods SET store_dir = ?2 || substr(store_dir, length(?1) + 1)
             WHERE substr(store_dir, 1, length(?1)) = ?1",
            params![old.to_string_lossy(), new.to_string_lossy()],
        )?;
        Ok(())
    }

    /// Keeps `m` as a previous version of its mod, with at most
    /// `KEPT_VERSIONS` others, the most recent ones.
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

    /// Previous versions of a mod, the most recent first.
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

    /// Forgets every previous version of a mod.
    pub fn forget_previous(&mut self, game: &str, id: &str) -> Result<(), StateError> {
        self.conn.execute(
            "DELETE FROM previous_versions WHERE game = ?1 AND id = ?2",
            params![game, id],
        )?;
        Ok(())
    }

    /// Forgets the previous versions of every mod of a game.
    pub fn forget_game_previous(&mut self, game: &str) -> Result<(), StateError> {
        self.conn.execute(
            "DELETE FROM previous_versions WHERE game = ?1",
            params![game],
        )?;
        Ok(())
    }

    /// What the storage must keep, across all games: the packages of the
    /// installed mods and of their previous versions, and the edited
    /// configs.
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

    /// Changes waiting for the game to close, in order.
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

    /// Adds (or replaces, if the game moved) a manual game.
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

    /// App setting (e.g. the user's Nexus API key, which never leaves their
    /// machine).
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

    /// `None` removes the setting.
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

/// Raw row: the deployment (JSON) is decoded afterwards, so that a possible
/// error there isn't mistaken for an SQL error.
struct Row {
    m: InstalledMod,
    deployment: String,
    placements: String,
    files: String,
    edits: String,
    needs: String,
}

impl Row {
    fn finish(self) -> Result<InstalledMod, StateError> {
        let mut m = self.m;
        m.deployment = serde_json::from_str(&self.deployment)?;
        m.placements = serde_json::from_str(&self.placements)?;
        m.files = serde_json::from_str(&self.files)?;
        m.edits = serde_json::from_str(&self.edits)?;
        m.needs = serde_json::from_str(&self.needs)?;
        Ok(m)
    }
}

/// Databases created by a previous version: columns added since.
fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    for (column, kind, default) in [
        ("placements", "TEXT", "'[]'"),
        ("files", "TEXT", "'[]'"),
        ("package", "TEXT", "''"),
        ("edits", "TEXT", "'{}'"),
        ("source", "TEXT", "''"),
        ("priority", "INTEGER", "0"),
        ("needs", "TEXT", "'[]'"),
    ] {
        let present = conn
            .prepare("SELECT 1 FROM pragma_table_info('installed_mods') WHERE name = ?1")?
            .exists([column])?;
        if !present {
            conn.execute_batch(&format!(
                "ALTER TABLE installed_mods ADD COLUMN {column} {kind} NOT NULL DEFAULT {default}"
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
            source: r.get("source")?,
            priority: r.get("priority")?,
            covered_by: Vec::new(),
            needs: Vec::new(),
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
        needs: r.get("needs")?,
    })
}

//! Single-writer SQLite authority with optimistic revision checks for every edit.
//! FULL synchronous WAL commits are acknowledgements of SQLite's durable boundary,
//! not guarantees about uncommitted events or faulty storage hardware.
mod organisation;
mod proposals;
mod writing;

use crate::{
    security::{Access, Scope},
    Error, Id, Result,
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::{path::Path, time::Duration};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Module,
    Project,
    Notebook,
    Note,
    Canvas,
}
impl Kind {
    pub fn accepts(self, child: Kind) -> bool {
        match self {
            Self::Module => true,
            Self::Project => matches!(child, Self::Notebook | Self::Note | Self::Canvas),
            Self::Notebook => matches!(child, Self::Note | Self::Canvas),
            Self::Note | Self::Canvas => false,
        }
    }
    pub fn is_surface(self) -> bool {
        matches!(self, Self::Note | Self::Canvas)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entity {
    pub id: Id,
    pub kind: Kind,
    pub parent: Option<Id>,
    pub title: String,
    pub created_ms: i64,
    pub modified_ms: i64,
    pub revision: u64,
    pub locked: bool,
    pub trashed: bool,
    pub purged: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum WritingMode {
    InkOnly,
    #[default]
    Manual,
    Assisted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveState {
    SavedLocally,
    Saving,
    NeedsAttention,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailurePoint {
    None,
    BeforeCommit,
}

/// Connection is not exposed to adapters or callers. All data reads pass policy.
pub struct Store {
    pub(crate) db: Connection,
    failure: FailurePoint,
    save_state: SaveState,
}

const V1: &str = include_str!("../migrations/001.sql");
const V2: &str = include_str!("../migrations/002.sql");

impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_connection(Connection::open(path)?)
    }
    pub fn memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }
    fn from_connection(mut db: Connection) -> Result<Self> {
        db.busy_timeout(Duration::from_millis(250))?;
        db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA trusted_schema=OFF;")?;
        let version: i64 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if !(0..=2).contains(&version) {
            return Err(Error::Unsupported);
        }
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if version < 1 {
            tx.execute_batch(V1)?;
        }
        if version < 2 {
            tx.execute_batch(V2)?;
        }
        tx.commit()?;
        Ok(Self {
            db,
            failure: FailurePoint::None,
            save_state: SaveState::SavedLocally,
        })
    }
    pub fn schema_version(&self) -> Result<i64> {
        Ok(self
            .db
            .pragma_query_value(None, "user_version", |r| r.get(0))?)
    }
    pub fn check_integrity(&self) -> Result<()> {
        let check: String = self
            .db
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        if check != "ok" {
            return Err(Error::Integrity);
        }
        let bad = self.db.prepare("PRAGMA foreign_key_check")?.exists([])?;
        if bad {
            return Err(Error::Integrity);
        }
        Ok(())
    }
    /// Fault injection only fails an operation; it cannot bypass validation.
    pub fn inject_failure(&mut self, point: FailurePoint) {
        self.failure = point;
    }
    pub fn save_state(&self) -> SaveState {
        self.save_state
    }
    pub(crate) fn write<T>(&mut self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        self.save_state = SaveState::Saving;
        let failure = std::mem::replace(&mut self.failure, FailurePoint::None);
        let result = (|| {
            let tx = self
                .db
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let value = f(&tx)?;
            tx.execute(
                "UPDATE settings SET value=CAST(value AS INTEGER)+1 WHERE key='generation'",
                [],
            )?;
            if failure == FailurePoint::BeforeCommit {
                return Err(Error::Interrupted);
            }
            tx.commit()?;
            Ok(value)
        })();
        self.save_state = if result.is_ok() {
            SaveState::SavedLocally
        } else {
            SaveState::NeedsAttention
        };
        result
    }
    pub(crate) fn setting(db: &Connection, key: &str) -> Result<String> {
        Ok(
            db.query_row("SELECT value FROM settings WHERE key=?1", [key], |r| {
                r.get(0)
            })?,
        )
    }
    pub(crate) fn raw_entity(db: &Connection, id: Id) -> Result<Entity> {
        let json: Option<String> = db
            .query_row(
                "SELECT payload FROM entities WHERE id=?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        serde_json::from_str(&json.ok_or(Error::Unavailable)?).map_err(Into::into)
    }
    pub(crate) fn save_entity(db: &Connection, e: &Entity) -> Result<()> {
        db.execute("INSERT INTO entities(id,parent,payload) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET parent=excluded.parent,payload=excluded.payload",
            rusqlite::params![e.id.to_string(),e.parent.map(|p|p.to_string()),serde_json::to_string(e)?])?;
        Ok(())
    }
    pub(crate) fn authorise(
        db: &Connection,
        id: Id,
        access: &Access,
        allow_trash: bool,
    ) -> Result<Entity> {
        if Self::setting(db, "app_locked")? == "true" && !access.permits(&Scope::App) {
            return Err(Error::Denied);
        }
        let entity = Self::raw_entity(db, id)?;
        let mut current = Some(id);
        let mut visited = std::collections::BTreeSet::new();
        while let Some(i) = current {
            if !visited.insert(i) {
                return Err(Error::Integrity);
            }
            let item = Self::raw_entity(db, i)?;
            if item.purged || (!allow_trash && item.trashed) {
                return Err(Error::Unavailable);
            }
            if item.locked && !access.permits(&Scope::Entity(i)) {
                return Err(Error::Denied);
            }
            current = item.parent;
        }
        Ok(entity)
    }
    pub(crate) fn require_app(db: &Connection, access: &Access) -> Result<()> {
        if Self::setting(db, "app_locked")? == "true" && !access.permits(&Scope::App) {
            return Err(Error::Denied);
        }
        Ok(())
    }
    pub fn set_app_lock(&mut self, enabled: bool, access: &Access) -> Result<()> {
        // Both enabling and disabling protection require platform authentication.
        if !access.permits(&Scope::App) {
            return Err(Error::Denied);
        }
        self.write(|db| {
            db.execute(
                "UPDATE settings SET value=?1 WHERE key='app_locked'",
                [enabled.to_string()],
            )?;
            Ok(())
        })
    }
}

pub use organisation::{DeletionReview, Link, LinkState, Order};
pub use proposals::{Proposal, ProposalState};
pub use writing::{Block, Display, HistoryEntry, InkSnapshot};

#[cfg(test)]
mod storage_tests {
    use super::*;
    #[test]
    fn sqlite_full_reports_failure_and_preserves_committed_work() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path().join("full.db")).unwrap();
        let a = Access::public();
        let e = s.create(Kind::Note, None, "committed", &a).unwrap();
        let pages: i64 =
            s.db.pragma_query_value(None, "page_count", |r| r.get(0))
                .unwrap();
        s.db.pragma_update(None, "max_page_count", pages).unwrap();
        let result = s.append_content(
            e.id,
            crate::content::Content::Text("x".repeat(1_000_000)),
            &a,
        );
        match result {
            Err(Error::Storage(rusqlite::Error::SqliteFailure(code, _))) => {
                assert_eq!(code.code, rusqlite::ErrorCode::DiskFull)
            }
            other => panic!("expected actual SQLite FULL, got {other:?}"),
        }
        assert_eq!(s.save_state(), SaveState::NeedsAttention);
        assert_eq!(s.entity(e.id, &a).unwrap(), e);
        assert!(s.blocks(e.id, 0, 100, &a).unwrap().is_empty());
        s.check_integrity().unwrap();
    }
}

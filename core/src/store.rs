//! Single-writer SQLite authority with optimistic revision checks for every edit.
//! FULL synchronous WAL commits are acknowledgements of SQLite's durable boundary,
//! not guarantees about uncommitted events or faulty storage hardware.
mod authoring;
mod canvas_table_scene;
mod nested_table_scene;
mod view_table_scene;
pub use nested_table_scene::{
    CanvasEmbedPathStep, CanvasInkClip, NestedCanvasTableInkPage, NestedCanvasTableInkScope,
};
pub use view_table_scene::{
    WritingViewTablePage, WritingViewTableRequest, WritingViewTableScope, WritingViewTableSource,
};
mod capture_recovery;
mod editing;
mod editing_links;
mod engines;
mod exercises;
mod layers;
mod navigation;
mod objects;
mod organisation;
mod positions;
mod proposals;
mod regions;
mod table_copy;
mod table_move;
mod table_scene;
pub use canvas_table_scene::{CanvasEmbedRevision, CanvasTableInkPage, CanvasTableInkScope};
pub use table_scene::{CellInkFragment, TableInkCursor, TableInkScenePage};
mod table_ink;
mod table_transfer;
pub use table_ink::CellInkPlacement;
pub use table_transfer::{CellTarget, CellTransferMode};
mod workspace;
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
    // One authoritative Store is shared by the host's master/exercise panes.
    document_save_states: std::collections::BTreeMap<Id, SaveState>,
}

const V1: &str = include_str!("../migrations/001.sql");
const V2: &str = include_str!("../migrations/002.sql");
const V3: &str = include_str!("../migrations/003.sql");
const V4: &str = include_str!("../migrations/004.sql");
const V5: &str = include_str!("../migrations/005.sql");
const V6: &str = include_str!("../migrations/006.sql");
const V7: &str = include_str!("../migrations/007.sql");
const V15: &str = include_str!("../migrations/015.sql");
const V16: &str = include_str!("../migrations/016.sql");
const V18: &str = include_str!("../migrations/018.sql");
const V19: &str = include_str!("../migrations/019.sql");
const V20: &str = include_str!("../migrations/020.sql");
const V21: &str = include_str!("../migrations/021.sql");
const V22: &str = include_str!("../migrations/022.sql");
const V17: &str = include_str!("../migrations/017.sql");
const V14: &str = include_str!("../migrations/014.sql");
const V13: &str = include_str!("../migrations/013.sql");
const V12: &str = include_str!("../migrations/012.sql");
const V11: &str = include_str!("../migrations/011.sql");
const V10: &str = include_str!("../migrations/010.sql");
const V9: &str = include_str!("../migrations/009.sql");
const V8: &str = include_str!("../migrations/008.sql");

impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_connection(Connection::open(path)?)
    }
    pub fn memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }
    pub(crate) fn from_connection(mut db: Connection) -> Result<Self> {
        db.busy_timeout(Duration::from_millis(250))?;
        db.execute_batch(
            "PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; \
                PRAGMA trusted_schema=OFF;",
        )?;
        let version: i64 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if !(0..=22).contains(&version) {
            return Err(Error::Unsupported);
        }
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if version < 1 {
            tx.execute_batch(V1)?;
        }
        if version < 2 {
            tx.execute_batch(V2)?;
        }
        if version < 3 {
            tx.execute_batch(V3)?;
        }
        if version < 4 {
            tx.execute_batch(V4)?;
        }
        if version < 5 {
            tx.execute_batch(V5)?;
        }
        if version < 6 {
            tx.execute_batch(V6)?;
        }
        if version < 7 {
            tx.execute_batch(V7)?;
        }
        if version < 8 {
            tx.execute_batch(V8)?;
        }
        if version < 9 {
            tx.execute_batch(V9)?;
        }
        if version < 10 {
            tx.execute_batch(V10)?;
        }
        if version < 11 {
            tx.execute_batch(V11)?;
        }
        if version < 12 {
            tx.execute_batch(V12)?;
        }
        if version < 13 {
            tx.execute_batch(V13)?;
        }
        if version < 14 {
            tx.execute_batch(V14)?;
        }
        if version < 15 {
            tx.execute_batch(V15)?;
        }
        if version < 16 {
            tx.execute_batch(V16)?;
        }
        if version < 17 {
            tx.execute_batch(V17)?;
        }
        if version < 18 {
            tx.execute_batch(V18)?;
        }
        if version < 19 {
            tx.execute_batch(V19)?;
        }
        if version < 20 {
            tx.execute_batch(V20)?;
        }
        if version < 21 {
            tx.execute_batch(V21)?;
        }
        if version < 22 {
            tx.execute_batch(V22)?;
        }
        tx.commit()?;
        Ok(Self {
            db,
            failure: FailurePoint::None,
            save_state: SaveState::SavedLocally,
            document_save_states: Default::default(),
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
    pub fn document_save_state(&self, entity: Id, access: &Access) -> Result<SaveState> {
        Self::authorise(&self.db, entity, access, false)?;
        Ok(self
            .document_save_states
            .get(&entity)
            .copied()
            .unwrap_or(SaveState::SavedLocally))
    }
    /// Reuse the authoring journal inside an existing transaction, including
    /// one atomic batch spanning several document-local undo operations.
    pub(crate) fn journal_for<T>(
        db: &Connection,
        entity: Id,
        f: impl FnOnce(&Connection) -> Result<T>,
    ) -> Result<T> {
        Self::journal_for_operation(db, entity, f).map(|(value, _)| value)
    }
    pub(crate) fn journal_for_operation<T>(
        db: &Connection,
        entity: Id,
        f: impl FnOnce(&Connection) -> Result<T>,
    ) -> Result<(T, i64)> {
        db.execute("INSERT INTO edit_operations(entity,active_ms) VALUES(?1,CAST((SELECT value FROM settings WHERE key='active_ms') AS INTEGER))", [entity.to_string()])?;
        let operation = db.last_insert_rowid();
        db.execute(
            "UPDATE settings SET value=?1 WHERE key='edit_operation'",
            [operation.to_string()],
        )?;
        let value = f(db)?;
        db.execute(
            "UPDATE settings SET value='' WHERE key='edit_operation'",
            [],
        )?;
        let changed = db
            .prepare("SELECT 1 FROM edit_changes WHERE operation=?1")?
            .exists([operation])?;
        if changed {
            db.execute(
                "DELETE FROM edit_operations WHERE entity=?1 AND undone=1 AND id<>?2",
                rusqlite::params![entity.to_string(), operation],
            )?;
        } else {
            db.execute("DELETE FROM edit_operations WHERE id=?1", [operation])?;
        }
        Ok((value, operation))
    }
    pub(crate) fn write_for<T>(
        &mut self,
        entity: Id,
        f: impl FnOnce(&Connection) -> Result<T>,
    ) -> Result<T> {
        self.document_save_states.insert(entity, SaveState::Saving);
        let result = self.write(|db| Self::journal_for(db, entity, f));
        self.document_save_states.insert(
            entity,
            if result.is_ok() {
                SaveState::SavedLocally
            } else {
                SaveState::NeedsAttention
            },
        );
        result
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
        db.execute(
            "INSERT INTO entities(id,parent,payload) VALUES(?1,?2,?3) ON CONFLICT(id) DO \
                UPDATE SET parent=excluded.parent,payload=excluded.payload",
            rusqlite::params![
                e.id.to_string(),
                e.parent.map(|p| p.to_string()),
                serde_json::to_string(e)?
            ],
        )?;
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

pub use capture_recovery::CaptureTargetState;
pub use engines::{
    ConsentProvenance, DiscardedEngineImport, EngineActivationPlan, EngineCatalogue,
    EngineCatalogueRow, EngineDiscardPermit, EngineDiscardPlan, EngineImportPlan,
    EngineImportTransfer, EngineReclamation, EngineReclamationPermit, EngineReclamationPlan,
    EngineReclamationReview, EngineReclamationSummary, EngineReclamations, EngineRecoveryPlan,
    EngineRemovalPlan, EngineResourceAction, EngineResourceActions, EngineVersion, Installation,
    PreparedEngineActivation, PreparedEngineDiscard, PreparedEngineImport,
    PreparedEngineReclamation, ReclaimedEngineResource, ReclamationConsent, ReclamationDecision,
    ReclamationFacts, ResourceActionKind, ResourceActionState, ResourceEntry, ResourceEntryKind,
};
pub use exercises::{
    Exercise, ExtractionCandidate, ExtractionPlan, MembershipChange, MembershipOperation,
    MembershipPlan, WritingTarget,
};
pub use navigation::SearchHit;
pub use objects::{Camera, Embed, Layout, MasterLocation, ObjectGroup, SurfaceItem, ViewSettings};
pub use organisation::{DeletionReview, Link, LinkState, Order};
pub use proposals::{Proposal, ProposalState, ProposalSummary};
pub use writing::{Block, Display, HistoryEntry, InkSnapshot, Region};

pub use positions::{
    BookmarkState, CaretPoint, CaretTarget, GraphLabel, InlinePoint, RestoredView,
    SelectionBookmark,
};

pub use workspace::{
    PaneTarget, RestoredPane, RestoredPaneState, RestoredWorkspace, SplitAxis, WorkspacePane,
    WorkspaceTool, WorkspaceUpdate,
};

pub use layers::{
    CanvasLayer, CanvasLayers, CanvasScene, CanvasSceneContent, CanvasSceneItem, LayerRef,
};

pub use authoring::{
    AuthoringSettings, EraserMode, EraserSettings, GuideBackground, GuideKind, PenPreset,
    ViewEraserSnapshot, ViewGuideSnapshot, ViewPenSnapshot,
};

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

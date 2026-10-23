//! Whole-library logical backup. Fixed schema and parameterised row writes: never
//! execute SQL supplied by a backup. Restore builds an independent database and
//! cannot overwrite the live Store. Encrypted output is the only public writer.
use crate::{
    attachments::{Attachments, NativeAsset, NativeLayout},
    crypto, digest,
    portable::{NativeBlock, NativeDocument, NativeMembership, NativeMode, NativeRule},
    security::{Access, Authenticator, Scope},
    sources::{AssetClass, Availability, PdfAnchor, Retention, Source},
    store::{
        Block, Embed, EngineVersion, Entity, Exercise, HistoryEntry, Link, MembershipOperation,
        ObjectGroup, Proposal, Store, ViewSettings,
    },
    Error, Id, Result,
};

use rusqlite::{
    params_from_iter,
    types::{Value, ValueRef},
};

use serde::{Deserialize, Serialize};

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

// Column order is the on-disk contract of schemas 9/10/11/12/13. Unknown tables/widths fail closed.
const TABLES: &[(&str, usize)] = &[
    ("settings", 2),
    ("entities", 3),
    ("links", 5),
    ("modes", 2),
    ("blocks", 4),
    ("strokes", 4),
    ("history", 4),
    ("proposals", 4),
    ("sources", 3),
    ("source_assets", 6),
    ("exercises", 4),
    ("exercise_members", 2),
    ("membership_versions", 2),
    ("confirmed_headings", 1),
    ("exercise_rules", 2),
    ("membership_operations", 3),
    ("image_assets", 5),
    ("layouts", 2),
    ("object_groups", 3),
    ("embeds", 4),
    ("view_settings", 3),
    ("capture_receipts", 3),
    ("engine_versions", 3),
    ("engine_active", 2),
    ("engine_defaults", 2),
    ("pdf_anchors", 4),
    ("edit_operations", 4),
    ("edit_changes", 5),
    ("workspace_panes", 7),
    ("workspace_state", 4),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", deny_unknown_fields)]
enum Cell {
    Null,
    Integer(i64),
    Text(String),
    Blob(Vec<u8>),
}

impl Cell {
    fn value(&self) -> Value {
        match self {
            Self::Null => Value::Null,
            Self::Integer(n) => Value::Integer(*n),
            Self::Text(s) => Value::Text(s.clone()),
            Self::Blob(b) => Value::Blob(b.clone()),
        }
    }

    fn text(&self) -> Result<&str> {
        if let Self::Text(s) = self {
            Ok(s)
        } else {
            Err(Error::Invalid("backup cell type"))
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    format: String,
    schema: u32,
    tables: BTreeMap<String, Vec<Vec<Cell>>>,
    omissions: Vec<String>,
}

pub struct BackupPlan {
    library: String,
    generation: String,
    snapshot: Snapshot,
    scopes: BTreeSet<Scope>,
    pub includes_original_ink: bool,
    pub source_assets: usize,
    pub omissions: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct BackupOptions {
    pub include_original_ink: bool,
    pub include_authorised_source_assets: bool,
}

impl Store {
    fn backup_scopes(&self) -> Result<BTreeSet<Scope>> {
        let mut scopes = BTreeSet::new();

        if Self::setting(&self.db, "app_locked")? == "true" {
            scopes.insert(Scope::App);
        }

        for (table, scope) in [("entities", 0), ("blocks", 1), ("exercises", 2)] {
            let mut q = self
                .db
                .prepare(&format!("SELECT id,payload FROM {table}"))?;

            for row in q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
                let (id, json) = row?;

                let value: serde_json::Value = serde_json::from_str(&json)?;

                if value["locked"].as_bool() == Some(true) {
                    let id = Id::parse(&id)?;

                    scopes.insert(match scope {
                        0 => Scope::Entity(id),
                        1 => Scope::Block(id),
                        _ => Scope::Exercise(id),
                    });
                }
            }
        }

        Ok(scopes)
    }

    pub fn prepare_backup(&self, options: BackupOptions, access: &Access) -> Result<BackupPlan> {
        Self::require_app(&self.db, access)?;

        let generation = Self::setting(&self.db, "generation")?;

        let library = Self::setting(&self.db, "library_id")?;

        let scopes = self.backup_scopes()?;

        if scopes.iter().any(|s| !access.permits(s)) {
            return Err(Error::Denied);
        }

        let ink = self
            .db
            .prepare("SELECT 1 FROM strokes LIMIT 1")?
            .exists([])?;

        if ink && !options.include_original_ink {
            return Err(Error::ReviewRequired);
        }

        let mut snapshot = Snapshot {
            format: "STEM-LIBRARY-1".into(),
            schema: 13,
            tables: BTreeMap::new(),
            omissions: vec![],
        };

        let mut total = 0usize;

        let mut asset_count = 0;

        for &(name, columns) in TABLES {
            let mut q = self.db.prepare(&format!("SELECT * FROM {name}"))?;

            if q.column_count() != columns {
                return Err(Error::Unsupported);
            }

            let mut cursor = q.query([])?;

            let mut rows = Vec::new();

            while let Some(row) = cursor.next()? {
                let mut cells = Vec::new();

                for i in 0..columns {
                    let value = match row.get_ref(i)? {
                        ValueRef::Null => Cell::Null,
                        ValueRef::Integer(n) => Cell::Integer(n),
                        ValueRef::Text(t) => {
                            Cell::Text(std::str::from_utf8(t).map_err(|_| Error::Integrity)?.into())
                        }

                        ValueRef::Blob(b) => Cell::Blob(b.into()),
                        ValueRef::Real(_) => return Err(Error::Unsupported),
                    };

                    cells.push(value);
                }

                if name == "source_assets" {
                    let class: AssetClass = serde_json::from_str(cells[2].text()?)?;

                    if class == AssetClass::LiveCache || !options.include_authorised_source_assets {
                        snapshot.omissions.push("External PDF bytes or disposable source cache excluded; source relationship retained.".into());

                        continue;
                    }

                    let authority: Retention = match &cells[3] {
                        Cell::Text(s) => serde_json::from_str(s)?,
                        _ => return Err(Error::Denied),
                    };

                    if authority.authority.is_empty() || !authority.export_allowed {
                        return Err(Error::Denied);
                    }

                    asset_count += 1;
                }

                if name == "engine_versions" {
                    let mut v: EngineVersion = serde_json::from_str(cells[2].text()?)?;

                    if v.local_resource.take().is_some() {
                        snapshot.omissions.push("Optional model resource excluded; reinstall and verify locally after restore.".into());
                    }

                    cells[2] = Cell::Text(serde_json::to_string(&v)?);
                }

                for value in &cells {
                    total = total
                        .checked_add(match value {
                            Cell::Blob(b) => b.len(),
                            Cell::Text(t) => t.len(),
                            _ => 8,
                        })
                        .ok_or(Error::ResourceLimit)?;
                }

                if total > 64 * 1024 * 1024 {
                    return Err(Error::ResourceLimit);
                }

                rows.push(cells);
            }

            snapshot.tables.insert(name.into(), rows);
        }

        snapshot.omissions.sort();

        snapshot.omissions.dedup();

        if Self::setting(&self.db, "generation")? != generation {
            return Err(Error::Conflict);
        }

        Ok(BackupPlan {
            library,
            generation,
            scopes,
            includes_original_ink: ink,
            source_assets: asset_count,
            omissions: snapshot.omissions.clone(),
            snapshot,
        })
    }

    pub fn finish_backup(
        &self,
        plan: BackupPlan,
        password: &str,
        auth: &mut dyn Authenticator,
        confirmed: bool,
    ) -> Result<crypto::Encrypted> {
        if !confirmed {
            return Err(Error::ReviewRequired);
        }

        if Self::setting(&self.db, "library_id")? != plan.library
            || Self::setting(&self.db, "generation")? != plan.generation
        {
            return Err(Error::Conflict);
        }

        if !plan.scopes.is_empty() && !auth.verify(Id::new(), &plan.scopes) {
            return Err(Error::Denied);
        }

        let bytes = zeroize::Zeroizing::new(serde_json::to_vec(&plan.snapshot)?);

        let encrypted = crypto::encrypt(&bytes, password)?;

        if Self::setting(&self.db, "generation")? != plan.generation {
            return Err(Error::Conflict);
        }

        Ok(encrypted)
    }

    pub fn restore_backup_password(bytes: &[u8], password: &str) -> Result<Self> {
        Self::restore_backup_plain(&crypto::decrypt(
            bytes,
            crypto::Credential::Password(password),
        )?)
    }

    pub fn restore_backup_recovery(bytes: &[u8], key: &str) -> Result<Self> {
        Self::restore_backup_plain(&crypto::decrypt(bytes, crypto::Credential::Recovery(key))?)
    }

    fn restore_backup_plain(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > 64 * 1024 * 1024 {
            return Err(Error::ResourceLimit);
        }

        let mut snapshot: Snapshot = serde_json::from_slice(bytes)?;
        let source_schema = snapshot.schema;
        if snapshot.schema == 8 {
            snapshot.tables.insert("edit_operations".into(), vec![]);
            snapshot.tables.insert("edit_changes".into(), vec![]);
            snapshot
                .tables
                .get_mut("settings")
                .ok_or(Error::Integrity)?
                .push(vec![
                    Cell::Text("edit_operation".into()),
                    Cell::Text(String::new()),
                ]);
            snapshot.schema = 9;
        }

        if snapshot.format != "STEM-LIBRARY-1" || !matches!(snapshot.schema, 9..=13) {
            return Err(Error::Unsupported);
        }

        if source_schema < 12 {
            if snapshot.tables.contains_key("workspace_panes")
                || snapshot.tables.contains_key("workspace_state")
            {
                return Err(Error::Invalid("legacy workspace extension"));
            }
            snapshot.tables.insert("workspace_panes".into(), vec![]);
            snapshot.tables.insert(
                "workspace_state".into(),
                vec![vec![
                    Cell::Integer(1),
                    Cell::Integer(0),
                    Cell::Null,
                    Cell::Text("Horizontal".into()),
                ]],
            );
        }
        if snapshot.tables.len() != TABLES.len()
            || TABLES
                .iter()
                .any(|(table, _)| !snapshot.tables.contains_key(*table))
        {
            return Err(Error::Invalid("backup table set"));
        }

        if snapshot
            .tables
            .get("settings")
            .ok_or(Error::Integrity)?
            .iter()
            .any(|r| {
                r.first().and_then(|c| c.text().ok()) == Some("edit_operation")
                    && r.get(1).and_then(|c| c.text().ok()) != Some("")
            })
        {
            return Err(Error::Integrity);
        }
        let mut destination = Store::memory()?;

        let tx = destination.db.transaction()?;

        tx.execute_batch("PRAGMA defer_foreign_keys=ON;")?;

        tx.execute("DELETE FROM workspace_state", [])?;
        tx.execute("DELETE FROM workspace_panes", [])?;
        tx.execute("DELETE FROM settings", [])?;

        let mut count = 0usize;

        for &(table, width) in TABLES {
            for row in &snapshot.tables[table] {
                count += 1;

                if count > 1_000_000 || row.len() != width {
                    return Err(Error::ResourceLimit);
                }

                let placeholders = (1..=width)
                    .map(|n| format!("?{n}"))
                    .collect::<Vec<_>>()
                    .join(",");

                let values: Vec<_> = row.iter().map(Cell::value).collect();

                tx.execute(
                    &format!("INSERT INTO {table} VALUES({placeholders})"),
                    params_from_iter(values),
                )?;
            }
        }

        tx.commit()?;

        destination.validate_backup_state(if source_schema >= 13 {
            7
        } else if source_schema >= 11 {
            6
        } else if source_schema >= 10 {
            5
        } else {
            4
        })?;

        // Provider grants and in-process adapter readiness are not transferable.
        // Retain relationship/revision metadata without pretending the file is open.
        let tx = destination.db.transaction()?;

        let rows = {
            let mut q = tx.prepare("SELECT payload FROM sources")?;

            let rows = q
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;

            rows
        };

        for row in rows {
            let mut source: Source = serde_json::from_str(&row)?;

            if source.availability != Availability::Revoked {
                source.availability = Availability::Missing;
            }

            Self::save_source(&tx, &source)?;
        }

        let origin = Self::setting(&tx, "library_id")?;

        tx.execute(
            "INSERT OR REPLACE INTO settings VALUES('restore_origin',?1)",
            [origin],
        )?;

        tx.execute(
            "UPDATE settings SET value=?1 WHERE key='library_id'",
            [Id::new().to_string()],
        )?;

        tx.commit()?;

        Ok(destination)
    }

    /// Publish to a NEW path only. Atomic hard-link publication never overwrites
    /// another database; the temporary file is fully synced before it becomes visible.
    pub fn persist_restored_library(
        &self,
        destination: &Path,
        access: &Access,
        auth: &mut dyn Authenticator,
        confirmed: bool,
    ) -> Result<()> {
        if !confirmed {
            return Err(Error::ReviewRequired);
        }

        Self::require_app(&self.db, access)?;

        let generation = Self::setting(&self.db, "generation")?;

        let scopes = self.backup_scopes()?;

        if scopes.iter().any(|s| !access.permits(s))
            || (!scopes.is_empty() && !auth.verify(Id::new(), &scopes))
        {
            return Err(Error::Denied);
        }

        let parent = destination.parent().ok_or(Error::Invalid("restore path"))?;

        let temp = parent.join(format!(".stem-restore-{}.db", Id::new()));

        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;

        drop(file);

        let result = (|| -> Result<()> {
            self.db.execute(
                "VACUUM INTO ?1",
                [temp.to_str().ok_or(Error::Invalid("restore path"))?],
            )?;

            std::fs::File::open(&temp)?.sync_all()?;

            if Self::setting(&self.db, "generation")? != generation {
                return Err(Error::Conflict);
            }

            std::fs::hard_link(&temp, destination)?;

            std::fs::File::open(parent)?.sync_all()?;

            Ok(())
        })();

        let _ = std::fs::remove_file(temp);

        result
    }

    pub fn library_identity(&self, access: &Access) -> Result<String> {
        Self::require_app(&self.db, access)?;

        Self::setting(&self.db, "library_id")
    }

    fn validate_backup_state(&self, native_version: u32) -> Result<()> {
        self.validate_edit_journal()?;
        self.check_integrity()?;
        Self::stored_workspace(&self.db)?;
        let db = &self.db;

        Id::parse(&Self::setting(db, "library_id")?)?;

        for key in ["generation", "active_ms"] {
            if Self::setting(db, key)?
                .parse::<i64>()
                .map_err(|_| Error::Integrity)?
                < 0
            {
                return Err(Error::Integrity);
            }
        }

        let hours = self.history_hours()?;

        if !hours.is_finite() || hours <= 0. || hours > 168. {
            return Err(Error::Integrity);
        }

        if !matches!(Self::setting(db, "app_locked")?.as_str(), "true" | "false") {
            return Err(Error::Integrity);
        }

        let _: crate::store::WritingMode =
            serde_json::from_str(&Self::setting(db, "global_mode")?)?;

        let payloads = |table: &str| -> Result<Vec<(String, String)>> {
            let mut q = db.prepare(&format!("SELECT id,payload FROM {table}"))?;

            let rows = q
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
                .collect::<std::result::Result<Vec<_>, _>>()?;

            Ok(rows)
        };

        let mut doc = NativeDocument {
            version: native_version,
            created_ms: 0,
            entities: vec![],
            blocks: vec![],
            links: vec![],
            omissions: vec![],
            exercises: vec![],
            memberships: vec![],
            headings: vec![],
            rules: vec![],
            modes: vec![],
            attachments: Attachments::default(),
        };

        for (id, json) in payloads("entities")? {
            let mut e: Entity = serde_json::from_str(&json)?;

            let parent: Option<String> =
                db.query_row("SELECT parent FROM entities WHERE id=?1", [&id], |r| {
                    r.get(0)
                })?;

            if e.id.to_string() != id || e.parent.map(|p| p.to_string()) != parent {
                return Err(Error::Integrity);
            }

            e.trashed = false;

            e.purged = false;

            doc.entities.push(e);
        }

        for (id, json) in payloads("blocks")? {
            let b: Block = serde_json::from_str(&json)?;

            let owner: String =
                db.query_row("SELECT entity FROM blocks WHERE id=?1", [&id], |r| r.get(0))?;

            if b.id.to_string() != id || b.entity.to_string() != owner {
                return Err(Error::Integrity);
            }

            let strokes = Self::original_strokes(db, &b)?;

            let mut q = db.prepare(
                "SELECT revision,active_ms,payload FROM history WHERE block=?1 ORDER BY revision",
            )?;

            let mut history = Vec::new();

            for row in q.query_map([&id], |r| {
                Ok((
                    r.get::<_, u64>(0)?,
                    r.get::<_, u64>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })? {
                let (revision, active_ms, json) = row?;

                history.push(HistoryEntry {
                    revision,
                    active_ms,
                    block: serde_json::from_str(&json)?,
                });
            }

            doc.blocks.push(NativeBlock {
                current: b,
                strokes,
                history,
            });
        }

        for (id, json) in payloads("exercises")? {
            let e: Exercise = serde_json::from_str(&json)?;

            let (master, heading): (String, Option<String>) = db.query_row(
                "SELECT master,heading FROM exercises WHERE id=?1",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;

            if e.id.to_string() != id
                || e.master.to_string() != master
                || e.heading.map(|h| h.to_string()) != heading
            {
                return Err(Error::Integrity);
            }

            doc.exercises.push(e);
        }

        let mut q = db.prepare(
            "SELECT m.block,m.exercise,v.revision FROM exercise_members m JOIN \
                membership_versions v ON m.block=v.block",
        )?;

        for row in q.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, u64>(2)?,
            ))
        })? {
            let (b, e, version) = row?;

            doc.memberships.push(NativeMembership {
                block: Id::parse(&b)?,
                exercise: Id::parse(&e)?,
                version,
            });
        }

        if doc.memberships.len()
            != db.query_row("SELECT COUNT(*) FROM exercise_members", [], |r| {
                r.get::<_, usize>(0)
            })?
        {
            return Err(Error::Integrity);
        }

        let mut q = db.prepare("SELECT block FROM confirmed_headings")?;

        for row in q.query_map([], |r| r.get::<_, String>(0))? {
            doc.headings.push(Id::parse(&row?)?);
        }

        let mut q = db.prepare("SELECT master,keyword FROM exercise_rules")?;

        for row in q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
            let (m, keyword) = row?;

            doc.rules.push(NativeRule {
                master: Id::parse(&m)?,
                keyword,
            });
        }

        let mut q = db.prepare("SELECT entity,mode FROM modes")?;

        for row in q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
            let (e, mode) = row?;

            doc.modes.push(NativeMode {
                entity: Id::parse(&e)?,
                mode: serde_json::from_str(&mode)?,
            });
        }

        let mut q = db.prepare("SELECT id,origin,target,anchor FROM links")?;

        for row in q.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })? {
            let (id, o, t, a) = row?;

            doc.links.push(Link {
                id: Id::parse(&id)?,
                origin: Id::parse(&o)?,
                target: Id::parse(&t)?,
                anchor: a.as_deref().map(Id::parse).transpose()?,
            });
        }

        let mut q = db.prepare("SELECT id,entity,mime,bytes,digest FROM image_assets")?;

        for row in q.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Vec<u8>>(3)?,
                r.get::<_, String>(4)?,
            ))
        })? {
            let (id, e, mime, bytes, digest) = row?;

            doc.attachments.assets.push(NativeAsset {
                id: Id::parse(&id)?,
                entity: Id::parse(&e)?,
                mime,
                bytes,
                digest,
            });
        }

        let mut q = db.prepare("SELECT block,payload FROM layouts")?;

        for row in q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
            let (b, json) = row?;

            doc.attachments.layouts.push(NativeLayout {
                block: Id::parse(&b)?,
                layout: serde_json::from_str(&json)?,
            });
        }

        for (id, json) in payloads("object_groups")? {
            let v: ObjectGroup = serde_json::from_str(&json)?;

            let owner: String =
                db.query_row("SELECT entity FROM object_groups WHERE id=?1", [&id], |r| {
                    r.get(0)
                })?;

            if v.id.to_string() != id || v.entity.to_string() != owner {
                return Err(Error::Integrity);
            }

            doc.attachments.groups.push(v);
        }

        for (id, json) in payloads("embeds")? {
            let v: Embed = serde_json::from_str(&json)?;

            let (host, source): (String, String) =
                db.query_row("SELECT host,source FROM embeds WHERE id=?1", [&id], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })?;

            if v.id.to_string() != id
                || v.host.to_string() != host
                || v.source.to_string() != source
            {
                return Err(Error::Integrity);
            }

            doc.attachments.embeds.push(v);
        }

        for (id, json) in payloads("view_settings")? {
            let v: ViewSettings = serde_json::from_str(&json)?;

            let owner: String =
                db.query_row("SELECT entity FROM view_settings WHERE id=?1", [&id], |r| {
                    r.get(0)
                })?;

            if v.id.to_string() != id || v.entity.to_string() != owner {
                return Err(Error::Integrity);
            }

            doc.attachments.views.push(v);
        }

        doc.validate()?;

        for (id, json) in payloads("sources")? {
            let s: Source = serde_json::from_str(&json)?;

            let context: String =
                db.query_row("SELECT context FROM sources WHERE id=?1", [&id], |r| {
                    r.get(0)
                })?;

            if s.id.to_string() != id
                || s.context.to_string() != context
                || s.revision.len() != 64
                || !s.revision.bytes().all(|b| b.is_ascii_hexdigit())
                || (!s.locator.starts_with("content://") && !s.locator.starts_with("file://"))
            {
                return Err(Error::Integrity);
            }

            s.view.validate()?;
        }

        let mut q = db.prepare("SELECT source,class,authority,bytes,digest FROM source_assets")?;

        for row in q.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Vec<u8>>(3)?,
                r.get::<_, String>(4)?,
            ))
        })? {
            let (source, class, authority, bytes, hash) = row?;

            let class: AssetClass = serde_json::from_str(&class)?;

            let retention: Retention = serde_json::from_str(&authority.ok_or(Error::Denied)?)?;

            if class == AssetClass::LiveCache
                || !retention.export_allowed
                || retention.authority.is_empty()
                || digest(&bytes) != hash
            {
                return Err(Error::Integrity);
            }

            Self::raw_source(db, Id::parse(&source)?)?;
        }

        for (id, json) in payloads("pdf_anchors")? {
            let a: PdfAnchor = serde_json::from_str(&json)?;

            let (source, block): (String, String) = db.query_row(
                "SELECT source,block FROM pdf_anchors WHERE id=?1",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;

            if a.id.to_string() != id
                || a.source.to_string() != source
                || a.block.to_string() != block
                || Self::raw_source(db, a.source)?.context != Self::raw_block(db, a.block)?.entity
            {
                return Err(Error::Integrity);
            }

            a.bounds.validate()?;
        }

        for (id, json) in payloads("proposals")? {
            let p: Proposal = serde_json::from_str(&json)?;

            let block: String =
                db.query_row("SELECT block FROM proposals WHERE id=?1", [&id], |r| {
                    r.get(0)
                })?;

            if p.id.to_string() != id
                || p.block.to_string() != block
                || p.output.candidates.is_empty()
                || p.output.candidates.len() > 32
            {
                return Err(Error::Integrity);
            }

            for c in &p.output.candidates {
                c.validate()?;
            }

            if let Some(c) = &p.correction {
                c.validate()?;
            }
        }

        for (id, json) in payloads("membership_operations")? {
            let o: MembershipOperation = serde_json::from_str(&json)?;

            let master: String = db.query_row(
                "SELECT master FROM membership_operations WHERE id=?1",
                [&id],
                |r| r.get(0),
            )?;

            if o.id.to_string() != id || o.master.to_string() != master {
                return Err(Error::Integrity);
            }

            for c in o.changes {
                if Self::raw_block(db, c.block)?.entity != o.master {
                    return Err(Error::Integrity);
                }

                for e in [c.from, c.to].into_iter().flatten() {
                    if Self::raw_exercise(db, e)?.master != o.master {
                        return Err(Error::Integrity);
                    }
                }
            }
        }

        for (id, json) in payloads("engine_versions")? {
            let v: EngineVersion = serde_json::from_str(&json)?;

            if v.id.to_string() != id || v.local_resource.is_some() {
                return Err(Error::Integrity);
            }

            crate::processing::Manager::validate(&v.descriptor)?;
        }

        Ok(())
    }
}

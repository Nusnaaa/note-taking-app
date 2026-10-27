//! Decode/remap/serialization work precedes insert-only atomic publication.
use super::*;
use crate::processing::JobControl;
use rusqlite::{params_from_iter, types::Value};
#[derive(Clone, Copy)]
enum Table {
    Entities,
    Assets,
    Blocks,
    Strokes,
    History,
    Exercises,
    Members,
    MemberVersions,
    Headings,
    Rules,
    Modes,
    Layers,
    Layouts,
    Groups,
    Embeds,
    Views,
    Links,
}
impl Table {
    fn sql(self) -> &'static str {
        match self {
            Self::Entities => "INSERT INTO entities VALUES(?1,?2,?3)",
            Self::Assets => "INSERT INTO image_assets VALUES(?1,?2,?3,?4,?5)",
            Self::Blocks => "INSERT INTO blocks VALUES(?1,?2,?3,?4)",
            Self::Strokes => "INSERT INTO strokes VALUES(?1,?2,?3,?4)",
            Self::History => "INSERT INTO history VALUES(?1,?2,?3,?4)",
            Self::Exercises => "INSERT INTO exercises VALUES(?1,?2,?3,?4)",
            Self::Members => "INSERT INTO exercise_members VALUES(?1,?2)",
            Self::MemberVersions => "INSERT INTO membership_versions VALUES(?1,?2)",
            Self::Headings => "INSERT INTO confirmed_headings VALUES(?1)",
            Self::Rules => "INSERT INTO exercise_rules VALUES(?1,?2)",
            Self::Modes => "INSERT INTO modes VALUES(?1,?2)",
            Self::Layers => "INSERT INTO canvas_layers VALUES(?1,?2)",
            Self::Layouts => "INSERT INTO layouts VALUES(?1,?2)",
            Self::Groups => "INSERT INTO object_groups VALUES(?1,?2,?3)",
            Self::Embeds => "INSERT INTO embeds VALUES(?1,?2,?3,?4)",
            Self::Views => "INSERT INTO view_settings VALUES(?1,?2,?3)",
            Self::Links => "INSERT INTO links VALUES(?1,?2,?3,?4,'{}')",
        }
    }
}
struct Row {
    table: Table,
    values: Vec<Value>,
}
pub const NATIVE_IMPORT_MAX_BYTES: usize =
    64 * 1024 * 1024 + crate::crypto::LEGACY_HEADER_BYTES + 16;
#[derive(Clone, Serialize)]
pub struct NativeImportSummary {
    pub source_version: u32,
    pub entities: usize,
    pub blocks: usize,
    pub strokes: usize,
    pub history_entries: usize,
    pub protected_objects: usize,
    pub retained_titles: usize,
    pub source_omissions: Vec<String>,
}
pub(crate) struct PreparedNativeImport {
    pub(crate) summary: NativeImportSummary,
    rows: Vec<Row>,
    global_ids: BTreeSet<Id>,
    pub(crate) entities: Vec<Id>,
    bytes: usize,
}
impl PreparedNativeImport {
    fn push(&mut self, table: Table, values: Vec<Value>) -> Result<()> {
        let size = values
            .iter()
            .try_fold(0usize, |n, v| {
                n.checked_add(match v {
                    Value::Text(s) => s.len(),
                    Value::Blob(b) => b.len(),
                    _ => 8,
                })
            })
            .ok_or(Error::ResourceLimit)?;
        self.bytes = self.bytes.checked_add(size).ok_or(Error::ResourceLimit)?;
        if self.bytes > export_work::EXPORT_BYTES {
            return Err(Error::ResourceLimit);
        }
        self.rows.push(Row { table, values });
        Ok(())
    }
}
fn id(id: Id) -> Value {
    Value::Text(id.to_string())
}
fn maybe(id: Option<Id>) -> Value {
    id.map_or(Value::Null, self::id)
}
fn json(value: &impl Serialize) -> Result<Value> {
    Ok(Value::Text(
        String::from_utf8(export_work::encode_json(value)?).map_err(|_| Error::Integrity)?,
    ))
}
pub(crate) fn prepare_document(
    mut doc: NativeDocument,
    recovered: bool,
    control: Option<&JobControl>,
) -> Result<PreparedNativeImport> {
    let retained_titles = if recovered {
        doc.entities
            .iter()
            .filter(|e| e.title.len() > 4096 - " (recovered)".len())
            .count()
    } else {
        0
    };
    let check = || control.map_or(Ok(()), |c| c.check());
    check()?;
    doc.validate()?;
    let mut ids = BTreeMap::new();
    for e in &doc.entities {
        check()?;
        ids.insert(e.id, Id::new());
    }
    for b in &doc.blocks {
        check()?;
        ids.insert(b.current.id, Id::new());
        for block in std::iter::once(&b.current).chain(b.history.iter().map(|h| &h.block)) {
            check()?;
            for region in &block.regions {
                check()?;
                ids.entry(region.id).or_default();
            }
            if let crate::content::Content::Table(table) = &block.content {
                for cell in table.rows.iter().flatten() {
                    check()?;
                    ids.entry(cell.id).or_default();
                }
            }
        }
        for s in &b.strokes {
            check()?;
            ids.insert(s.id, Id::new());
        }
    }
    for e in &doc.exercises {
        check()?;
        ids.insert(e.id, Id::new());
    }
    doc.attachments.add_ids(&mut ids);
    for e in &mut doc.entities {
        check()?;
        e.id = ids[&e.id];
        e.parent = e.parent.map(|p| ids[&p]);
        if recovered && e.title.len() <= 4096 - " (recovered)".len() {
            e.title = format!("{} (recovered)", e.title);
        }
    }
    let remap_block = |b: &mut Block| -> Result<()> {
        if let crate::content::Content::Table(table) = &mut b.content {
            for cell in table.rows.iter_mut().flatten() {
                check()?;
                cell.id = ids[&cell.id];
            }
            if let Some(layout) = &mut table.layout {
                for ink in &mut layout.ink_origins {
                    check()?;
                    ink.cell = ids[&ink.cell];
                    ink.stroke = ids[&ink.stroke];
                }
            }
        }
        b.id = ids[&b.id];
        b.entity = ids[&b.entity];
        b.source_ids = b.source_ids.iter().map(|i| ids[i]).collect();
        for region in &mut b.regions {
            check()?;
            region.id = ids[&region.id];
            region.cell = region.cell.map(|cell| ids[&cell]);
            region.strokes = region.strokes.iter().map(|id| ids[id]).collect();
        }
        b.visible_strokes = b.visible_strokes.iter().map(|i| ids[i]).collect();
        if let crate::content::Content::Image(image) = &mut b.content {
            image.asset = ids[&image.asset];
        }
        Ok(())
    };
    for b in &mut doc.blocks {
        check()?;
        remap_block(&mut b.current)?;
        for h in &mut b.history {
            check()?;
            remap_block(&mut h.block)?;
        }
        for s in &mut b.strokes {
            check()?;
            s.id = ids[&s.id];
        }
    }
    for l in &mut doc.links {
        check()?;
        l.id = Id::new();
        l.origin = ids[&l.origin];
        l.target = ids[&l.target];
        l.anchor = l.anchor.map(|a| ids[&a]);
    }
    for e in &mut doc.exercises {
        check()?;
        e.id = ids[&e.id];
        e.master = ids[&e.master];
        e.heading = e.heading.map(|h| ids[&h]);
    }
    for m in &mut doc.memberships {
        check()?;
        m.block = ids[&m.block];
        m.exercise = ids[&m.exercise];
    }
    for h in &mut doc.headings {
        check()?;
        *h = ids[h];
    }
    for r in &mut doc.rules {
        check()?;
        r.master = ids[&r.master];
    }
    for m in &mut doc.modes {
        check()?;
        m.entity = ids[&m.entity];
    }
    doc.attachments.remap(&ids);
    check()?;
    doc.validate()?;
    encode_document(doc, control, retained_titles)
}
fn encode_document(
    doc: NativeDocument,
    control: Option<&JobControl>,
    retained_titles: usize,
) -> Result<PreparedNativeImport> {
    let check = || control.map_or(Ok(()), |c| c.check());
    check()?;
    let mut global_ids = BTreeSet::new();
    for e in &doc.entities {
        global_ids.insert(e.id);
    }
    for b in &doc.blocks {
        global_ids.insert(b.current.id);
        for s in &b.strokes {
            global_ids.insert(s.id);
        }
        for block in std::iter::once(&b.current).chain(b.history.iter().map(|h| &h.block)) {
            for r in &block.regions {
                global_ids.insert(r.id);
            }
        }
    }
    for e in &doc.exercises {
        global_ids.insert(e.id);
    }
    for l in &doc.links {
        global_ids.insert(l.id);
    }
    for a in &doc.attachments.assets {
        global_ids.insert(a.id);
    }
    for g in &doc.attachments.groups {
        global_ids.insert(g.id);
    }
    for e in &doc.attachments.embeds {
        global_ids.insert(e.id);
    }
    for v in &doc.attachments.views {
        global_ids.insert(v.id);
    }
    for state in &doc.attachments.layers {
        for layer in &state.layers {
            global_ids.insert(layer.id);
        }
    }
    let summary = NativeImportSummary {
        source_version: doc.version,
        entities: doc.entities.len(),
        blocks: doc.blocks.len(),
        strokes: doc.blocks.iter().map(|b| b.strokes.len()).sum(),
        history_entries: doc.blocks.iter().map(|b| b.history.len()).sum(),
        protected_objects: doc.entities.iter().filter(|e| e.locked).count()
            + doc.blocks.iter().filter(|b| b.current.locked).count()
            + doc.exercises.iter().filter(|e| e.locked).count(),
        retained_titles,
        source_omissions: doc.omissions.clone(),
    };
    let mut p = PreparedNativeImport {
        summary,
        rows: Vec::new(),
        global_ids,
        entities: doc.entities.iter().map(|e| e.id).collect(),
        bytes: 0,
    };
    for e in &doc.entities {
        check()?;
        p.push(Table::Entities, vec![id(e.id), maybe(e.parent), json(e)?])?;
    }
    for a in &doc.attachments.assets {
        check()?;
        p.push(
            Table::Assets,
            vec![
                id(a.id),
                id(a.entity),
                Value::Text(a.mime.clone()),
                Value::Blob(a.bytes.clone()),
                Value::Text(a.digest.clone()),
            ],
        )?;
    }
    for (ordinal, b) in doc.blocks.iter().enumerate() {
        check()?;
        p.push(
            Table::Blocks,
            vec![
                id(b.current.id),
                id(b.current.entity),
                Value::Integer(ordinal as i64),
                json(&b.current)?,
            ],
        )?;
        for s in &b.strokes {
            check()?;
            let data = serde_json::to_string(s)?;
            let hash = digest(data.as_bytes());
            p.push(
                Table::Strokes,
                vec![
                    id(s.id),
                    id(b.current.id),
                    Value::Text(data),
                    Value::Text(hash),
                ],
            )?;
        }
        let newest = b.history.iter().map(|h| h.active_ms).max().unwrap_or(0);
        for h in &b.history {
            check()?;
            let age = (newest - h.active_ms).min(i64::MAX as u64) as i64;
            p.push(
                Table::History,
                vec![
                    id(b.current.id),
                    Value::Integer(h.revision as i64),
                    Value::Integer(age),
                    json(&h.block)?,
                ],
            )?;
        }
    }
    for e in &doc.exercises {
        check()?;
        p.push(
            Table::Exercises,
            vec![id(e.id), id(e.master), maybe(e.heading), json(e)?],
        )?;
    }
    for m in &doc.memberships {
        check()?;
        p.push(Table::Members, vec![id(m.block), id(m.exercise)])?;
        p.push(
            Table::MemberVersions,
            vec![id(m.block), Value::Integer(m.version as i64)],
        )?;
    }
    for h in &doc.headings {
        check()?;
        p.push(Table::Headings, vec![id(*h)])?;
    }
    for r in &doc.rules {
        check()?;
        p.push(
            Table::Rules,
            vec![id(r.master), Value::Text(r.keyword.clone())],
        )?;
    }
    for m in &doc.modes {
        check()?;
        p.push(Table::Modes, vec![id(m.entity), json(&m.mode)?])?;
    }
    for s in &doc.attachments.layers {
        check()?;
        p.push(Table::Layers, vec![id(s.canvas), json(s)?])?;
    }
    for l in &doc.attachments.layouts {
        check()?;
        p.push(Table::Layouts, vec![id(l.block), json(&l.layout)?])?;
    }
    for g in &doc.attachments.groups {
        check()?;
        p.push(Table::Groups, vec![id(g.id), id(g.entity), json(g)?])?;
    }
    for e in &doc.attachments.embeds {
        check()?;
        p.push(
            Table::Embeds,
            vec![id(e.id), id(e.host), id(e.source), json(e)?],
        )?;
    }
    for v in &doc.attachments.views {
        check()?;
        p.push(Table::Views, vec![id(v.id), id(v.entity), json(v)?])?;
    }
    for l in &doc.links {
        check()?;
        p.push(
            Table::Links,
            vec![id(l.id), id(l.origin), id(l.target), maybe(l.anchor)],
        )?;
    }
    check()?;
    Ok(p)
}
impl Store {
    pub(crate) fn publish_prepared_native(
        &mut self,
        prepared: PreparedNativeImport,
        access: &Access,
        copy_guard: Option<(&str, &BTreeSet<Scope>)>,
        operation: Option<(Id, &str)>,
        control: Option<&JobControl>,
    ) -> Result<NativeImportResult> {
        let check = || control.map_or(Ok(()), |c| c.check());
        check()?;
        if let Some((operation, digest)) = operation {
            if let Some(receipt) = self.native_import_receipt(operation, access)? {
                if receipt.artifact_digest != digest {
                    return Err(Error::Conflict);
                }
                return Ok(NativeImportResult {
                    entities: receipt.entities.clone(),
                    receipt: Some(receipt),
                });
            }
        }
        self.write_checked(
            |db| {
                Self::require_app(db, access)?;
                if let Some((expected, scopes)) = copy_guard {
                    if Self::setting(db, "generation")? != expected {
                        return Err(Error::Conflict);
                    }
                    if scopes.iter().any(|s| !access.permits(s)) {
                        return Err(Error::Denied);
                    }
                }
                db.execute_batch("PRAGMA defer_foreign_keys=ON;")?;
                reject_existing_ids(db, &prepared.global_ids, control)?;
                let tick: i64 = Self::setting(db, "active_ms")?
                    .parse()
                    .map_err(|_| Error::Integrity)?;
                for mut row in prepared.rows {
                    check()?;
                    if let Table::History = row.table {
                        let Value::Integer(age) = row.values[2] else {
                            return Err(Error::Integrity);
                        };
                        row.values[2] = Value::Integer(tick.saturating_sub(age).max(0));
                    }
                    db.execute(row.table.sql(), params_from_iter(row.values))?;
                }
                check()?;
                let receipt = if let Some((operation, artifact_digest)) = operation {
                    let receipt = NativeImportReceipt {
                        operation,
                        artifact_digest: artifact_digest.into(),
                        entities: prepared.entities.clone(),
                        completed_ms: now_ms(),
                    };
                    db.execute(
                        "INSERT INTO native_imports VALUES(?1,?2,?3)",
                        rusqlite::params![
                            operation.to_string(),
                            artifact_digest,
                            serde_json::to_string(&receipt)?
                        ],
                    )?;
                    Some(receipt)
                } else {
                    None
                };
                Ok(NativeImportResult {
                    entities: prepared.entities,
                    receipt,
                })
            },
            check,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeImportReceipt {
    pub operation: Id,
    pub artifact_digest: String,
    pub entities: Vec<Id>,
    pub completed_ms: i64,
}
impl NativeImportReceipt {
    fn validate(&self, operation: &str, digest: &str) -> Result<()> {
        if self.operation.to_string() != operation
            || self.artifact_digest != digest
            || digest.len() != 64
            || !digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || self.completed_ms < 0
            || self.entities.is_empty()
            || self.entities.len() > 10000
            || self.entities.iter().collect::<BTreeSet<_>>().len() != self.entities.len()
        {
            return Err(Error::Integrity);
        }
        Ok(())
    }
}
impl Store {
    pub fn native_import_receipt(
        &self,
        operation: Id,
        access: &Access,
    ) -> Result<Option<NativeImportReceipt>> {
        Self::require_app(&self.db, access)?;
        use rusqlite::OptionalExtension;
        let row: Option<(String, String)> = self
            .db
            .query_row(
                "SELECT digest,payload FROM native_imports WHERE operation=?1",
                [operation.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        row.map(|(digest, payload)| {
            let receipt: NativeImportReceipt = serde_json::from_str(&payload)?;
            receipt.validate(&operation.to_string(), &digest)?;
            Ok(receipt)
        })
        .transpose()
    }
    pub(crate) fn validate_native_import_receipts(
        &self,
        control: Option<&JobControl>,
    ) -> Result<()> {
        let mut statement = self
            .db
            .prepare("SELECT operation,digest,payload FROM native_imports ORDER BY operation")?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            if let Some(c) = control {
                c.check()?;
            }
            let operation: String = row.get(0)?;
            let digest: String = row.get(1)?;
            let payload: String = row.get(2)?;
            let receipt: NativeImportReceipt = serde_json::from_str(&payload)?;
            receipt.validate(&operation, &digest)?;
            for entity in receipt.entities {
                if let Some(c) = control {
                    c.check()?;
                }
                if !self
                    .db
                    .prepare("SELECT 1 FROM entities WHERE id=?1")?
                    .exists([entity.to_string()])?
                {
                    return Err(Error::Integrity);
                }
            }
        }
        Ok(())
    }
}

pub(crate) struct NativeImportResult {
    pub(crate) entities: Vec<Id>,
    pub(crate) receipt: Option<NativeImportReceipt>,
}
fn reject_existing_ids(
    db: &rusqlite::Connection,
    ids: &BTreeSet<Id>,
    control: Option<&JobControl>,
) -> Result<()> {
    let check = || control.map_or(Ok(()), |c| c.check());
    check()?;
    db.execute_batch("CREATE TEMP TABLE native_import_fresh_ids(id TEXT PRIMARY KEY) STRICT;")?;
    {
        let mut statement = db.prepare("INSERT INTO native_import_fresh_ids VALUES(?1)")?;
        for id in ids {
            check()?;
            statement.execute([id.to_string()])?;
        }
    }
    for (table, column) in [
        ("entities", "id"),
        ("blocks", "id"),
        ("strokes", "id"),
        ("exercises", "id"),
        ("links", "id"),
        ("image_assets", "id"),
        ("object_groups", "id"),
        ("embeds", "id"),
        ("view_settings", "id"),
        ("sources", "id"),
        ("source_assets", "id"),
        ("proposals", "id"),
        ("pdf_anchors", "id"),
        ("engine_versions", "id"),
        ("workspace_panes", "slot"),
    ] {
        check()?;
        if db.prepare(&format!("SELECT 1 FROM {table} existing JOIN native_import_fresh_ids fresh ON fresh.id=existing.{column} LIMIT 1"))?.exists([])? {return Err(Error::Conflict);}
    }
    // Regions are global identities; table-local cell keys intentionally are not.
    for sql in [
        "SELECT 1 FROM blocks b,json_each(b.payload,'$.regions') r JOIN native_import_fresh_ids f ON f.id=CASE WHEN json_valid(r.value) THEN json_extract(r.value,'$.id') END LIMIT 1",
        "SELECT 1 FROM history h,json_each(h.payload,'$.regions') r JOIN native_import_fresh_ids f ON f.id=CASE WHEN json_valid(r.value) THEN json_extract(r.value,'$.id') END LIMIT 1",
        "SELECT 1 FROM canvas_layers c,json_each(c.payload,'$.layers') l JOIN native_import_fresh_ids f ON f.id=CASE WHEN json_valid(l.value) THEN json_extract(l.value,'$.id') END LIMIT 1",
    ] {check()?;if db.prepare(sql)?.exists([])? {return Err(Error::Conflict);}}
    db.execute_batch("DROP TABLE native_import_fresh_ids;")?;
    check()
}

pub(crate) fn prepare_bytes(
    bytes: &[u8],
    credential: Option<crypto::Credential<'_>>,
    control: &JobControl,
) -> Result<PreparedNativeImport> {
    control.check()?;
    if bytes.len() > NATIVE_IMPORT_MAX_BYTES {
        return Err(Error::ResourceLimit);
    }
    let plain = match credential {
        Some(credential) => crypto::decrypt(bytes, credential)?,
        None => zeroize::Zeroizing::new(bytes.to_vec()),
    };
    control.check()?;
    let doc = NativeDocument::decode(&plain)?;
    control.check()?;
    if doc.entities.is_empty() {
        return Err(Error::Invalid("empty native import"));
    }
    prepare_document(doc, true, Some(control))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        backup::BackupOptions,
        content::Content,
        crypto,
        store::{FailurePoint, Kind},
    };
    struct Auth;
    impl Authenticator for Auth {
        fn verify(&mut self, _: Id, _: &BTreeSet<Scope>) -> bool {
            true
        }
    }
    fn document(title: &str) -> NativeDocument {
        let mut s = Store::memory().unwrap();
        let a = Access::public();
        let note = s.create(Kind::Note, None, title, &a).unwrap();
        s.append_content(note.id, Content::Text("001 + 1 = 3".into()), &a)
            .unwrap();
        s.prepare_native_export(&[note.id], true, &a)
            .unwrap()
            .document
    }
    #[test]
    fn prepared_import_cannot_upsert_existing_identity_and_failed_probe_leaves_no_temp_state() {
        let mut s = Store::memory().unwrap();
        let a = Access::public();
        let note = s.create(Kind::Note, None, "kept", &a).unwrap();
        let mut collision = document("replacement");
        let old = collision.entities[0].id;
        collision.entities[0].id = note.id;
        for b in &mut collision.blocks {
            if b.current.entity == old {
                b.current.entity = note.id;
            }
            for h in &mut b.history {
                h.block.entity = note.id;
            }
        }
        collision.modes[0].entity = note.id;
        collision.validate().unwrap();
        let p = encode_document(collision, None, 0).unwrap();
        assert!(matches!(
            s.publish_prepared_native(p, &a, None, None, None),
            Err(Error::Conflict)
        ));
        assert_eq!(s.entity(note.id, &a).unwrap().title, "kept");
        let result = s
            .publish_prepared_native(
                prepare_document(document("new"), true, None).unwrap(),
                &a,
                None,
                None,
                None,
            )
            .unwrap();
        assert_eq!(result.entities.len(), 1);
    }
    #[test]
    fn receipt_and_import_rows_rollback_together_and_replay_never_duplicates_or_advances_generation(
    ) {
        let mut s = Store::memory().unwrap();
        let a = Access::public();
        let doc = document("new");
        let operation = Id::new();
        let hash = "a".repeat(64);
        s.inject_failure(FailurePoint::BeforeCommit);
        assert!(matches!(
            s.publish_prepared_native(
                prepare_document(doc.clone(), true, None).unwrap(),
                &a,
                None,
                Some((operation, &hash)),
                None
            ),
            Err(Error::Interrupted)
        ));
        assert!(s.native_import_receipt(operation, &a).unwrap().is_none());
        assert_eq!(
            s.db.query_row("SELECT count(*) FROM entities", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        let first = s
            .publish_prepared_native(
                prepare_document(doc.clone(), true, None).unwrap(),
                &a,
                None,
                Some((operation, &hash)),
                None,
            )
            .unwrap();
        assert_eq!(first.receipt.as_ref().unwrap().entities, first.entities);
        let generation = Store::setting(&s.db, "generation").unwrap();
        let replay = s
            .publish_prepared_native(
                prepare_document(doc.clone(), true, None).unwrap(),
                &a,
                None,
                Some((operation, &hash)),
                None,
            )
            .unwrap();
        assert_eq!(replay.entities, first.entities);
        assert_eq!(Store::setting(&s.db, "generation").unwrap(), generation);
        assert!(matches!(
            s.publish_prepared_native(
                prepare_document(doc, true, None).unwrap(),
                &a,
                None,
                Some((operation, &"b".repeat(64))),
                None
            ),
            Err(Error::Conflict)
        ));
    }
    #[test]
    fn checked_commit_cancellation_rolls_back_rows_and_generation() {
        let mut s = Store::memory().unwrap();
        let before = Store::setting(&s.db, "generation").unwrap();
        let c = JobControl::new(std::time::Duration::from_secs(30)).unwrap();
        let result = s.write_checked(
            |db| {
                db.execute(
                    "INSERT INTO settings VALUES('synthetic-import-check','value')",
                    [],
                )?;
                Ok(())
            },
            || {
                c.cancel();
                c.check()
            },
        );
        assert!(matches!(result, Err(Error::Cancelled)));
        assert!(!s
            .db
            .prepare("SELECT 1 FROM settings WHERE key='synthetic-import-check'")
            .unwrap()
            .exists([])
            .unwrap());
        assert_eq!(Store::setting(&s.db, "generation").unwrap(), before);
    }
    #[test]
    fn valid_maximum_title_is_retained_and_imported_protection_never_grants_access() {
        let mut s = Store::memory().unwrap();
        let a = Access::public();
        let title = "x".repeat(4096);
        let mut doc = document(&title);
        doc.entities[0].locked = true;
        let ids = s
            .publish_prepared_native(
                prepare_document(doc, true, None).unwrap(),
                &a,
                None,
                None,
                None,
            )
            .unwrap()
            .entities;
        assert!(matches!(s.entity(ids[0], &a), Err(Error::Denied)));
        let key =
            Access::authenticate(&mut Auth, [Scope::Entity(ids[0])].into_iter().collect()).unwrap();
        assert_eq!(s.entity(ids[0], &key).unwrap().title, title);
    }
    #[test]
    fn native_export_keeps_the_existing_format_capacity_above_ten_thousand_blocks() {
        let mut s = Store::memory().unwrap();
        let a = Access::public();
        let note = s.create(Kind::Note, None, "many", &a).unwrap();
        let prototype = s
            .append_content(note.id, Content::Text(String::new()), &a)
            .unwrap();
        {
            let tx = s.db.transaction().unwrap();
            let mut insert = tx
                .prepare("INSERT INTO blocks VALUES(?1,?2,?3,?4)")
                .unwrap();
            for ordinal in 1..10001 {
                let mut b = prototype.clone();
                b.id = Id::new();
                insert
                    .execute(rusqlite::params![
                        b.id.to_string(),
                        note.id.to_string(),
                        ordinal,
                        serde_json::to_string(&b).unwrap()
                    ])
                    .unwrap();
            }
            drop(insert);
            tx.commit().unwrap();
        }
        let p = s.prepare_native_export(&[note.id], false, &a).unwrap();
        assert_eq!(p.selected_blocks, 10001);
        let bytes = s
            .finish_native_export(
                p,
                ExportProtection::Plaintext {
                    acknowledge_protected: false,
                },
                &mut Auth,
                true,
            )
            .unwrap()
            .bytes;
        assert_eq!(NativeDocument::decode(&bytes).unwrap().blocks.len(), 10001);
    }
    #[test]
    fn schema_22_migration_preserves_existing_content_without_inventing_receipts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        let a = Access::public();
        let mut s = Store::open(&path).unwrap();
        let entity = s.create(Kind::Note, None, "before migration", &a).unwrap();
        let block = s
            .append_content(entity.id, Content::Text("001 + 1 = 3".into()), &a)
            .unwrap();
        let generation = Store::setting(&s.db, "generation").unwrap();
        s.db.execute_batch("DROP TABLE native_imports; PRAGMA user_version=22;")
            .unwrap();
        drop(s);
        let migrated = Store::open(&path).unwrap();
        assert_eq!(migrated.schema_version().unwrap(), 23);
        assert_eq!(
            Store::setting(&migrated.db, "generation").unwrap(),
            generation
        );
        assert_eq!(
            migrated.block(block.id, &a).unwrap().content,
            Content::Text("001 + 1 = 3".into())
        );
        assert!(migrated
            .native_import_receipt(Id::new(), &a)
            .unwrap()
            .is_none());
        migrated.check_integrity().unwrap();
    }

    #[test]
    fn streaming_backup_retains_committed_import_receipt() {
        let mut s = Store::memory().unwrap();
        let a = Access::public();
        let operation = Id::new();
        let committed = s
            .publish_prepared_native(
                prepare_document(document("streamed copy"), true, None).unwrap(),
                &a,
                None,
                Some((operation, &"b".repeat(64))),
                None,
            )
            .unwrap()
            .receipt
            .unwrap();
        let control = JobControl::new(std::time::Duration::from_secs(180)).unwrap();
        let plan = s
            .prepare_stream_backup(
                BackupOptions {
                    include_original_ink: true,
                    include_authorised_source_assets: false,
                },
                &a,
            )
            .unwrap();
        let password = "Native import streaming receipt backup passphrase";
        let (bytes, key) = s
            .finish_stream_backup(plan, password, &mut Auth, true, Vec::new(), &control)
            .unwrap();
        let mut builder = tempfile::Builder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            builder.permissions(std::fs::Permissions::from_mode(0o700));
        }
        let dir = builder.tempdir().unwrap();
        let restored = Store::restore_stream_backup(
            bytes.as_slice(),
            crypto::Credential::Recovery(&key),
            16 * 1024 * 1024,
            dir.path(),
            &control,
        )
        .unwrap();
        assert_eq!(
            restored
                .store()
                .native_import_receipt(operation, &a)
                .unwrap(),
            Some(committed)
        );
        // A lower schema header must not silently discard committed receipts.
        use std::io::{Read, Write};
        let mut reader = crypto::stream::DecryptStream::new(
            bytes.as_slice(),
            crypto::Credential::Password(password),
            16 * 1024 * 1024,
        )
        .unwrap();
        let mut plain = Vec::new();
        reader.read_to_end(&mut plain).unwrap();
        reader.finish().unwrap();
        plain[8..12].copy_from_slice(&22u32.to_be_bytes());
        let mut writer = crypto::stream::EncryptStream::new(Vec::new(), password).unwrap();
        writer.write_all(&plain).unwrap();
        let (disguised, _) = writer.finish().unwrap();
        assert!(matches!(
            Store::restore_stream_backup(
                disguised.as_slice(),
                crypto::Credential::Password(password),
                16 * 1024 * 1024,
                dir.path(),
                &control
            ),
            Err(Error::Unsupported)
        ));
    }

    #[test]
    fn import_receipts_survive_backup_and_old_schema_cannot_hide_nonempty_receipts() {
        let mut s = Store::memory().unwrap();
        let a = Access::public();
        let operation = Id::new();
        let hash = "a".repeat(64);
        let committed = s
            .publish_prepared_native(
                prepare_document(document("new"), true, None).unwrap(),
                &a,
                None,
                Some((operation, &hash)),
                None,
            )
            .unwrap()
            .receipt
            .unwrap();
        let password = "Native import receipt backup test passphrase";
        let bytes = s
            .finish_backup(
                s.prepare_backup(
                    BackupOptions {
                        include_original_ink: true,
                        include_authorised_source_assets: false,
                    },
                    &a,
                )
                .unwrap(),
                password,
                &mut Auth,
                true,
            )
            .unwrap()
            .bytes;
        let restored = Store::restore_backup_password(&bytes, password).unwrap();
        assert_eq!(
            restored.native_import_receipt(operation, &a).unwrap(),
            Some(committed)
        );
        let plain = crypto::decrypt(&bytes, crypto::Credential::Password(password)).unwrap();
        let mut json: serde_json::Value = serde_json::from_slice(&plain).unwrap();
        json["schema"] = 22.into();
        let forged = crypto::encrypt(&serde_json::to_vec(&json).unwrap(), password).unwrap();
        assert!(matches!(
            Store::restore_backup_password(&forged.bytes, password),
            Err(Error::Unsupported)
        ));
    }
}

//! Native JSON v7 (with v1-v6 read compatibility) for the implemented core subset.
//! It is NOT yet the complete v0.5.8 interchange contract (see status).
use crate::{
    crypto, digest,
    ink::Stroke,
    now_ms,
    security::{Access, Authenticator, Scope},
    store::{Block, Entity, Exercise, HistoryEntry, Link, Store, WritingMode},
    Error, Id, Result,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeBlock {
    pub current: Block,
    pub strokes: Vec<Stroke>,
    pub history: Vec<HistoryEntry>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeDocument {
    #[serde(default)]
    pub attachments: crate::attachments::Attachments,
    pub version: u32,
    pub created_ms: i64,
    pub entities: Vec<Entity>,
    pub blocks: Vec<NativeBlock>,
    pub links: Vec<Link>,
    pub omissions: Vec<String>,
    #[serde(default)]
    pub exercises: Vec<Exercise>,
    #[serde(default)]
    pub memberships: Vec<NativeMembership>,
    #[serde(default)]
    pub headings: Vec<Id>,
    #[serde(default)]
    pub rules: Vec<NativeRule>,
    #[serde(default)]
    pub modes: Vec<NativeMode>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeMembership {
    pub block: Id,
    pub exercise: Id,
    pub version: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeRule {
    pub master: Id,
    pub keyword: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeMode {
    pub entity: Id,
    pub mode: WritingMode,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Checked {
    payload: String,
    sha256: String,
}

pub struct ExportPlan {
    library: String,
    generation: String,
    document: NativeDocument,
    scopes: BTreeSet<Scope>,
    pub selected_entities: usize,
    pub selected_blocks: usize,
    pub includes_raw_ink: bool,
    pub omissions: Vec<String>,
}
pub enum ExportProtection<'a> {
    Encrypted { password: &'a str },
    Plaintext { acknowledge_protected: bool },
}
pub struct Exported {
    pub bytes: Vec<u8>,
    pub recovery_key: Option<zeroize::Zeroizing<String>>,
}

impl NativeDocument {
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > 64 * 1024 * 1024 {
            return Err(Error::ResourceLimit);
        }
        let checked: Checked = serde_json::from_slice(bytes)?;
        if digest(checked.payload.as_bytes()) != checked.sha256 {
            return Err(Error::Integrity);
        }
        let doc: Self = serde_json::from_str(&checked.payload)?;
        doc.validate()?;
        Ok(doc)
    }
    fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let payload = serde_json::to_string(self)?;
        Ok(serde_json::to_vec(&Checked {
            sha256: digest(payload.as_bytes()),
            payload,
        })?)
    }
    pub fn validate(&self) -> Result<()> {
        if !matches!(self.version, 1..=7) {
            return Err(Error::Unsupported);
        }
        if self.version < 4
            && self.blocks.iter().any(|b| {
                !b.current.regions.is_empty()
                    || b.history.iter().any(|h| !h.block.regions.is_empty())
            })
        {
            return Err(Error::Unsupported);
        }
        if self.version < 3 && self.attachments != crate::attachments::Attachments::default() {
            return Err(Error::Invalid("native v3 extensions"));
        }
        if self.version < 6 && self.attachments.views.iter().any(|v| v.bookmark.is_some()) {
            return Err(Error::Unsupported);
        }
        self.attachments.validate(self)?;
        if self.version == 1
            && (!self.exercises.is_empty()
                || !self.memberships.is_empty()
                || !self.headings.is_empty()
                || !self.rules.is_empty()
                || !self.modes.is_empty())
        {
            return Err(Error::Invalid("v1 extension"));
        }
        if self.entities.len() > 10_000 || self.blocks.len() > 100_000 {
            return Err(Error::ResourceLimit);
        }
        let mut entities = BTreeMap::new();
        // Imports remap all durable identifiers through one table. Reject cross-kind
        // aliases before remapping, as well as duplicates inside each collection.
        let mut durable_ids = BTreeSet::new();
        for e in &self.entities {
            if e.purged
                || e.trashed
                || e.title.len() > 4096
                || e.revision > i64::MAX as u64
                || !durable_ids.insert(e.id)
                || entities.insert(e.id, e).is_some()
            {
                return Err(Error::Invalid("native entity"));
            }
        }
        for e in &self.entities {
            let mut cursor = e.parent;
            let mut visited = BTreeSet::from([e.id]);
            let mut depth = usize::from(e.kind == crate::store::Kind::Module);
            if let Some(p) = cursor {
                let parent = entities.get(&p).ok_or(Error::Invalid("native parent"))?;
                if !parent.kind.accepts(e.kind) {
                    return Err(Error::Invalid("native parent kind"));
                }
            }
            while let Some(p) = cursor {
                if !visited.insert(p) {
                    return Err(Error::Invalid("native cycle"));
                }
                let parent = entities.get(&p).ok_or(Error::Invalid("native ancestor"))?;
                depth += usize::from(parent.kind == crate::store::Kind::Module);
                cursor = parent.parent;
            }
            if depth > 2 {
                return Err(Error::Invalid("native Module depth"));
            }
        }
        let mut block_ids = BTreeSet::new();
        let mut stroke_ids = BTreeSet::new();
        for n in &self.blocks {
            let b = &n.current;
            b.validate()?;
            if b.revision > i64::MAX as u64
                || !durable_ids.insert(b.id)
                || !block_ids.insert(b.id)
                || !entities.get(&b.entity).is_some_and(|e| e.kind.is_surface())
            {
                return Err(Error::Invalid("native block"));
            }
            let ids = n.strokes.iter().map(|s| s.id).collect::<Vec<_>>();
            if ids != b.source_ids {
                return Err(Error::Invalid("native ink membership"));
            }
            for s in &n.strokes {
                s.validate_persisted()?;
                if self.version < 7 && s.style.is_some() {
                    return Err(Error::Unsupported);
                }
                if self.version < 5 && !s.end.is_completed() {
                    return Err(Error::Unsupported);
                }
                if !stroke_ids.insert(s.id) || !durable_ids.insert(s.id) {
                    return Err(Error::Invalid("native duplicate stroke"));
                }
            }
            let mut revisions = BTreeSet::new();
            for h in &n.history {
                h.block.validate()?;
                if h.block.id != b.id
                    || h.block.entity != b.entity
                    || h.revision != h.block.revision
                    || h.revision > b.revision
                    || h.active_ms > i64::MAX as u64
                    || !revisions.insert(h.revision)
                    || h.block.source_ids.iter().any(|i| !b.source_ids.contains(i))
                {
                    return Err(Error::Invalid("native history"));
                }
            }
        }
        let mut links = BTreeSet::new();
        let block_map: BTreeMap<_, _> = self
            .blocks
            .iter()
            .map(|b| (b.current.id, &b.current))
            .collect();
        let mut exercise_map = BTreeMap::new();
        let mut exercise_headings = BTreeSet::new();
        if self.exercises.len() > 10000 || self.memberships.len() > self.blocks.len() {
            return Err(Error::ResourceLimit);
        }
        for e in &self.exercises {
            if !durable_ids.insert(e.id)
                || exercise_map.insert(e.id, e).is_some()
                || !entities
                    .get(&e.master)
                    .is_some_and(|m| m.kind == crate::store::Kind::Note)
                || e.title.is_empty()
                || e.title.len() > 4096
                || e.revision > i64::MAX as u64
            {
                return Err(Error::Invalid("native exercise"));
            }
            if let Some(h) = e.heading {
                if !exercise_headings.insert(h)
                    || block_map.get(&h).is_none_or(|b| b.entity != e.master)
                {
                    return Err(Error::Invalid("native exercise heading"));
                }
            }
        }
        let mut members = BTreeSet::new();
        for m in &self.memberships {
            if !members.insert(m.block)
                || m.version == 0
                || m.version > i64::MAX as u64
                || block_map
                    .get(&m.block)
                    .zip(exercise_map.get(&m.exercise))
                    .is_none_or(|(b, e)| b.entity != e.master)
            {
                return Err(Error::Invalid("native membership"));
            }
        }
        let mut headings = BTreeSet::new();
        for h in &self.headings {
            if !headings.insert(h) || !block_map.contains_key(h) {
                return Err(Error::Invalid("native confirmed heading"));
            }
        }
        let mut rules = BTreeSet::new();
        for r in &self.rules {
            if !rules.insert(r.master)
                || !entities.contains_key(&r.master)
                || r.keyword.trim().is_empty()
                || r.keyword.len() > 256
            {
                return Err(Error::Invalid("native exercise rule"));
            }
        }
        let mut modes = BTreeSet::new();
        for m in &self.modes {
            if !modes.insert(m.entity) || !entities.contains_key(&m.entity) {
                return Err(Error::Invalid("native mode"));
            }
        }
        for l in &self.links {
            if !links.insert(l.id)
                || !durable_ids.insert(l.id)
                || !entities.contains_key(&l.origin)
                || !entities.contains_key(&l.target)
                || l.anchor.is_some_and(|a| {
                    !self
                        .blocks
                        .iter()
                        .any(|b| b.current.id == a && b.current.entity == l.origin)
                })
            {
                return Err(Error::Invalid("native link"));
            }
        }
        durable_ids.extend(self.attachments.assets.iter().map(|a| a.id));
        durable_ids.extend(self.attachments.groups.iter().map(|g| g.id));
        durable_ids.extend(self.attachments.embeds.iter().map(|e| e.id));
        durable_ids.extend(self.attachments.views.iter().map(|v| v.id));
        let mut region_owners = BTreeMap::new();
        for b in &self.blocks {
            for block in std::iter::once(&b.current).chain(b.history.iter().map(|h| &h.block)) {
                for r in &block.regions {
                    if durable_ids.contains(&r.id)
                        || region_owners
                            .insert(r.id, b.current.id)
                            .is_some_and(|owner| owner != b.current.id)
                    {
                        return Err(Error::Invalid("region identity"));
                    }
                }
            }
        }
        Ok(())
    }
}

impl Store {
    pub(crate) fn protection_scopes(
        &self,
        entity: Id,
        block: Option<Id>,
    ) -> Result<BTreeSet<Scope>> {
        let mut scopes = BTreeSet::new();
        if Self::setting(&self.db, "app_locked")? == "true" {
            scopes.insert(Scope::App);
        }
        let mut cursor = Some(entity);
        while let Some(id) = cursor {
            let e = Self::raw_entity(&self.db, id)?;
            if e.locked {
                scopes.insert(Scope::Entity(id));
            }
            cursor = e.parent;
        }
        if let Some(id) = block {
            if Self::raw_block(&self.db, id)?.locked {
                scopes.insert(Scope::Block(id));
            }
            if let Some(exercise) = Self::block_exercise(&self.db, id)? {
                if Self::raw_exercise(&self.db, exercise)?.locked {
                    scopes.insert(Scope::Exercise(exercise));
                }
            }
        }
        Ok(scopes)
    }
    /// Explicit exact-entity selection: no implicit descendant or linked-source export.
    /// Lossless original ink/history require affirmative inclusion. Unsupported data
    /// are refused, not silently exported as a supposedly complete native document.
    pub fn prepare_native_export(
        &self,
        ids: &[Id],
        include_original_ink: bool,
        access: &Access,
    ) -> Result<ExportPlan> {
        self.prepare_native_selection(ids, None, include_original_ink, access)
    }
    /// Selecting a linked exercise never adds unselected master blocks to output.
    pub fn prepare_exercise_export(
        &self,
        exercise: Id,
        include_original_ink: bool,
        access: &Access,
    ) -> Result<ExportPlan> {
        let e = self.exercise(exercise, access)?;
        if !e.visible {
            return Err(Error::Unavailable);
        }
        self.prepare_native_selection(&[e.master], Some(e.id), include_original_ink, access)
    }
    fn prepare_native_selection(
        &self,
        ids: &[Id],
        exercise: Option<Id>,
        include_original_ink: bool,
        access: &Access,
    ) -> Result<ExportPlan> {
        let generation = Self::setting(&self.db, "generation")?;
        if ids.is_empty() {
            return Err(Error::Invalid("export scope"));
        }
        let selected: BTreeSet<_> = ids.iter().copied().collect();
        if selected.len() != ids.len() {
            return Err(Error::Invalid("duplicate export scope"));
        }
        let mut entities = Vec::new();
        let mut blocks = Vec::new();
        let mut links = Vec::new();
        let mut omissions = Vec::new();
        let mut scopes = BTreeSet::new();
        let mut exercises = Vec::new();
        let mut memberships = Vec::new();
        let mut headings = Vec::new();
        let mut rules = Vec::new();
        let mut modes = Vec::new();
        let mut has_ink = false;
        for &id in ids {
            let mut e = Self::authorise(&self.db, id, access, false)?;
            scopes.extend(self.protection_scopes(id, None)?);
            modes.push(NativeMode {
                entity: id,
                mode: self.mode(id, access)?,
            });
            if let Some(view) = exercise {
                let view = Self::read_exercise(&self.db, view, access)?;
                if !view.visible {
                    return Err(Error::Unavailable);
                }
                e.title = view.title;
                if view.locked {
                    scopes.insert(Scope::Exercise(view.id));
                    e.locked = true;
                }
            } else {
                let mut query = self
                    .db
                    .prepare("SELECT id FROM exercises WHERE master=?1")?;
                for row in query.query_map([id.to_string()], |r| r.get::<_, String>(0))? {
                    let view = Self::read_exercise(&self.db, Id::parse(&row?)?, access)?;
                    if view.locked {
                        scopes.insert(Scope::Exercise(view.id));
                    }
                    exercises.push(view);
                }
                use rusqlite::OptionalExtension;
                let keyword: Option<String> = self
                    .db
                    .query_row(
                        "SELECT keyword FROM exercise_rules WHERE master=?1",
                        [id.to_string()],
                        |r| r.get(0),
                    )
                    .optional()?;
                if let Some(keyword) = keyword {
                    rules.push(NativeRule {
                        master: id,
                        keyword,
                    });
                }
            }
            if self
                .db
                .prepare("SELECT 1 FROM membership_operations WHERE master=?1")?
                .exists([id.to_string()])?
            {
                omissions.push(
                    "Exercise reassignment undo history is not included in native subset v2."
                        .into(),
                );
            }
            // Refuse unsupported source packaging instead of silently losing PDF state.
            if self
                .db
                .prepare("SELECT 1 FROM sources WHERE context=?1")?
                .exists([id.to_string()])?
            {
                return Err(Error::Unsupported);
            }
            if e.parent.is_some_and(|p| !selected.contains(&p)) {
                e.parent = None;
                omissions.push("Unselected parent omitted; selected entity becomes a root in the recovered copy.".into());
            }
            if !self.protection_scopes(id, None)?.is_empty() {
                e.locked = true;
            }
            let mut stmt = self
                .db
                .prepare("SELECT id FROM blocks WHERE entity=?1 ORDER BY ordinal,id")?;
            let block_ids = stmt
                .query_map([id.to_string()], |r| r.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            for bid in block_ids {
                let bid = Id::parse(&bid)?;
                let membership = Self::block_exercise(&self.db, bid)?;
                if exercise.is_some() && membership != exercise {
                    continue;
                }
                let b = Self::read_block(&self.db, bid, access)?;
                scopes.extend(self.protection_scopes(id, Some(bid))?);
                has_ink |= !b.source_ids.is_empty();
                if !include_original_ink && !b.source_ids.is_empty() {
                    return Err(Error::ReviewRequired);
                }
                blocks.push(NativeBlock {
                    strokes: Self::original_strokes(&self.db, &b)?,
                    history: self.history(bid, access)?,
                    current: b,
                });
                if exercise.is_none() {
                    if let Some(owner) = membership {
                        let version = self.db.query_row(
                            "SELECT revision FROM membership_versions WHERE block=?1",
                            [bid.to_string()],
                            |r| r.get(0),
                        )?;
                        memberships.push(NativeMembership {
                            block: bid,
                            exercise: owner,
                            version,
                        });
                    }
                }
                if self
                    .db
                    .prepare("SELECT 1 FROM confirmed_headings WHERE block=?1")?
                    .exists([bid.to_string()])?
                {
                    headings.push(bid);
                }
            }
            for l in self.links(id, false, access)? {
                if exercise.is_some()
                    && l.anchor
                        .is_none_or(|a| !blocks.iter().any(|b| b.current.id == a))
                {
                    continue;
                }
                if selected.contains(&l.target) {
                    links.push(l)
                } else {
                    omissions.push("Unselected link target omitted; current subset format does not preserve external native references.".into());
                }
            }
            entities.push(e);
        }
        if self
            .db
            .prepare("SELECT 1 FROM workspace_panes LIMIT 1")?
            .exists([])?
        {
            omissions.push("App workspace and pane activation are library-local and are not included in native copies; selected saved views remain included.".into());
        }
        let mut document = NativeDocument {
            version: 7,
            created_ms: now_ms(),
            entities,
            blocks,
            links,
            omissions: omissions.clone(),
            exercises,
            memberships,
            headings,
            rules,
            modes,
            attachments: Default::default(),
        };
        document.attachments = self.export_attachments(&mut document, exercise, access)?;
        document.validate()?;
        if Self::setting(&self.db, "generation")? != generation {
            return Err(Error::Conflict);
        }
        Ok(ExportPlan {
            library: Self::setting(&self.db, "library_id")?,
            generation,
            selected_entities: document.entities.len(),
            selected_blocks: document.blocks.len(),
            includes_raw_ink: has_ink,
            omissions: document.omissions.clone(),
            document,
            scopes,
        })
    }
    pub fn finish_native_export(
        &self,
        plan: ExportPlan,
        protection: ExportProtection<'_>,
        auth: &mut dyn Authenticator,
        acknowledge_omissions: bool,
    ) -> Result<Exported> {
        if Self::setting(&self.db, "library_id")? != plan.library
            || Self::setting(&self.db, "generation")? != plan.generation
        {
            return Err(Error::Conflict);
        }
        if !plan.omissions.is_empty() && !acknowledge_omissions {
            return Err(Error::ReviewRequired);
        }
        if !plan.scopes.is_empty() && !auth.verify(Id::new(), &plan.scopes) {
            return Err(Error::Denied);
        }
        let plain = zeroize::Zeroizing::new(plan.document.encode()?);
        let exported = match protection {
            ExportProtection::Encrypted { password } => {
                let encrypted = crypto::encrypt(&plain, password)?;
                Exported {
                    bytes: encrypted.bytes,
                    recovery_key: Some(encrypted.recovery_key),
                }
            }
            ExportProtection::Plaintext {
                acknowledge_protected,
            } => {
                if !plan.scopes.is_empty() && !acknowledge_protected {
                    return Err(Error::ReviewRequired);
                }
                Exported {
                    bytes: plain.to_vec(),
                    recovery_key: None,
                }
            }
        };
        // Authentication/encryption may be slow. Recheck after both before releasing
        // any bytes; a second connection may have changed locks or revoked sources.
        if Self::setting(&self.db, "library_id")? != plan.library
            || Self::setting(&self.db, "generation")? != plan.generation
        {
            return Err(Error::Conflict);
        }
        Ok(exported)
    }
    /// Restore to independent IDs by default. Never upsert into an existing object.
    /// Protection stays set; destination platform credentials are established later.
    pub fn import_native(&mut self, bytes: &[u8], access: &Access) -> Result<Vec<Id>> {
        self.import_document(NativeDocument::decode(bytes)?, access, None, true)
    }
    /// Local independent copy: current/source representations keep their values,
    /// while every durable owner/block/stroke/view ID becomes independent.
    pub fn copy_exercise_independent(&mut self, exercise: Id, access: &Access) -> Result<Id> {
        let plan = self.prepare_exercise_export(exercise, true, access)?;
        let mut doc = plan.document;
        doc.entities[0].title.push_str(" (independent copy)");
        doc.entities[0].created_ms = now_ms();
        doc.entities[0].modified_ms = now_ms();
        let ids =
            self.import_document(doc, access, Some((&plan.generation, &plan.scopes)), false)?;
        Ok(ids[0])
    }
    fn import_document(
        &mut self,
        mut doc: NativeDocument,
        access: &Access,
        copy_guard: Option<(&str, &BTreeSet<Scope>)>,
        recovered: bool,
    ) -> Result<Vec<Id>> {
        doc.validate()?;
        let mut ids = BTreeMap::new();
        for e in &doc.entities {
            ids.insert(e.id, Id::new());
        }
        for b in &doc.blocks {
            ids.insert(b.current.id, Id::new());
            for block in std::iter::once(&b.current).chain(b.history.iter().map(|h| &h.block)) {
                for region in &block.regions {
                    ids.entry(region.id).or_default();
                }
            }
            for s in &b.strokes {
                ids.insert(s.id, Id::new());
            }
        }
        for e in &doc.exercises {
            ids.insert(e.id, Id::new());
        }
        doc.attachments.add_ids(&mut ids);
        for e in &mut doc.entities {
            e.id = ids[&e.id];
            e.parent = e.parent.map(|p| ids[&p]);
            if recovered {
                e.title = format!("{} (recovered)", e.title);
            }
        }
        let remap_block = |b: &mut Block| {
            b.id = ids[&b.id];
            b.entity = ids[&b.entity];
            b.source_ids = b.source_ids.iter().map(|i| ids[i]).collect();
            for region in &mut b.regions {
                region.id = ids[&region.id];
                region.strokes = region.strokes.iter().map(|id| ids[id]).collect();
            }
            b.visible_strokes = b.visible_strokes.iter().map(|i| ids[i]).collect();
            if let crate::content::Content::Image(image) = &mut b.content {
                image.asset = ids[&image.asset];
            }
        };
        for b in &mut doc.blocks {
            remap_block(&mut b.current);
            for h in &mut b.history {
                remap_block(&mut h.block);
            }
            for s in &mut b.strokes {
                s.id = ids[&s.id];
            }
        }
        for l in &mut doc.links {
            l.id = Id::new();
            l.origin = ids[&l.origin];
            l.target = ids[&l.target];
            l.anchor = l.anchor.map(|a| ids[&a]);
        }
        for e in &mut doc.exercises {
            e.id = ids[&e.id];
            e.master = ids[&e.master];
            e.heading = e.heading.map(|h| ids[&h]);
        }
        for m in &mut doc.memberships {
            m.block = ids[&m.block];
            m.exercise = ids[&m.exercise];
        }
        for h in &mut doc.headings {
            *h = ids[h];
        }
        for r in &mut doc.rules {
            r.master = ids[&r.master];
        }
        for m in &mut doc.modes {
            m.entity = ids[&m.entity];
        }
        doc.attachments.remap(&ids);
        doc.validate()?;
        self.write(|db| {
            Self::require_app(db, access)?;
            if let Some((expected, scopes)) = copy_guard {
                if Self::setting(db, "generation")? != expected {
                    return Err(Error::Conflict);
                }
                if scopes.iter().any(|scope| !access.permits(scope)) {
                    return Err(Error::Denied);
                }
            }
            // Parent insertion order is not trusted. Defer FK checks until commit.
            db.execute_batch("PRAGMA defer_foreign_keys=ON;")?;
            for e in &doc.entities {
                Self::save_entity(db, e)?;
            }
            Self::import_assets(db, &doc.attachments)?;
            for (ordinal, b) in doc.blocks.iter().enumerate() {
                db.execute(
                    "INSERT INTO blocks VALUES(?1,?2,?3,?4)",
                    rusqlite::params![
                        b.current.id.to_string(),
                        b.current.entity.to_string(),
                        ordinal as i64,
                        serde_json::to_string(&b.current)?
                    ],
                )?;
                for s in &b.strokes {
                    let p = serde_json::to_string(s)?;
                    db.execute(
                        "INSERT INTO strokes VALUES(?1,?2,?3,?4)",
                        rusqlite::params![
                            s.id.to_string(),
                            b.current.id.to_string(),
                            p,
                            digest(p.as_bytes())
                        ],
                    )?;
                }
                // Translate historical active-use times to destination clock while
                // keeping their age relative to the newest retained entry.
                let tick: i64 = Self::setting(db, "active_ms")?
                    .parse()
                    .map_err(|_| Error::Integrity)?;
                let newest = b.history.iter().map(|h| h.active_ms).max().unwrap_or(0);
                for h in &b.history {
                    let age = newest - h.active_ms;
                    let t = tick.saturating_sub(age.min(i64::MAX as u64) as i64).max(0);
                    db.execute(
                        "INSERT INTO history VALUES(?1,?2,?3,?4)",
                        rusqlite::params![
                            b.current.id.to_string(),
                            h.revision,
                            t,
                            serde_json::to_string(&h.block)?
                        ],
                    )?;
                }
            }
            for e in &doc.exercises {
                Self::save_exercise(db, e)?;
            }
            for m in &doc.memberships {
                db.execute(
                    "INSERT INTO exercise_members VALUES(?1,?2)",
                    rusqlite::params![m.block.to_string(), m.exercise.to_string()],
                )?;
                db.execute(
                    "INSERT INTO membership_versions VALUES(?1,?2)",
                    rusqlite::params![m.block.to_string(), m.version],
                )?;
            }
            for h in &doc.headings {
                db.execute("INSERT INTO confirmed_headings VALUES(?1)", [h.to_string()])?;
            }
            for r in &doc.rules {
                db.execute(
                    "INSERT INTO exercise_rules VALUES(?1,?2)",
                    rusqlite::params![r.master.to_string(), r.keyword],
                )?;
            }
            for m in &doc.modes {
                db.execute(
                    "INSERT INTO modes VALUES(?1,?2)",
                    rusqlite::params![m.entity.to_string(), serde_json::to_string(&m.mode)?],
                )?;
            }
            Self::import_attachment_relations(db, &doc.attachments)?;
            for l in &doc.links {
                db.execute(
                    "INSERT INTO links VALUES(?1,?2,?3,?4,'{}')",
                    rusqlite::params![
                        l.id.to_string(),
                        l.origin.to_string(),
                        l.target.to_string(),
                        l.anchor.map(|i| i.to_string())
                    ],
                )?;
            }
            Ok(doc.entities.iter().map(|e| e.id).collect())
        })
    }
}

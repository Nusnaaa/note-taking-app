use super::*;
use crate::{
    content::Content,
    digest,
    ink::{Stroke, Transform},
    now_ms,
};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Display {
    #[default]
    Original,
    Editable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Block {
    pub id: Id,
    pub entity: Id,
    pub revision: u64,
    pub content: Content,
    /// All observations ever committed for this block, independent of undo expiry.
    pub source_ids: Vec<Id>,
    /// Erasure modifies visibility, never the observed samples.
    pub visible_strokes: Vec<Id>,
    pub transform: Transform,
    pub display: Display,
    pub visible: bool,
    pub locked: bool,
    pub manually_corrected: bool,
}
impl Block {
    pub fn validate(&self) -> Result<()> {
        self.content.validate()?;
        self.transform.validate()?;
        let all: BTreeSet<_> = self.source_ids.iter().copied().collect();
        let visible: BTreeSet<_> = self.visible_strokes.iter().copied().collect();
        if all.len() != self.source_ids.len()
            || visible.len() != self.visible_strokes.len()
            || !visible.is_subset(&all)
        {
            return Err(Error::Invalid("stroke membership"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct InkSnapshot {
    pub(crate) block: Id,
    pub(crate) revision: u64,
    pub(crate) fingerprint: String,
    pub(crate) strokes: Vec<Stroke>,
    pub(crate) transform: Transform,
}
impl InkSnapshot {
    pub fn block(&self) -> Id {
        self.block
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
    pub fn strokes(&self) -> &[Stroke] {
        &self.strokes
    }
    pub fn transform(&self) -> Transform {
        self.transform
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryEntry {
    pub revision: u64,
    pub active_ms: u64,
    pub block: Block,
}

impl Store {
    pub(crate) fn raw_block(db: &Connection, id: Id) -> Result<Block> {
        let json: Option<String> = db
            .query_row(
                "SELECT payload FROM blocks WHERE id=?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        Ok(serde_json::from_str(&json.ok_or(Error::Unavailable)?)?)
    }
    pub(crate) fn read_block(db: &Connection, id: Id, access: &Access) -> Result<Block> {
        let b = Self::raw_block(db, id)?;
        Self::authorise(db, b.entity, access, false)?;
        Self::authorise_block_protection(db, &b, access)?;
        Ok(b)
    }
    pub(crate) fn authorise_block_protection(
        db: &Connection,
        b: &Block,
        access: &Access,
    ) -> Result<()> {
        if b.locked && !access.permits(&Scope::Block(b.id)) {
            return Err(Error::Denied);
        }
        if let Some(exercise) = Self::block_exercise(db, b.id)? {
            let e = Self::raw_exercise(db, exercise)?;
            if e.locked && !access.permits(&Scope::Exercise(e.id)) {
                return Err(Error::Denied);
            }
        }
        Ok(())
    }
    pub(crate) fn persist_block(db: &Connection, b: &Block) -> Result<()> {
        b.validate()?;
        let payload = serde_json::to_string(b)?;
        db.execute(
            "UPDATE blocks SET payload=?1 WHERE id=?2",
            rusqlite::params![payload, b.id.to_string()],
        )?;
        let tick = Self::setting(db, "active_ms")?
            .parse::<i64>()
            .map_err(|_| Error::Integrity)?;
        db.execute(
            "INSERT INTO history VALUES(?1,?2,?3,?4)",
            rusqlite::params![b.id.to_string(), b.revision, tick, payload],
        )?;
        let mut e = Self::raw_entity(db, b.entity)?;
        e.revision += 1;
        e.modified_ms = now_ms();
        Self::save_entity(db, &e)
    }
    pub fn block(&self, id: Id, access: &Access) -> Result<Block> {
        Self::read_block(&self.db, id, access)
    }
    /// Page by ordinal, then filter protected blocks. No content from denied blocks.
    pub fn blocks(
        &self,
        entity: Id,
        offset: u32,
        limit: u32,
        access: &Access,
    ) -> Result<Vec<Block>> {
        Self::authorise(&self.db, entity, access, false)?;
        if limit > 512 {
            return Err(Error::ResourceLimit);
        }
        let mut stmt = self.db.prepare(
            "SELECT id FROM blocks WHERE entity=?1 ORDER BY ordinal,id LIMIT ?2 OFFSET ?3",
        )?;
        let ids = stmt
            .query_map(rusqlite::params![entity.to_string(), limit, offset], |r| {
                r.get::<_, String>(0)
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut out = Vec::new();
        for id in ids {
            match Self::read_block(&self.db, Id::parse(&id)?, access) {
                Ok(b) => out.push(b),
                Err(Error::Denied) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(out)
    }
    pub fn append_ink(
        &mut self,
        entity: Id,
        strokes: Vec<Stroke>,
        access: &Access,
    ) -> Result<Block> {
        self.append_ink_to(WritingTarget::Entity(entity), strokes, access)
    }
    pub fn append_ink_to(
        &mut self,
        target: WritingTarget,
        strokes: Vec<Stroke>,
        access: &Access,
    ) -> Result<Block> {
        if strokes.is_empty() || strokes.len() > 4096 {
            return Err(Error::Invalid("stroke batch"));
        }
        let mut ids = BTreeSet::new();
        let mut sequences = BTreeSet::new();
        for s in &strokes {
            s.validate()?;
            if !ids.insert(s.id) || !sequences.insert((s.capture_session, s.sequence)) {
                return Err(Error::Invalid("duplicate stroke"));
            }
        }
        // Preserve caller acquisition order, rejecting misordered samples per session.
        let mut sequence = std::collections::BTreeMap::new();
        for s in &strokes {
            if sequence
                .insert(s.capture_session, s.sequence)
                .is_some_and(|old| old >= s.sequence)
            {
                return Err(Error::Invalid("stroke sequence"));
            }
        }
        self.append_block(target, Content::Ink, strokes, access)
    }
    pub fn append_content(
        &mut self,
        entity: Id,
        content: Content,
        access: &Access,
    ) -> Result<Block> {
        self.append_content_to(WritingTarget::Entity(entity), content, access)
    }
    pub fn append_content_to(
        &mut self,
        target: WritingTarget,
        content: Content,
        access: &Access,
    ) -> Result<Block> {
        content.validate()?;
        self.append_block(target, content, vec![], access)
    }
    fn append_block(
        &mut self,
        target: WritingTarget,
        content: Content,
        strokes: Vec<Stroke>,
        access: &Access,
    ) -> Result<Block> {
        let entity = Self::writing_owner(&self.db, target, access)?;
        let ids: Vec<_> = strokes.iter().map(|s| s.id).collect();
        let display = if matches!(content, Content::Ink) {
            Display::Original
        } else {
            Display::Editable
        };
        let b = Block {
            id: Id::new(),
            entity,
            revision: 0,
            content,
            source_ids: ids.clone(),
            visible_strokes: ids,
            transform: Transform::default(),
            display,
            visible: true,
            locked: false,
            manually_corrected: false,
        };
        self.write_for(entity, |db| {
            if Self::writing_owner(db, target, access)? != entity { return Err(Error::Conflict); }
            let e = Self::authorise(db, entity, access, false)?;
            if !e.kind.is_surface() {
                return Err(Error::Invalid("writing destination"));
            }
            let mut ordinal: i64 = db.query_row(
                "SELECT COALESCE(MAX(ordinal),-1)+1 FROM blocks WHERE entity=?1",
                [entity.to_string()],
                |r| r.get(0),
            )?;
            if let WritingTarget::Exercise(exercise) = target {
                let last: Option<i64> = db.query_row("SELECT MAX(b.ordinal) FROM blocks b JOIN exercise_members m ON m.block=b.id WHERE m.exercise=?1", [exercise.to_string()], |r|r.get(0))?;
                if let Some(last) = last {
                    ordinal = last + 1;
                    db.execute("UPDATE blocks SET ordinal=ordinal+1 WHERE entity=?1 AND ordinal>=?2",rusqlite::params![entity.to_string(),ordinal])?;
                }
            }
            db.execute(
                "INSERT INTO blocks VALUES(?1,?2,?3,?4)",
                rusqlite::params![
                    b.id.to_string(),
                    entity.to_string(),
                    ordinal,
                    serde_json::to_string(&b)?
                ],
            )?;
            if let WritingTarget::Exercise(exercise) = target {
                Self::set_membership(db, b.id, Some(exercise))?;
            }
            for s in &strokes {
                let payload = serde_json::to_string(s)?;
                db.execute(
                    "INSERT INTO strokes VALUES(?1,?2,?3,?4)",
                    rusqlite::params![
                        s.id.to_string(),
                        b.id.to_string(),
                        payload,
                        digest(payload.as_bytes())
                    ],
                )?;
            }
            Self::persist_block(db, &b)?;
            Ok(b.clone())
        })
    }
    pub fn edit_content(
        &mut self,
        id: Id,
        revision: u64,
        content: Content,
        access: &Access,
    ) -> Result<Block> {
        content.validate()?;
        self.edit_block(id, revision, access, |b| {
            b.content = content;
            b.manually_corrected = true;
            b.display = Display::Editable;
            Ok(())
        })
    }
    pub(crate) fn edit_block(
        &mut self,
        id: Id,
        revision: u64,
        access: &Access,
        edit: impl FnOnce(&mut Block) -> Result<()>,
    ) -> Result<Block> {
        let entity = Self::read_block(&self.db, id, access)?.entity;
        self.write_for(entity, |db| {
            let mut b = Self::read_block(db, id, access)?;
            if b.revision != revision {
                return Err(Error::Conflict);
            }
            edit(&mut b)?;
            b.revision += 1;
            Self::persist_block(db, &b)?;
            Ok(b)
        })
    }
    pub fn set_display(
        &mut self,
        id: Id,
        revision: u64,
        display: Display,
        access: &Access,
    ) -> Result<Block> {
        self.edit_block(id, revision, access, |b| {
            if display == Display::Editable && matches!(b.content, Content::Ink) {
                return Err(Error::Invalid("no editable conversion"));
            }
            b.display = display;
            Ok(())
        })
    }
    pub fn transform_block(
        &mut self,
        id: Id,
        revision: u64,
        transform: Transform,
        access: &Access,
    ) -> Result<Block> {
        transform.validate()?;
        self.edit_block(id, revision, access, |b| {
            b.transform = transform;
            Ok(())
        })
    }
    pub fn erase_strokes(
        &mut self,
        id: Id,
        revision: u64,
        strokes: &[Id],
        access: &Access,
    ) -> Result<Block> {
        self.edit_block(id, revision, access, |b| {
            if strokes.is_empty() || strokes.iter().any(|s| !b.visible_strokes.contains(s)) {
                return Err(Error::Invalid("erasure selection"));
            }
            b.visible_strokes.retain(|s| !strokes.contains(s));
            Ok(())
        })
    }
    pub fn set_visible(
        &mut self,
        id: Id,
        revision: u64,
        visible: bool,
        access: &Access,
    ) -> Result<Block> {
        self.edit_block(id, revision, access, |b| {
            b.visible = visible;
            Ok(())
        })
    }
    pub fn set_block_lock(
        &mut self,
        id: Id,
        revision: u64,
        locked: bool,
        access: &Access,
    ) -> Result<Block> {
        if !access.permits(&Scope::Block(id)) {
            return Err(Error::Denied);
        }
        self.edit_block(id, revision, access, |b| {
            b.locked = locked;
            Ok(())
        })
    }
    pub(crate) fn original_strokes(db: &Connection, b: &Block) -> Result<Vec<Stroke>> {
        let mut strokes = Vec::new();
        for id in &b.source_ids {
            let (payload, hash): (String, String) = db.query_row(
                "SELECT payload,digest FROM strokes WHERE id=?1 AND block=?2",
                rusqlite::params![id.to_string(), b.id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            if digest(payload.as_bytes()) != hash {
                return Err(Error::Integrity);
            }
            let s: Stroke = serde_json::from_str(&payload)?;
            s.validate()?;
            strokes.push(s);
        }
        Ok(strokes)
    }
    pub fn source_ink(&self, block: Id, access: &Access) -> Result<Vec<Stroke>> {
        let b = Self::read_block(&self.db, block, access)?;
        Self::original_strokes(&self.db, &b)
    }
    pub(crate) fn make_snapshot(db: &Connection, id: Id, access: &Access) -> Result<InkSnapshot> {
        let b = Self::read_block(db, id, access)?;
        if !b.visible {
            return Err(Error::Unavailable);
        }
        let strokes = Self::original_strokes(db, &b)?
            .into_iter()
            .filter(|s| b.visible_strokes.contains(&s.id))
            .collect::<Vec<_>>();
        if strokes.is_empty() {
            return Err(Error::Invalid("no selected ink"));
        }
        let fingerprint = digest(&serde_json::to_vec(&(&b, &strokes))?);
        Ok(InkSnapshot {
            block: id,
            revision: b.revision,
            fingerprint,
            strokes,
            transform: b.transform,
        })
    }
    pub fn ink_snapshot(&self, id: Id, access: &Access) -> Result<InkSnapshot> {
        Self::make_snapshot(&self.db, id, access)
    }
    pub fn history(&self, id: Id, access: &Access) -> Result<Vec<HistoryEntry>> {
        Self::read_block(&self.db, id, access)?;
        let mut stmt = self.db.prepare(
            "SELECT revision,active_ms,payload FROM history WHERE block=?1 ORDER BY revision",
        )?;
        let rows = stmt
            .query_map([id.to_string()], |r| {
                Ok((
                    r.get::<_, u64>(0)?,
                    r.get::<_, u64>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(revision, active_ms, payload)| {
                Ok(HistoryEntry {
                    revision,
                    active_ms,
                    block: serde_json::from_str(&payload)?,
                })
            })
            .collect()
    }
    pub fn restore_block_revision(
        &mut self,
        id: Id,
        current_revision: u64,
        old_revision: u64,
        access: &Access,
    ) -> Result<Block> {
        let master = Self::read_block(&self.db, id, access)?.entity;
        self.write_for(master, |db| {
            let current = Self::read_block(db, id, access)?;
            if current.revision != current_revision {
                return Err(Error::Conflict);
            }
            let payload: String = db
                .query_row(
                    "SELECT payload FROM history WHERE block=?1 AND revision=?2",
                    rusqlite::params![id.to_string(), old_revision],
                    |r| r.get(0),
                )
                .optional()?
                .ok_or(Error::Unavailable)?;
            let mut old: Block = serde_json::from_str(&payload)?;
            // Undo cannot remove protection added since the historical snapshot.
            old.locked = current.locked;
            old.revision = current.revision + 1;
            // Retain all observations even if later edits extended source ownership.
            old.source_ids = current.source_ids;
            Self::persist_block(db, &old)?;
            Ok(old)
        })
    }
    pub fn history_hours(&self) -> Result<f64> {
        Self::setting(&self.db, "history_hours")?
            .parse()
            .map_err(|_| Error::Integrity)
    }
    pub fn set_history_hours(&mut self, hours: f64, access: &Access) -> Result<()> {
        if !hours.is_finite() || hours <= 0. || hours > 168. {
            return Err(Error::Invalid("history duration"));
        }
        self.write(|db| {
            Self::require_app(db, access)?;
            db.execute(
                "UPDATE settings SET value=?1 WHERE key='history_hours'",
                [hours.to_string()],
            )?;
            Ok(())
        })
    }
    /// Host supplies measured foreground authoring time, not elapsed wall-clock time.
    pub fn record_active_use(&mut self, duration: Duration, access: &Access) -> Result<()> {
        let increment = i64::try_from(duration.as_millis()).map_err(|_| Error::ResourceLimit)?;
        self.write(|db| {
            Self::require_app(db, access)?;
            let current: i64 = Self::setting(db, "active_ms")?
                .parse()
                .map_err(|_| Error::Integrity)?;
            let next = current.checked_add(increment).ok_or(Error::ResourceLimit)?;
            db.execute(
                "UPDATE settings SET value=?1 WHERE key='active_ms'",
                [next.to_string()],
            )?;
            Ok(())
        })
    }
    pub fn prune_history(&mut self, access: &Access) -> Result<usize> {
        self.write(|db|{
            Self::require_app(db,access)?;
            let active:i64=Self::setting(db,"active_ms")?.parse().map_err(|_|Error::Integrity)?;
            let hours:f64=Self::setting(db,"history_hours")?.parse().map_err(|_|Error::Integrity)?;
            let cutoff=active.saturating_sub((hours*3_600_000.) as i64);
            // The newest pre-cutoff full snapshot is the retained recovery checkpoint.
            // Immutable strokes live in another table and are never pruned here.
            let mut stmt = db.prepare("SELECT id FROM blocks")?;
            let ids = stmt.query_map([], |r| r.get::<_, String>(0))?.collect::<std::result::Result<Vec<_>, _>>()?;
            let mut deleted = 0;
            for id in ids {
                match Self::read_block(db, Id::parse(&id)?, access) {
                    Ok(_) => {},
                    Err(Error::Denied | Error::Unavailable) => continue,
                    Err(e) => return Err(e),
                }
                deleted += db.execute("DELETE FROM history WHERE block=?1 AND revision < (SELECT MAX(h.revision) FROM history h WHERE h.block=?1 AND h.active_ms<=?2)", rusqlite::params![id, cutoff])?;
            }
            Ok(deleted)
        })
    }
    pub fn search(&self, query: &str, access: &Access) -> Result<Vec<(Id, Option<Id>)>> {
        Self::require_app(&self.db, access)?;
        if query.is_empty() || query.len() > 4096 {
            return Err(Error::Invalid("search query"));
        }
        let mut out = Vec::new();
        let mut stmt = self.db.prepare("SELECT id FROM entities")?;
        let ids = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for id in ids {
            let id = Id::parse(&id)?;
            let e = match Self::authorise(&self.db, id, access, false) {
                Ok(e) => e,
                Err(Error::Denied | Error::Unavailable) => continue,
                Err(e) => return Err(e),
            };
            if e.title.contains(query) {
                out.push((id, None));
            }
            let mut blocks = self
                .db
                .prepare("SELECT id FROM blocks WHERE entity=?1 ORDER BY ordinal,id")?;
            for row in blocks.query_map([id.to_string()], |r| r.get::<_, String>(0))? {
                let b = match Self::read_block(&self.db, Id::parse(&row?)?, access) {
                    Ok(b) => b,
                    Err(Error::Denied) => continue,
                    Err(e) => return Err(e),
                };
                if b.visible && b.content.literal().contains(query) {
                    out.push((id, Some(b.id)));
                }
            }
        }
        Ok(out)
    }
}

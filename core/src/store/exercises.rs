//! Linked exercises project the master's blocks; they never own duplicate ink.
//! Membership and protection survive heading changes and removal of a view.
use super::*;
use crate::{content::Content, now_ms};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exercise {
    pub id: Id,
    pub master: Id,
    pub heading: Option<Id>,
    pub title: String,
    pub revision: u64,
    pub locked: bool,
    pub visible: bool,
    pub created_ms: i64,
}
#[derive(Debug, Clone, Copy)]
pub enum WritingTarget {
    Entity(Id),
    Exercise(Id),
}

#[derive(Debug, Clone)]
pub struct ExtractionCandidate {
    pub heading: Id,
    pub title: String,
    pub existing_exercise: Option<Id>,
    pub blocks: Vec<Id>,
}
pub struct ExtractionPlan {
    generation: String,
    master: Id,
    candidates: Vec<ExtractionCandidate>,
}
impl ExtractionPlan {
    pub fn candidates(&self) -> &[ExtractionCandidate] {
        &self.candidates
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MembershipChange {
    pub block: Id,
    pub from: Option<Id>,
    pub to: Option<Id>,
    pub after_version: u64,
}
pub struct MembershipPlan {
    generation: String,
    master: Id,
    changes: Vec<MembershipChange>,
}
impl MembershipPlan {
    pub fn changes(&self) -> &[MembershipChange] {
        &self.changes
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MembershipOperation {
    pub id: Id,
    pub master: Id,
    pub changes: Vec<MembershipChange>,
    pub created_ms: i64,
    pub reverses: Option<Id>,
}

impl Store {
    pub(crate) fn raw_exercise(db: &Connection, id: Id) -> Result<Exercise> {
        let payload: Option<String> = db
            .query_row(
                "SELECT payload FROM exercises WHERE id=?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        Ok(serde_json::from_str(&payload.ok_or(Error::Unavailable)?)?)
    }
    pub(crate) fn read_exercise(db: &Connection, id: Id, access: &Access) -> Result<Exercise> {
        let e = Self::raw_exercise(db, id)?;
        Self::authorise(db, e.master, access, false)?;
        if e.locked && !access.permits(&Scope::Exercise(id)) {
            return Err(Error::Denied);
        }
        Ok(e)
    }
    pub(crate) fn save_exercise(db: &Connection, e: &Exercise) -> Result<()> {
        if e.title.is_empty() || e.title.len() > 4096 {
            return Err(Error::Invalid("exercise title"));
        }
        db.execute("INSERT INTO exercises VALUES(?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload,heading=excluded.heading",rusqlite::params![e.id.to_string(),e.master.to_string(),e.heading.map(|h|h.to_string()),serde_json::to_string(e)?])?;
        Ok(())
    }
    pub(crate) fn block_exercise(db: &Connection, block: Id) -> Result<Option<Id>> {
        let id: Option<String> = db
            .query_row(
                "SELECT exercise FROM exercise_members WHERE block=?1",
                [block.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        id.as_deref().map(Id::parse).transpose()
    }
    fn membership_version(db: &Connection, block: Id) -> Result<u64> {
        Ok(db
            .query_row(
                "SELECT revision FROM membership_versions WHERE block=?1",
                [block.to_string()],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(0))
    }
    pub(crate) fn set_membership(db: &Connection, block: Id, exercise: Option<Id>) -> Result<()> {
        db.execute(
            "DELETE FROM exercise_members WHERE block=?1",
            [block.to_string()],
        )?;
        if let Some(id) = exercise {
            db.execute(
                "INSERT INTO exercise_members VALUES(?1,?2)",
                rusqlite::params![block.to_string(), id.to_string()],
            )?;
        }
        db.execute("INSERT INTO membership_versions VALUES(?1,1) ON CONFLICT(block) DO UPDATE SET revision=revision+1",[block.to_string()])?;
        Ok(())
    }
    fn touch_master(db: &Connection, master: Id) -> Result<()> {
        let mut e = Self::raw_entity(db, master)?;
        e.revision += 1;
        e.modified_ms = now_ms();
        Self::save_entity(db, &e)
    }
    pub(crate) fn writing_owner(
        db: &Connection,
        target: WritingTarget,
        access: &Access,
    ) -> Result<Id> {
        match target {
            WritingTarget::Entity(id) => {
                Self::authorise(db, id, access, false)?;
                Ok(id)
            }
            WritingTarget::Exercise(id) => {
                let e = Self::read_exercise(db, id, access)?;
                if !e.visible {
                    return Err(Error::Unavailable);
                };
                Ok(e.master)
            }
        }
    }
    pub fn exercise(&self, id: Id, access: &Access) -> Result<Exercise> {
        Self::read_exercise(&self.db, id, access)
    }
    /// Navigation uses this relationship instead of pretending a Note contains Notes.
    pub fn exercises(
        &self,
        master: Id,
        include_hidden: bool,
        access: &Access,
    ) -> Result<Vec<Exercise>> {
        Self::authorise(&self.db, master, access, false)?;
        let mut query = self
            .db
            .prepare("SELECT id FROM exercises WHERE master=?1 ORDER BY rowid")?;
        let mut result = Vec::new();
        for id in query.query_map([master.to_string()], |r| r.get::<_, String>(0))? {
            match Self::read_exercise(&self.db, Id::parse(&id?)?, access) {
                Ok(e) if e.visible || include_hidden => result.push(e),
                Ok(_) | Err(Error::Denied) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(result)
    }
    pub fn exercise_blocks(
        &self,
        id: Id,
        offset: u32,
        limit: u32,
        access: &Access,
    ) -> Result<Vec<Block>> {
        let e = Self::read_exercise(&self.db, id, access)?;
        if !e.visible {
            return Err(Error::Unavailable);
        }
        if limit > 512 {
            return Err(Error::ResourceLimit);
        }
        let mut query=self.db.prepare("SELECT b.id FROM blocks b JOIN exercise_members m ON m.block=b.id WHERE m.exercise=?1 ORDER BY b.ordinal,b.id LIMIT ?2 OFFSET ?3")?;
        let mut result = Vec::new();
        for id in query.query_map(rusqlite::params![id.to_string(), limit, offset], |r| {
            r.get::<_, String>(0)
        })? {
            match Self::read_block(&self.db, Id::parse(&id?)?, access) {
                Ok(b) => result.push(b),
                Err(Error::Denied) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(result)
    }
    pub fn exercise_mode(&self, id: Id, access: &Access) -> Result<WritingMode> {
        self.mode(self.exercise(id, access)?.master, access)
    }
    pub fn exercise_save_state(&self, id: Id, access: &Access) -> Result<SaveState> {
        self.document_save_state(self.exercise(id, access)?.master, access)
    }
    pub fn create_exercise(
        &mut self,
        master: Id,
        title: &str,
        blocks: &[Id],
        access: &Access,
    ) -> Result<Exercise> {
        if blocks.len() > 4096 {
            return Err(Error::ResourceLimit);
        }
        self.write_for(master, |db| {
            let owner = Self::authorise(db, master, access, false)?;
            if owner.kind != Kind::Note {
                return Err(Error::Invalid("exercise master"));
            }
            let mut unique = BTreeSet::new();
            for &id in blocks {
                if !unique.insert(id)
                    || Self::read_block(db, id, access)?.entity != master
                    || Self::block_exercise(db, id)?.is_some()
                {
                    return Err(Error::Invalid("exercise membership"));
                }
            }
            let e = Exercise {
                id: Id::new(),
                master,
                heading: None,
                title: title.into(),
                revision: 0,
                locked: false,
                visible: true,
                created_ms: now_ms(),
            };
            Self::save_exercise(db, &e)?;
            for &block in blocks {
                Self::set_membership(db, block, Some(e.id))?;
            }
            Self::touch_master(db, master)?;
            Ok(e)
        })
    }
    fn edit_exercise(
        &mut self,
        id: Id,
        revision: u64,
        access: &Access,
        edit: impl FnOnce(&mut Exercise),
    ) -> Result<Exercise> {
        let master = self.exercise(id, access)?.master;
        self.write_for(master, |db| {
            let mut e = Self::read_exercise(db, id, access)?;
            if e.revision != revision {
                return Err(Error::Conflict);
            }
            edit(&mut e);
            e.revision += 1;
            Self::save_exercise(db, &e)?;
            Self::touch_master(db, master)?;
            Ok(e)
        })
    }
    pub fn rename_exercise(
        &mut self,
        id: Id,
        revision: u64,
        title: &str,
        access: &Access,
    ) -> Result<Exercise> {
        self.edit_exercise(id, revision, access, |e| e.title = title.into())
    }
    pub fn set_exercise_visible(
        &mut self,
        id: Id,
        revision: u64,
        visible: bool,
        access: &Access,
    ) -> Result<Exercise> {
        self.edit_exercise(id, revision, access, |e| e.visible = visible)
    }
    pub fn set_exercise_lock(
        &mut self,
        id: Id,
        revision: u64,
        locked: bool,
        access: &Access,
    ) -> Result<Exercise> {
        if !access.permits(&Scope::Exercise(id)) {
            return Err(Error::Denied);
        }
        self.edit_exercise(id, revision, access, |e| e.locked = locked)
    }
    /// Heading confirmation is explicit; matching words in prose have no effect.
    pub fn set_heading(
        &mut self,
        block: Id,
        revision: u64,
        confirmed: bool,
        access: &Access,
    ) -> Result<Block> {
        let master = self.block(block, access)?.entity;
        self.write_for(master, |db| {
            let mut b = Self::read_block(db, block, access)?;
            if b.revision != revision {
                return Err(Error::Conflict);
            }
            if confirmed && !matches!(&b.content,Content::Text(t) if !t.trim().is_empty()) {
                return Err(Error::Invalid("heading text"));
            }
            if confirmed {
                db.execute(
                    "INSERT OR IGNORE INTO confirmed_headings VALUES(?1)",
                    [block.to_string()],
                )?;
            } else {
                db.execute(
                    "DELETE FROM confirmed_headings WHERE block=?1",
                    [block.to_string()],
                )?;
            }
            b.revision += 1;
            Self::persist_block(db, &b)?;
            Ok(b)
        })
    }
    pub fn set_exercise_keyword(
        &mut self,
        master: Id,
        keyword: &str,
        access: &Access,
    ) -> Result<()> {
        if keyword.trim().is_empty() || keyword.len() > 256 {
            return Err(Error::Invalid("exercise keyword"));
        }
        self.write_for(master,|db|{Self::authorise(db,master,access,false)?;db.execute("INSERT INTO exercise_rules VALUES(?1,?2) ON CONFLICT(master) DO UPDATE SET keyword=excluded.keyword",rusqlite::params![master.to_string(),keyword])?;Ok(())})
    }
    pub fn plan_extraction(&self, master: Id, access: &Access) -> Result<ExtractionPlan> {
        let generation = Self::setting(&self.db, "generation")?;
        if Self::authorise(&self.db, master, access, false)?.kind != Kind::Note {
            return Err(Error::Invalid("exercise master"));
        }
        let keyword: String = self
            .db
            .query_row(
                "SELECT keyword FROM exercise_rules WHERE master=?1",
                [master.to_string()],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or_else(|| "Exercise".into());
        let mut query=self.db.prepare("SELECT b.id,h.block IS NOT NULL FROM blocks b LEFT JOIN confirmed_headings h ON h.block=b.id WHERE b.entity=?1 ORDER BY b.ordinal,b.id LIMIT 10001")?;
        let rows = query
            .query_map([master.to_string()], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if rows.len() > 10000 {
            return Err(Error::ResourceLimit);
        }
        let mut candidates: Vec<ExtractionCandidate> = Vec::new();
        let mut current = None;
        for (block, heading) in rows {
            let block = Id::parse(&block)?;
            if heading {
                current = None;
            }
            let b = match Self::read_block(&self.db, block, access) {
                Ok(b) => b,
                Err(Error::Denied) => continue,
                Err(e) => return Err(e),
            };
            if !b.visible {
                continue;
            }
            let membership = Self::block_exercise(&self.db, block)?;
            if heading {
                if let Content::Text(title) = &b.content {
                    let matches = title.strip_prefix(&keyword).is_some_and(|rest| {
                        rest.is_empty() || rest.starts_with(char::is_whitespace)
                    });
                    if matches {
                        let existing: Option<String> = self
                            .db
                            .query_row(
                                "SELECT id FROM exercises WHERE heading=?1",
                                [block.to_string()],
                                |r| r.get(0),
                            )
                            .optional()?;
                        let existing = existing.as_deref().map(Id::parse).transpose()?;
                        if membership.is_some() && existing != membership {
                            continue;
                        }
                        if let Some(id) = existing {
                            Self::read_exercise(&self.db, id, access)?;
                        }
                        candidates.push(ExtractionCandidate {
                            heading: block,
                            title: title.clone(),
                            existing_exercise: existing,
                            blocks: Vec::new(),
                        });
                        current = Some(candidates.len() - 1);
                    }
                }
            }
            if membership.is_none() {
                if let Some(index) = current {
                    candidates[index].blocks.push(block);
                }
            }
        }
        candidates.retain(|c| !c.blocks.is_empty());
        if Self::setting(&self.db, "generation")? != generation {
            return Err(Error::Conflict);
        }
        Ok(ExtractionPlan {
            generation,
            master,
            candidates,
        })
    }
    pub fn accept_extraction(
        &mut self,
        plan: ExtractionPlan,
        confirmed: bool,
        access: &Access,
    ) -> Result<Vec<Exercise>> {
        if !confirmed {
            return Err(Error::ReviewRequired);
        }
        self.write_for(plan.master, |db| {
            if Self::setting(db, "generation")? != plan.generation {
                return Err(Error::Conflict);
            }
            Self::authorise(db, plan.master, access, false)?;
            let mut result = Vec::new();
            for c in plan.candidates {
                let mut e = if let Some(id) = c.existing_exercise {
                    Self::read_exercise(db, id, access)?
                } else {
                    Exercise {
                        id: Id::new(),
                        master: plan.master,
                        heading: Some(c.heading),
                        title: c.title,
                        revision: 0,
                        // The title is derived from this heading. Do not expose
                        // protected heading text through an unlocked directory row.
                        locked: Self::read_block(db, c.heading, access)?.locked,
                        visible: true,
                        created_ms: now_ms(),
                    }
                };
                e.visible = true;
                e.revision += 1;
                Self::save_exercise(db, &e)?;
                for block in c.blocks {
                    if Self::read_block(db, block, access)?.entity != plan.master
                        || Self::block_exercise(db, block)?.is_some()
                    {
                        return Err(Error::Conflict);
                    }
                    Self::set_membership(db, block, Some(e.id))?;
                }
                result.push(e);
            }
            Self::touch_master(db, plan.master)?;
            Ok(result)
        })
    }
    pub fn plan_reassignment(
        &self,
        master: Id,
        blocks: &[Id],
        destination: Option<Id>,
        access: &Access,
    ) -> Result<MembershipPlan> {
        let generation = Self::setting(&self.db, "generation")?;
        Self::authorise(&self.db, master, access, false)?;
        if blocks.is_empty() || blocks.len() > 4096 {
            return Err(Error::Invalid("reassignment selection"));
        }
        if let Some(to) = destination {
            if self.exercise(to, access)?.master != master {
                return Err(Error::Invalid("exercise destination"));
            }
        }
        let mut seen = BTreeSet::new();
        let mut changes = Vec::new();
        for &block in blocks {
            if !seen.insert(block) || self.block(block, access)?.entity != master {
                return Err(Error::Invalid("reassignment selection"));
            }
            let from = Self::block_exercise(&self.db, block)?;
            if from != destination {
                changes.push(MembershipChange {
                    block,
                    from,
                    to: destination,
                    after_version: Self::membership_version(&self.db, block)? + 1,
                });
            }
        }
        if changes.is_empty() {
            return Err(Error::Invalid("unchanged membership"));
        }
        if Self::setting(&self.db, "generation")? != generation {
            return Err(Error::Conflict);
        }
        Ok(MembershipPlan {
            generation,
            master,
            changes,
        })
    }
    fn apply_membership(
        db: &Connection,
        master: Id,
        changes: Vec<MembershipChange>,
        reverses: Option<Id>,
        access: &Access,
    ) -> Result<MembershipOperation> {
        Self::authorise(db, master, access, false)?;
        for change in &changes {
            let mut b = Self::read_block(db, change.block, access)?;
            if b.entity != master
                || Self::block_exercise(db, b.id)? != change.from
                || Self::membership_version(db, b.id)? + 1 != change.after_version
            {
                return Err(Error::Conflict);
            }
            for id in [change.from, change.to].into_iter().flatten() {
                if Self::read_exercise(db, id, access)?.master != master {
                    return Err(Error::Invalid("exercise destination"));
                }
            }
            // Moving out of a protected exercise cannot declassify its content.
            if let Some(from) = change.from {
                if Self::raw_exercise(db, from)?.locked && !b.locked {
                    b.locked = true;
                    b.revision += 1;
                    Self::persist_block(db, &b)?;
                }
            }
            Self::set_membership(db, b.id, change.to)?;
        }
        let operation = MembershipOperation {
            id: Id::new(),
            master,
            changes,
            created_ms: now_ms(),
            reverses,
        };
        db.execute(
            "INSERT INTO membership_operations VALUES(?1,?2,?3)",
            rusqlite::params![
                operation.id.to_string(),
                master.to_string(),
                serde_json::to_string(&operation)?
            ],
        )?;
        Self::touch_master(db, master)?;
        Ok(operation)
    }
    pub fn commit_reassignment(
        &mut self,
        plan: MembershipPlan,
        confirmed: bool,
        access: &Access,
    ) -> Result<MembershipOperation> {
        if !confirmed {
            return Err(Error::ReviewRequired);
        }
        self.write_for(plan.master, |db| {
            if Self::setting(db, "generation")? != plan.generation {
                return Err(Error::Conflict);
            }
            Self::apply_membership(db, plan.master, plan.changes, None, access)
        })
    }
    pub fn undo_reassignment(
        &mut self,
        operation: Id,
        access: &Access,
    ) -> Result<MembershipOperation> {
        let payload: Option<String> = self
            .db
            .query_row(
                "SELECT payload FROM membership_operations WHERE id=?1",
                [operation.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        let previous: MembershipOperation =
            serde_json::from_str(&payload.ok_or(Error::Unavailable)?)?;
        let changes = previous
            .changes
            .into_iter()
            .map(|c| MembershipChange {
                block: c.block,
                from: c.to,
                to: c.from,
                after_version: c.after_version + 1,
            })
            .collect();
        self.write_for(previous.master, |db| {
            Self::apply_membership(db, previous.master, changes, Some(operation), access)
        })
    }
    pub(crate) fn require_content_access(
        db: &Connection,
        entity: Id,
        access: &Access,
    ) -> Result<()> {
        let mut query = db.prepare("SELECT payload FROM blocks WHERE entity=?1")?;
        for row in query.query_map([entity.to_string()], |r| r.get::<_, String>(0))? {
            Self::authorise_block_protection(db, &serde_json::from_str(&row?)?, access)?;
        }
        let mut query = db.prepare("SELECT payload FROM exercises WHERE master=?1")?;
        for row in query.query_map([entity.to_string()], |r| r.get::<_, String>(0))? {
            let e: Exercise = serde_json::from_str(&row?)?;
            if e.locked && !access.permits(&Scope::Exercise(e.id)) {
                return Err(Error::Denied);
            }
        }
        Ok(())
    }
}

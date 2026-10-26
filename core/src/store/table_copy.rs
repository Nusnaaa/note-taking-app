//! Whole-table independent copy. This is an authored object copy, not a new
//! capture: observed contacts and their end/style provenance are unchanged.
use super::*;
use crate::{content::Content, digest};
use std::collections::BTreeMap;
impl Store {
    pub fn copy_table(
        &mut self,
        block: Id,
        revision: u64,
        target: WritingTarget,
        access: &Access,
    ) -> Result<Block> {
        let original = Self::read_block(&self.db, block, access)?;
        let owner = Self::writing_owner(&self.db, target, access)?;
        self.write_for(owner, |db| {
            let source = Self::read_block(db, block, access)?;
            if source.revision != revision || source.entity != original.entity {
                return Err(Error::Conflict);
            }
            Self::copy_table_in_transaction(db, &source, target, access)
        })
    }

    pub(crate) fn copy_table_in_transaction(
        db: &Connection,
        source: &Block,
        target: WritingTarget,
        access: &Access,
    ) -> Result<Block> {
        source.validate()?;
        let owner = Self::writing_owner(db, target, access)?;
        if !source.visible {
            return Err(Error::Unavailable);
        }
        let Content::Table(table) = &source.content else {
            return Err(Error::Invalid("table block"));
        };
        let cells: BTreeMap<_, _> = table
            .rows
            .iter()
            .flatten()
            .map(|cell| (cell.id, Id::new()))
            .collect();
        let strokes: BTreeMap<_, _> = source
            .source_ids
            .iter()
            .map(|id| (*id, Id::new()))
            .collect();
        let mapped = |map: &BTreeMap<Id, Id>, id: Id| map.get(&id).copied().ok_or(Error::Integrity);
        let mut copied = source.clone();
        copied.id = Id::new();
        copied.entity = owner;
        copied.revision = 0;
        copied.source_ids = source
            .source_ids
            .iter()
            .map(|id| mapped(&strokes, *id))
            .collect::<Result<_>>()?;
        copied.visible_strokes = source
            .visible_strokes
            .iter()
            .map(|id| mapped(&strokes, *id))
            .collect::<Result<_>>()?;
        for region in &mut copied.regions {
            let cell = mapped(&cells, region.target())?;
            region.id = Id::new();
            region.cell = Some(cell);
            region.strokes = region
                .strokes
                .iter()
                .map(|id| mapped(&strokes, *id))
                .collect::<Result<_>>()?;
        }
        let Content::Table(table) = &mut copied.content else {
            return Err(Error::Integrity);
        };
        for cell in table.rows.iter_mut().flatten() {
            cell.id = mapped(&cells, cell.id)?;
        }
        if let Some(layout) = &mut table.layout {
            for origin in &mut layout.ink_origins {
                origin.cell = mapped(&cells, origin.cell)?;
                origin.stroke = mapped(&strokes, origin.stroke)?;
            }
        }
        // Construction is private to this transaction. Attach membership
        // before restoring the source's block lock; no new scope is granted
        // to the caller and only the final locked state can commit.
        let locked = copied.locked;
        copied.locked = false;
        Self::insert_block_at_target(db, target, &copied, access)?;
        copied.locked = locked;
        // Read and hash-check one source record at a time; hidden and retired
        // observations remain retained without collecting their point arrays.
        for old in &source.source_ids {
            let mut stroke = Self::original_stroke(db, source.id, *old)?;
            stroke.id = mapped(&strokes, *old)?;
            if let Content::Table(table) = &copied.content {
                if let Some(layout) = &table.layout {
                    layout.validate_source(table, &stroke)?;
                }
            }
            let payload = serde_json::to_string(&stroke)?;
            db.execute(
                "INSERT INTO strokes VALUES(?1,?2,?3,?4)",
                rusqlite::params![
                    stroke.id.to_string(),
                    copied.id.to_string(),
                    payload,
                    digest(payload.as_bytes())
                ],
            )?;
        }
        let stored_layout: Option<String> = db
            .query_row(
                "SELECT payload FROM layouts WHERE block=?1",
                [source.id.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(json) = stored_layout {
            let mut layout: Layout = serde_json::from_str(&json)?;
            layout.bounds.validate()?;
            layout.revision = 0;
            db.execute(
                "INSERT INTO layouts VALUES(?1,?2)",
                rusqlite::params![copied.id.to_string(), serde_json::to_string(&layout)?],
            )?;
        }
        Self::authorise_block_metadata(db, source.id, access)?;
        if Self::writing_owner(db, target, access)? != owner {
            return Err(Error::Conflict);
        }
        Self::persist_block(db, &copied)?;
        Ok(copied)
    }
}

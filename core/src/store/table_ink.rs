//! Explicit table-cell capture. Cell IDs are region IDs inside their table;
//! original observations remain owned by the table independently of undo.
use super::*;
use crate::{content::Content, digest, ink::Stroke};
use std::collections::{BTreeMap, BTreeSet};

impl Store {
    /// Idempotent, revision-bound capture into one existing cell. Coordinates
    /// remain the observed table source coordinates; no clipping or grid inference.
    pub fn append_cell_capture(
        &mut self,
        ticket: Id,
        block: Id,
        revision: u64,
        cell: Id,
        strokes: Vec<Stroke>,
        access: &Access,
    ) -> Result<Block> {
        if strokes.is_empty() || strokes.len() > 1000 {
            return Err(Error::Invalid("cell capture batch"));
        }
        let mut ids = BTreeSet::new();
        let mut sequence = BTreeMap::new();
        for stroke in &strokes {
            stroke.validate_persisted()?;
            if !ids.insert(stroke.id)
                || sequence
                    .insert(stroke.capture_session, stroke.sequence)
                    .is_some_and(|previous| stroke.sequence <= previous)
            {
                return Err(Error::Invalid("cell capture order"));
            }
        }
        let owner = Self::read_block(&self.db, block, access)?.entity;
        let fingerprint = digest(&serde_json::to_vec(&(block, revision, cell, &strokes))?);
        self.write_for(owner, |db| {
            let mut current = Self::read_block(db, block, access)?;
            let old: Option<(String, String)> = db
                .query_row(
                    "SELECT block,fingerprint FROM capture_receipts WHERE ticket=?1",
                    [ticket.to_string()],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            if let Some((old_block, old_hash)) = old {
                // Do not confirm another protected receipt through this target.
                let old = Self::read_block(db, Id::parse(&old_block)?, access)?;
                if old.id != block || old_hash != fingerprint {
                    return Err(Error::Conflict);
                }
                return Ok(current);
            }
            if current.revision != revision {
                return Err(Error::Conflict);
            }
            let Content::Table(table) = &current.content else {
                return Err(Error::Invalid("cell capture table"));
            };
            if !table.rows.iter().flatten().any(|entry| entry.id == cell) {
                return Err(Error::Unavailable);
            }
            if !current.visible {
                return Err(Error::Unavailable);
            }
            let mut previous = BTreeMap::new();
            for original in Self::original_strokes(db, &current)? {
                previous.entry(original.capture_session).and_modify(|sequence: &mut u64| *sequence = (*sequence).max(original.sequence))
                    .or_insert(original.sequence);
            }
            for stroke in &strokes {
                if previous.insert(stroke.capture_session, stroke.sequence).is_some_and(|sequence| stroke.sequence <= sequence) {
                    return Err(Error::Invalid("cell capture session order"));
                }
            }
            if current.source_ids.len().saturating_add(strokes.len()) > 4096 {
                return Err(Error::ResourceLimit);
            }
            if cell == block
                || cell == owner
                || current.source_ids.contains(&cell)
                || ids.contains(&cell)
            {
                return Err(Error::Invalid("cell region identity"));
            }
            if !current.regions.iter().any(|region| region.id == cell) {
                for table in ["entities", "blocks", "strokes", "links", "exercises", "image_assets", "object_groups", "embeds", "view_settings"] {
                    if db.prepare(&format!("SELECT 1 FROM {table} WHERE id=?1"))?.exists([cell.to_string()])? {
                        return Err(Error::Conflict);
                    }
                }
                if db.prepare("SELECT 1 FROM blocks b,json_each(b.payload,'$.regions') r WHERE b.id<>?1 AND json_extract(r.value,'$.id')=?2 UNION ALL SELECT 1 FROM history h,json_each(h.payload,'$.regions') r WHERE h.block<>?1 AND json_extract(r.value,'$.id')=?2 UNION ALL SELECT 1 FROM canvas_layers c,json_each(c.payload,'$.layers') l WHERE json_extract(l.value,'$.id')=?2 LIMIT 1")?
                    .exists(rusqlite::params![block.to_string(),cell.to_string()])? {
                    return Err(Error::Conflict);
                }
            }
            let new_ids: Vec<_> = strokes.iter().map(|stroke| stroke.id).collect();
            for stroke in &strokes {
                let payload = serde_json::to_string(stroke)?;
                db.execute(
                    "INSERT INTO strokes VALUES(?1,?2,?3,?4)",
                    rusqlite::params![
                        stroke.id.to_string(),
                        block.to_string(),
                        payload,
                        digest(payload.as_bytes())
                    ],
                )?;
            }
            current.source_ids.extend(&new_ids);
            current.visible_strokes.extend(&new_ids);
            if let Some(region) = current.regions.iter_mut().find(|region| region.id == cell) {
                region.strokes.extend(new_ids);
                region.content = None; // new ink awaits an independent review; prior cell text stays retained
            } else {
                current.regions.push(Region {
                    id: cell,
                    strokes: new_ids,
                    content: None,
                });
            }
            current.revision += 1;
            Self::persist_block(db, &current)?;
            db.execute(
                "INSERT INTO capture_receipts VALUES(?1,?2,?3)",
                rusqlite::params![ticket.to_string(), block.to_string(), fingerprint],
            )?;
            Ok(current)
        })
    }
}

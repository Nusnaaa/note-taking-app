//! Explicit cell cut/copy. Raw observations remain independent of active cell
//! membership; copied vectors retain capture provenance, not a fabricated contact.
use super::*;
use crate::{
    content::Content,
    digest,
    objects::{CellValue, Table},
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CellTarget {
    pub block: Id,
    pub revision: u64,
    pub cell: Id,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CellTransferMode {
    Copy,
    Move,
}

fn table(block: &Block) -> Result<&Table> {
    block.validate()?;
    if !block.visible {
        return Err(Error::Unavailable);
    }
    match &block.content {
        Content::Table(t) => Ok(t),
        _ => Err(Error::Invalid("table block")),
    }
}
fn remove_cell_ink(block: &mut Block, cell: Id) {
    let removed: std::collections::BTreeSet<_> = block
        .regions
        .iter()
        .filter(|r| r.target() == cell)
        .flat_map(|r| r.strokes.iter().copied())
        .collect();
    block.regions.retain(|r| r.target() != cell);
    block.visible_strokes.retain(|id| !removed.contains(id));
    if let Content::Table(t) = &mut block.content {
        if let Some(layout) = &mut t.layout {
            layout.ink_origins.retain(|ink| ink.cell != cell);
        }
    }
}
fn replace_value(block: &mut Block, cell: Id, value: CellValue) -> Result<()> {
    let Content::Table(t) = &mut block.content else {
        return Err(Error::Invalid("table block"));
    };
    t.edit(cell, value)?;
    block.manually_corrected = true;
    Ok(())
}
impl Store {
    pub fn transfer_cell(
        &mut self,
        source: CellTarget,
        target: CellTarget,
        mode: CellTransferMode,
        access: &Access,
    ) -> Result<Vec<Block>> {
        let source_block = Self::read_block(&self.db, source.block, access)?;
        let target_block = Self::read_block(&self.db, target.block, access)?;
        if source.block == target.block && source.cell == target.cell {
            return Err(Error::Invalid("identical cell targets"));
        }
        let source_owner = source_block.entity;
        let target_owner = target_block.entity;
        let linked = mode == CellTransferMode::Move && source_owner != target_owner;
        let action = |db: &Connection| -> Result<Vec<Block>> {
            let mut from = Self::read_block(db, source.block, access)?;
            let mut to = Self::read_block(db, target.block, access)?;
            if from.revision != source.revision
                || to.revision != target.revision
                || from.entity != source_owner
                || to.entity != target_owner
            {
                return Err(Error::Conflict);
            }
            let source_table = table(&from)?;
            let target_table = table(&to)?;
            let value = source_table
                .rows
                .iter()
                .flatten()
                .find(|c| c.id == source.cell)
                .ok_or(Error::Unavailable)?
                .value
                .clone();
            if !target_table
                .rows
                .iter()
                .flatten()
                .any(|c| c.id == target.cell)
            {
                return Err(Error::Unavailable);
            }
            let mut region = from
                .regions
                .iter()
                .find(|r| r.target() == source.cell)
                .cloned();
            let mut origins = Vec::new();
            if region.is_some() {
                let source_layout = source_table.layout.as_ref().ok_or(Error::ReviewRequired)?;
                let target_layout = target_table.layout.as_ref().ok_or(Error::ReviewRequired)?;
                if source_layout.coordinate_space != target_layout.coordinate_space {
                    return Err(Error::Invalid("cell transfer coordinate space"));
                }
                origins = source_layout
                    .ink_origins
                    .iter()
                    .filter(|ink| ink.cell == source.cell)
                    .cloned()
                    .collect();
            }
            let same = source.block == target.block;
            let reuse = same && mode == CellTransferMode::Move;
            let mut visible = Vec::new();
            if let Some(ink) = &mut region {
                if !reuse && to.source_ids.len().saturating_add(ink.strokes.len()) > 4096 {
                    return Err(Error::ResourceLimit);
                }
                let mut copied = std::collections::BTreeMap::new();
                for id in &mut ink.strokes {
                    let old = *id;
                    let was_visible = from.visible_strokes.contains(&old);
                    if !reuse {
                        let mut stroke = Self::original_stroke(db, from.id, old)?;
                        stroke.id = Id::new();
                        let payload = serde_json::to_string(&stroke)?;
                        db.execute(
                            "INSERT INTO strokes VALUES(?1,?2,?3,?4)",
                            rusqlite::params![
                                stroke.id.to_string(),
                                to.id.to_string(),
                                payload,
                                digest(payload.as_bytes())
                            ],
                        )?;
                        *id = stroke.id;
                        to.source_ids.push(*id);
                    }
                    copied.insert(old, *id);
                    if was_visible {
                        visible.push(*id);
                    }
                }
                if !reuse {
                    ink.id = Id::new();
                }
                ink.cell = Some(target.cell);
                for origin in &mut origins {
                    origin.cell = target.cell;
                    origin.stroke = *copied.get(&origin.stroke).ok_or(Error::Integrity)?;
                }
            }
            if same {
                // The destination candidate owns any newly copied source IDs.
                if mode == CellTransferMode::Move {
                    remove_cell_ink(&mut to, source.cell);
                    replace_value(&mut to, source.cell, CellValue::Text(String::new()))?;
                }
            } else if mode == CellTransferMode::Move {
                remove_cell_ink(&mut from, source.cell);
                replace_value(&mut from, source.cell, CellValue::Text(String::new()))?;
            }
            remove_cell_ink(&mut to, target.cell);
            replace_value(&mut to, target.cell, value)?;
            if let Some(region) = region {
                to.regions.push(region);
                to.visible_strokes.extend(visible);
                let Content::Table(t) = &mut to.content else {
                    return Err(Error::Integrity);
                };
                t.layout
                    .as_mut()
                    .ok_or(Error::Integrity)?
                    .ink_origins
                    .extend(origins);
            }
            // Validate translated placements against the unchanged samples.
            if let Content::Table(t) = &to.content {
                if let Some(layout) = &t.layout {
                    for ink in &layout.ink_origins {
                        layout
                            .validate_source(t, &Self::original_stroke(db, to.id, ink.stroke)?)?;
                    }
                }
            }
            to.revision = to.revision.checked_add(1).ok_or(Error::ResourceLimit)?;
            if !same && mode == CellTransferMode::Move {
                from.revision = from.revision.checked_add(1).ok_or(Error::ResourceLimit)?;
                if !linked {
                    Self::persist_block(db, &from)?;
                }
            }
            if linked {
                let (_, first) = Self::journal_for_operation(db, source_owner, |db| {
                    Self::persist_block(db, &from)
                })?;
                let (_, second) = Self::journal_for_operation(db, target_owner, |db| {
                    Self::persist_block(db, &to)
                })?;
                db.execute(
                    "INSERT INTO edit_links(operation,peer) VALUES(?1,?2),(?2,?1)",
                    rusqlite::params![first, second],
                )?;
            } else {
                Self::persist_block(db, &to)?;
            }
            Ok(if same { vec![to] } else { vec![from, to] })
        };
        self.document_save_states
            .insert(target_owner, SaveState::Saving);
        if linked {
            self.document_save_states
                .insert(source_owner, SaveState::Saving);
        }
        let result = self.write(|db| {
            if linked {
                action(db)
            } else {
                Self::journal_for(db, target_owner, action)
            }
        });
        let state = if result.is_ok() {
            SaveState::SavedLocally
        } else {
            SaveState::NeedsAttention
        };
        self.document_save_states.insert(target_owner, state);
        if linked {
            self.document_save_states.insert(source_owner, state);
        }
        result
    }
}

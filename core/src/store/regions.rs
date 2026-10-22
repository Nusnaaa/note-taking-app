use super::*;
use crate::{content::Content, digest};
use std::collections::BTreeSet;
impl Store {
    pub fn partition_regions(
        &mut self,
        block: Id,
        revision: u64,
        groups: Vec<Vec<Id>>,
        confirmed: bool,
        access: &Access,
    ) -> Result<Block> {
        if !confirmed {
            return Err(Error::ReviewRequired);
        }
        self.edit_block(block, revision, access, |b| {
            if groups.is_empty() || groups.len() > 4096 {
                return Err(Error::Invalid("region partition"));
            }
            // Repartitioning accepted content is explicitly destructive; use split/merge
            // on unconverted regions or retain an explicit recoverable prior revision.
            if !matches!(b.content, Content::Ink) || b.regions.iter().any(|r| r.content.is_some()) {
                return Err(Error::ReviewRequired);
            }
            let given: Vec<_> = groups.iter().flatten().copied().collect();
            let set: BTreeSet<_> = given.iter().copied().collect();
            if set.len() != given.len() || set != b.visible_strokes.iter().copied().collect() {
                return Err(Error::Invalid("region partition"));
            }
            b.regions = groups
                .into_iter()
                .map(|strokes| Region {
                    id: Id::new(),
                    strokes,
                    content: None,
                })
                .collect();
            Ok(())
        })
    }
    pub fn split_region(
        &mut self,
        block: Id,
        revision: u64,
        region: Id,
        parts: Vec<Vec<Id>>,
        access: &Access,
    ) -> Result<Block> {
        self.edit_block(block, revision, access, |b| {
            let at = b
                .regions
                .iter()
                .position(|r| r.id == region)
                .ok_or(Error::Unavailable)?;
            let old = &b.regions[at];
            if old.content.is_some() {
                return Err(Error::ReviewRequired);
            }
            let selected: Vec<_> = parts.iter().flatten().copied().collect();
            if parts.len() < 2
                || parts.iter().any(Vec::is_empty)
                || selected.len() != old.strokes.len()
                || selected.iter().copied().collect::<BTreeSet<_>>()
                    != old.strokes.iter().copied().collect()
            {
                return Err(Error::Invalid("region split"));
            }
            b.regions.splice(
                at..=at,
                parts.into_iter().map(|strokes| Region {
                    id: Id::new(),
                    strokes,
                    content: None,
                }),
            );
            Ok(())
        })
    }
    pub fn merge_regions(
        &mut self,
        block: Id,
        revision: u64,
        regions: &[Id],
        access: &Access,
    ) -> Result<Block> {
        self.edit_block(block, revision, access, |b| {
            let ids: BTreeSet<_> = regions.iter().copied().collect();
            if ids.len() != regions.len() || ids.len() < 2 {
                return Err(Error::Invalid("merge selection"));
            }
            let selected: Vec<_> = b.regions.iter().filter(|r| ids.contains(&r.id)).collect();
            if selected.len() != ids.len() {
                return Err(Error::Unavailable);
            }
            if selected.iter().any(|r| r.content.is_some()) {
                return Err(Error::ReviewRequired);
            }
            let source: BTreeSet<_> = selected
                .iter()
                .flat_map(|r| r.strokes.iter().copied())
                .collect();
            let merged = Region {
                id: Id::new(),
                strokes: b
                    .source_ids
                    .iter()
                    .copied()
                    .filter(|s| source.contains(s))
                    .collect(),
                content: None,
            };
            let at = b
                .regions
                .iter()
                .position(|r| ids.contains(&r.id))
                .ok_or(Error::Unavailable)?;
            b.regions.retain(|r| !ids.contains(&r.id));
            b.regions.insert(at, merged);
            Ok(())
        })
    }
    pub fn region_snapshot(&self, block: Id, region: Id, access: &Access) -> Result<InkSnapshot> {
        Self::make_region_snapshot(&self.db, block, Some(region), access)
    }
    pub(crate) fn make_region_snapshot(
        db: &Connection,
        block: Id,
        region: Option<Id>,
        access: &Access,
    ) -> Result<InkSnapshot> {
        let mut snapshot = Self::make_snapshot(db, block, access)?;
        if let Some(id) = region {
            let b = Self::read_block(db, block, access)?;
            let region = b
                .regions
                .iter()
                .find(|r| r.id == id)
                .ok_or(Error::Unavailable)?;
            snapshot.strokes.retain(|s| region.strokes.contains(&s.id));
            if snapshot.strokes.is_empty() {
                return Err(Error::Invalid("empty region"));
            }
            snapshot.fingerprint = digest(&serde_json::to_vec(&(&b, &snapshot.strokes, id))?);
            snapshot.region = Some(id);
        }
        Ok(snapshot)
    }
    pub(crate) fn apply_region_content(
        block: &mut Block,
        region: Option<Id>,
        content: Content,
    ) -> Result<()> {
        if let Some(id) = region {
            block
                .regions
                .iter_mut()
                .find(|r| r.id == id)
                .ok_or(Error::Unavailable)?
                .content = Some(content);
        } else {
            if !block.regions.is_empty() {
                return Err(Error::ReviewRequired);
            }
            block.content = content;
        }
        block.validate()
    }
    pub fn edit_region(
        &mut self,
        block: Id,
        revision: u64,
        region: Id,
        content: Content,
        access: &Access,
    ) -> Result<Block> {
        self.edit_block(block, revision, access, |b| {
            Self::apply_region_content(b, Some(region), content)?;
            b.manually_corrected = true;
            Ok(())
        })
    }
}

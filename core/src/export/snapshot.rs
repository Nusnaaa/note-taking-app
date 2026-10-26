//! Authorized, bounded source capture. Rendering never calls the live Store.
use super::*;
use crate::{ink::Stroke, store::Layout};
use rusqlite::OptionalExtension;
use serde::Serialize;
use std::{
    io::{self, Write},
    sync::Arc,
};
pub(super) const BYTES: usize = 64 * 1024 * 1024;
pub struct DerivativeSource {
    pub(super) library: String,
    pub(super) generation: String,
    pub(super) scopes: BTreeSet<Scope>,
    pub(super) records: Vec<DerivativeBlock>,
    pub(super) crop: Option<Rect>,
    pub(super) warnings: Vec<String>,
    pub(super) selected_blocks: usize,
    pub(super) include_original_ink: bool,
}
pub(super) struct DerivativeBlock {
    pub block: Block,
    pub layout: Option<Layout>,
    pub ink: Vec<Stroke>,
    pub image: Option<Arc<(String, Vec<u8>)>>,
}
#[derive(Default)]
struct SnapshotBudget {
    used: usize,
}
impl SnapshotBudget {
    fn ensure(&self, bytes: usize) -> Result<()> {
        if bytes > BYTES.saturating_sub(self.used) {
            Err(Error::ResourceLimit)
        } else {
            Ok(())
        }
    }
    fn bytes(&mut self, bytes: usize) -> Result<()> {
        self.ensure(bytes)?;
        self.used += bytes;
        Ok(())
    }
    fn add(&mut self, value: &impl Serialize) -> Result<()> {
        struct Counter<'a>(&'a mut SnapshotBudget, bool);
        impl Write for Counter<'_> {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                if self.0.bytes(bytes.len()).is_err() {
                    self.1 = true;
                    return Err(io::Error::other("derivative snapshot budget"));
                }
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut counter = Counter(self, false);
        match serde_json::to_writer(&mut counter, value) {
            Ok(()) => Ok(()),
            Err(_) if counter.1 => Err(Error::ResourceLimit),
            Err(error) => Err(error.into()),
        }
    }
}
fn read_block(
    store: &Store,
    id: Id,
    budget: &mut SnapshotBudget,
    access: &Access,
) -> Result<Block> {
    Store::authorise_block_metadata(&store.db, id, access)?;
    let bytes: usize = store.db.query_row(
        "SELECT length(CAST(payload AS BLOB)) FROM blocks WHERE id=?1",
        [id.to_string()],
        |r| r.get(0),
    )?;
    budget.ensure(bytes)?;
    let block = store.block(id, access)?;
    budget.add(&block)?;
    Ok(block)
}
fn read_layout(store: &Store, id: Id) -> Result<Option<Layout>> {
    let json: Option<String> = store
        .db
        .query_row(
            "SELECT payload FROM layouts WHERE block=?1",
            [id.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    let layout: Option<Layout> = json.as_deref().map(serde_json::from_str).transpose()?;
    if let Some(layout) = &layout {
        layout.bounds.validate()?;
    }
    Ok(layout)
}
fn read_image(
    store: &Store,
    block: &Block,
    id: Id,
    budget: &mut SnapshotBudget,
    scopes: &mut BTreeSet<Scope>,
    access: &Access,
) -> Result<(String, Vec<u8>)> {
    let (owner, bytes): (String, usize) = store.db.query_row(
        "SELECT entity,length(bytes) FROM image_assets WHERE id=?1",
        [id.to_string()],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let owner = Id::parse(&owner)?;
    Store::authorise(&store.db, owner, access, false)?;
    if owner != block.entity {
        return Err(Error::Integrity);
    }
    scopes.extend(store.protection_scopes(owner, None)?);
    // Preserve the existing shared-asset policy without decoding unrelated bodies.
    let mut q=store.db.prepare("SELECT id FROM blocks WHERE entity=?1 AND json_extract(payload,'$.content.type')='Image' AND json_extract(payload,'$.content.value.asset')=?2 UNION SELECT b.id FROM history h JOIN blocks b ON h.block=b.id WHERE b.entity=?1 AND json_extract(h.payload,'$.content.type')='Image' AND json_extract(h.payload,'$.content.value.asset')=?2")?;
    for row in q.query_map(rusqlite::params![owner.to_string(), id.to_string()], |r| {
        r.get::<_, String>(0)
    })? {
        let id = Id::parse(&row?)?;
        Store::authorise_block_metadata(&store.db, id, access)?;
        scopes.extend(store.protection_scopes(owner, Some(id))?);
    }
    budget.ensure(bytes)?;
    let (mime, bytes, hash): (String, Vec<u8>, String) = store.db.query_row(
        "SELECT mime,bytes,digest FROM image_assets WHERE id=?1",
        [id.to_string()],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    if !matches!(mime.as_str(), "image/png" | "image/jpeg") || crate::digest(&bytes) != hash {
        return Err(Error::Integrity);
    }
    budget.bytes(bytes.len())?;
    budget.add(&mime)?;
    Ok((mime, bytes))
}
impl Store {
    pub(crate) fn derivative_source(
        &self,
        selection: Selection,
        include_original_ink: bool,
        access: &Access,
    ) -> Result<DerivativeSource> {
        self.derivative_source_authorised(selection, include_original_ink, access, None)
    }
    pub(crate) fn derivative_source_authorised(
        &self,
        selection: Selection,
        include_original_ink: bool,
        access: &Access,
        authority: Option<&crate::processing::WorkAuthority>,
    ) -> Result<DerivativeSource> {
        let check = || authority.map_or(Ok(()), |authority| authority.check());
        check()?;
        Self::require_app(&self.db, access)?;

        let generation = Self::setting(&self.db, "generation")?;

        let library = Self::setting(&self.db, "library_id")?;

        let mut scopes = BTreeSet::new();
        if Self::setting(&self.db, "app_locked")? == "true" {
            scopes.insert(Scope::App);
        }

        let mut warnings = Vec::new();

        let mut budget = SnapshotBudget::default();
        let mut blocks: Vec<Block> = Vec::new();

        let mut crop = None;

        match selection {
            Selection::Blocks(ids) => {
                if ids.len() > 10000 {
                    return Err(Error::ResourceLimit);
                }
                if ids.iter().collect::<BTreeSet<_>>().len() != ids.len() {
                    return Err(Error::Invalid("duplicate export block"));
                }

                for id in ids {
                    check()?;
                    blocks.push(read_block(self, id, &mut budget, access)?);
                }
            }

            Selection::Document(id) => {
                Self::authorise(&self.db, id, access, false)?;

                scopes.extend(self.protection_scopes(id, None)?);

                let mut q = self.db.prepare(
                    "SELECT id FROM blocks WHERE entity=?1 ORDER BY ordinal,id LIMIT 10001",
                )?;

                for row in q.query_map([id.to_string()], |r| r.get::<_, String>(0))? {
                    check()?;
                    blocks.push(read_block(self, Id::parse(&row?)?, &mut budget, access)?);
                }

                if self
                    .db
                    .prepare("SELECT 1 FROM embeds WHERE host=?1")?
                    .exists([id.to_string()])?
                {
                    return Err(Error::Unsupported);
                }
            }

            Selection::Exercise(id) => {
                let e = self.exercise(id, access)?;
                scopes.extend(self.protection_scopes(e.master, None)?);

                if !e.visible {
                    return Err(Error::Unavailable);
                }

                if e.locked {
                    scopes.insert(Scope::Exercise(id));
                }

                let mut q = self.db.prepare(
                    "SELECT b.id FROM blocks b JOIN exercise_members m ON b.id=m.block WHERE \
                m.exercise=?1 ORDER BY b.ordinal,b.id LIMIT 10001",
                )?;

                for row in q.query_map([id.to_string()], |r| r.get::<_, String>(0))? {
                    check()?;
                    blocks.push(read_block(self, Id::parse(&row?)?, &mut budget, access)?);
                }
            }

            Selection::CanvasRegion { canvas, bounds } => {
                // Clipping an SVG does not remove off-screen coordinates from its
                // file. Require whole-block scope for original-ink derivatives.
                if include_original_ink {
                    return Err(Error::Unsupported);
                }

                bounds.validate()?;

                if Self::authorise(&self.db, canvas, access, false)?.kind != Kind::Canvas {
                    return Err(Error::Invalid("canvas scope"));
                }

                scopes.extend(self.protection_scopes(canvas, None)?);
                crop = Some(bounds);

                let mut q = self.db.prepare(
                    "SELECT id FROM blocks WHERE entity=?1 ORDER BY ordinal,id LIMIT 10001",
                )?;

                for row in q.query_map([canvas.to_string()], |r| r.get::<_, String>(0))? {
                    check()?;
                    let id = Id::parse(&row?)?;

                    Self::authorise_block_metadata(&self.db, id, access)?;
                    let layout = read_layout(self, id)?.ok_or(Error::ReviewRequired)?;

                    let r = layout.bounds;

                    if r.x < bounds.x + bounds.width
                        && r.x + r.width > bounds.x
                        && r.y < bounds.y + bounds.height
                        && r.y + r.height > bounds.y
                    {
                        if r.x < bounds.x
                            || r.y < bounds.y
                            || r.x + r.width > bounds.x + bounds.width
                            || r.y + r.height > bounds.y + bounds.height
                        {
                            return Err(Error::ReviewRequired);
                        }

                        blocks.push(read_block(self, id, &mut budget, access)?);
                    }
                }
                if self
                    .db
                    .prepare("SELECT 1 FROM embeds WHERE host=?1")?
                    .exists([canvas.to_string()])?
                {
                    return Err(Error::Unsupported);
                }
            }
        }

        if blocks.len() > 10000 {
            return Err(Error::ResourceLimit);
        }

        let selected_blocks = blocks.len();
        let mut layer_states = BTreeMap::new();
        for entity in blocks.iter().map(|b| b.entity).collect::<BTreeSet<_>>() {
            check()?;
            if Self::raw_entity(&self.db, entity)?.kind == Kind::Canvas {
                let state = Self::raw_canvas_layers(&self.db, entity)?;
                budget.add(&state)?;
                layer_states.insert(entity, (state.base_visible, state.index()));
            }
        }
        let mut records = Vec::new();
        let mut images = BTreeMap::new();
        for b in blocks {
            check()?;
            scopes.extend(self.protection_scopes(b.entity, Some(b.id))?);
            if let Some((base, index)) = layer_states.get(&b.entity) {
                if !index.get(&b.id).map_or(*base, |(_, _, visible)| *visible) {
                    warnings.push("Hidden canvas layer omitted from derivative export; native source remains retained.".into());
                    continue;
                }
            }
            if !b.visible {
                warnings.push("Hidden block omitted from derivative export.".into());
                continue;
            }
            let layout = read_layout(self, b.id)?;
            if let Some(layout) = &layout {
                budget.add(layout)?;
            }
            let mut ink = Vec::new();
            if include_original_ink {
                for id in &b.source_ids {
                    check()?;
                    if !b.visible_strokes.contains(id) {
                        continue;
                    }
                    let bytes: usize = self.db.query_row("SELECT length(CAST(payload AS BLOB)) FROM strokes WHERE id=?1 AND block=?2", rusqlite::params![id.to_string(), b.id.to_string()], |r| r.get(0))?;
                    budget.ensure(bytes)?;
                    let stroke = Self::scene_stroke(&self.db, b.id, *id)?;
                    budget.add(&stroke)?;
                    ink.push(stroke);
                }
            }
            let image = if let Content::Image(image) = &b.content {
                if let Some(value) = images.get(&image.asset) {
                    Some(Arc::clone(value))
                } else {
                    let value = Arc::new(read_image(
                        self,
                        &b,
                        image.asset,
                        &mut budget,
                        &mut scopes,
                        access,
                    )?);
                    images.insert(image.asset, Arc::clone(&value));
                    Some(value)
                }
            } else {
                None
            };
            records.push(DerivativeBlock {
                block: b,
                layout,
                ink,
                image,
            });
        }
        if Self::setting(&self.db, "generation")? != generation {
            return Err(Error::Conflict);
        }
        check()?;
        Ok(DerivativeSource {
            library,
            generation,
            scopes,
            records,
            crop,
            warnings,
            selected_blocks,
            include_original_ink,
        })
    }
}

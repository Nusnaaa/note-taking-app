//! Canvas presentation layers never move or rewrite original source observations.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum LayerRef {
    Base,
    Custom(Id),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanvasLayer {
    pub id: Id,
    pub name: String,
    pub visible: bool,
    pub blocks: Vec<Id>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanvasLayers {
    pub canvas: Id,
    pub revision: u64,
    pub base_visible: bool,
    pub layers: Vec<CanvasLayer>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanvasSceneItem {
    pub layer: LayerRef,
    pub item: CanvasSceneContent,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum CanvasSceneContent {
    Block {
        item: SurfaceItem,
        layout: Option<Layout>,
    },
    Embed(Embed),
    ProtectedEmbed(Id),
    UnavailableEmbed(Id),
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanvasScene {
    pub canvas: Id,
    pub layer_revision: u64,
    pub entity_revision: u64,
    pub embed_revision: Option<u64>,
    pub next_offset: Option<u32>,
    pub unresolved: Vec<LayerRef>,
    pub items: Vec<CanvasSceneItem>,
}
impl CanvasLayers {
    pub(crate) fn implicit(canvas: Id) -> Self {
        Self {
            canvas,
            revision: 0,
            base_visible: true,
            layers: vec![],
        }
    }
    pub(crate) fn validate(&self) -> Result<()> {
        let mut ids = BTreeSet::new();
        let mut blocks = BTreeSet::new();
        if self.revision > i64::MAX as u64 || self.layers.len() > 128 {
            return Err(Error::ResourceLimit);
        }
        for layer in &self.layers {
            if layer.name.is_empty()
                || layer.name.len() > 1024
                || !ids.insert(layer.id)
                || layer.blocks.iter().any(|b| !blocks.insert(*b))
            {
                return Err(Error::Invalid("canvas layers"));
            }
            if blocks.len() > 100_000 {
                return Err(Error::ResourceLimit);
            }
        }
        Ok(())
    }
    pub(crate) fn index(&self) -> BTreeMap<Id, (usize, LayerRef, bool)> {
        self.layers
            .iter()
            .enumerate()
            .flat_map(|(i, l)| {
                l.blocks
                    .iter()
                    .map(move |b| (*b, (i + 1, LayerRef::Custom(l.id), l.visible)))
            })
            .collect()
    }
    pub(crate) fn contains(&self, r: LayerRef) -> bool {
        match r {
            LayerRef::Base => true,
            LayerRef::Custom(id) => self.layers.iter().any(|l| l.id == id),
        }
    }
}
impl Store {
    pub(crate) fn raw_canvas_layers(db: &Connection, canvas: Id) -> Result<CanvasLayers> {
        let json: Option<String> = db
            .query_row(
                "SELECT payload FROM canvas_layers WHERE canvas=?1",
                [canvas.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        let state = json
            .as_deref()
            .map(serde_json::from_str::<CanvasLayers>)
            .transpose()?
            .unwrap_or_else(|| CanvasLayers::implicit(canvas));
        state.validate()?;
        if state.canvas != canvas {
            return Err(Error::Integrity);
        }
        Ok(state)
    }
    pub(crate) fn check_canvas(db: &Connection, canvas: Id, access: &Access) -> Result<()> {
        if Self::authorise(db, canvas, access, false)?.kind != Kind::Canvas {
            return Err(Error::Invalid("canvas layers owner"));
        }
        Ok(())
    }
    pub(crate) fn check_layer_references(
        db: &Connection,
        state: &CanvasLayers,
        access: &Access,
    ) -> Result<()> {
        state.validate()?;
        Self::check_canvas(db, state.canvas, access)?;
        for layer in &state.layers {
            let alias=db.prepare("SELECT 1 FROM entities WHERE id=?1 UNION ALL SELECT 1 FROM blocks WHERE id=?1 UNION ALL SELECT 1 FROM strokes WHERE id=?1 UNION ALL SELECT 1 FROM view_settings WHERE id=?1 UNION ALL SELECT 1 FROM sources WHERE id=?1 UNION ALL SELECT 1 FROM object_groups WHERE id=?1 UNION ALL SELECT 1 FROM embeds WHERE id=?1 UNION ALL SELECT 1 FROM workspace_panes WHERE slot=?1 UNION ALL SELECT 1 FROM image_assets WHERE id=?1 UNION ALL SELECT 1 FROM exercises WHERE id=?1 UNION ALL SELECT 1 FROM links WHERE id=?1 UNION ALL SELECT 1 FROM pdf_anchors WHERE id=?1 UNION ALL SELECT 1 FROM engine_versions WHERE id=?1 UNION ALL SELECT 1 FROM blocks b,json_each(b.payload,'$.regions') r WHERE json_extract(r.value,'$.id')=?1 UNION ALL SELECT 1 FROM history h,json_each(h.payload,'$.regions') r WHERE json_extract(r.value,'$.id')=?1")?.exists([layer.id.to_string()])?;
            if alias {
                return Err(Error::Invalid("canvas layer identity"));
            }
            if db.prepare("SELECT 1 FROM canvas_layers c,json_each(c.payload,'$.layers') l WHERE c.canvas<>?1 AND json_extract(l.value,'$.id')=?2")?.exists(rusqlite::params![state.canvas.to_string(),layer.id.to_string()])? {return Err(Error::Invalid("canvas layer identity"))}
            for block in &layer.blocks {
                if Self::read_block(db, *block, access)?.entity != state.canvas {
                    return Err(Error::Invalid("canvas layer member"));
                }
            }
        }
        Ok(())
    }
    pub fn canvas_layers(&self, canvas: Id, access: &Access) -> Result<CanvasLayers> {
        Self::check_canvas(&self.db, canvas, access)?;
        Self::require_content_access(&self.db, canvas, access)?;
        Self::raw_canvas_layers(&self.db, canvas)
    }
    pub fn set_canvas_layers(
        &mut self,
        mut state: CanvasLayers,
        expected: u64,
        access: &Access,
    ) -> Result<CanvasLayers> {
        state.validate()?;
        let canvas = state.canvas;
        self.write_for(canvas,|db| {
   Self::check_canvas(db,canvas,access)?;Self::require_content_access(db,canvas,access)?;
   let previous=Self::raw_canvas_layers(db,canvas)?;if previous.revision!=expected {return Err(Error::Conflict)}
   Self::check_layer_references(db,&state,access)?;
   state.revision=expected.checked_add(1).filter(|r|*r<=i64::MAX as u64).ok_or(Error::ResourceLimit)?;
   db.execute("INSERT INTO canvas_layers VALUES(?1,?2) ON CONFLICT(canvas) DO UPDATE SET payload=excluded.payload",rusqlite::params![canvas.to_string(),serde_json::to_string(&state)?])?;
   Self::touch_master(db,canvas)?;
   Ok(state)
  })
    }
    pub fn set_embed_layers(
        &mut self,
        embed: Id,
        expected: u64,
        visible: Option<Vec<LayerRef>>,
        access: &Access,
    ) -> Result<Embed> {
        if visible
            .as_ref()
            .is_some_and(|v| v.len() > 129 || v.iter().collect::<BTreeSet<_>>().len() != v.len())
        {
            return Err(Error::Invalid("embed layers"));
        }
        let old = self.embed(embed, access)?;
        self.write_for(old.host, |db| {
            let mut value = Self::read_embed(db, embed, access)?;
            if value.revision != expected {
                return Err(Error::Conflict);
            }
            let state = Self::raw_canvas_layers(db, value.source)?;
            if visible
                .as_ref()
                .is_some_and(|v| v.iter().any(|r| !state.contains(*r)))
            {
                return Err(Error::Invalid("embed layer source"));
            }
            value.visible_layers = visible;
            value.revision = value
                .revision
                .checked_add(1)
                .filter(|r| *r <= i64::MAX as u64)
                .ok_or(Error::ResourceLimit)?;
            db.execute(
                "UPDATE embeds SET payload=?1 WHERE id=?2",
                rusqlite::params![serde_json::to_string(&value)?, embed.to_string()],
            )?;
            Self::touch_master(db, value.host)?;
            Ok(value)
        })
    }
    pub fn canvas_scene(
        &self,
        canvas: Id,
        embed: Option<Id>,
        offset: u32,
        limit: u32,
        access: &Access,
    ) -> Result<CanvasScene> {
        Self::check_canvas(&self.db, canvas, access)?;
        if limit > 512 {
            return Err(Error::ResourceLimit);
        }
        let state = Self::raw_canvas_layers(&self.db, canvas)?;
        let index = state.index();
        let mut embed_revision = None;
        let visible = if let Some(id) = embed {
            let e = Self::read_embed(&self.db, id, access)?;
            if e.source != canvas {
                return Err(Error::Invalid("scene embed source"));
            }
            embed_revision = Some(e.revision);
            e.visible_layers
        } else {
            None
        };
        let unresolved = visible
            .as_ref()
            .map(|v| v.iter().filter(|r| !state.contains(**r)).copied().collect())
            .unwrap_or_default();
        let mut q=self.db.prepare("SELECT 0,b.id,b.ordinal,COALESCE(json_extract(l.payload,'$.z'),0),length(CAST(b.payload AS BLOB)) FROM blocks b LEFT JOIN layouts l ON l.block=b.id WHERE b.entity=?1 AND json_extract(b.payload,'$.visible')=1 UNION ALL SELECT 1,e.id,e.rowid,0,length(CAST(e.payload AS BLOB)) FROM embeds e WHERE e.host=?1 LIMIT 100001")?;
        let mut ordered = vec![];
        for row in q.query_map([canvas.to_string()], |r| {
            Ok((
                r.get::<_, u8>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i32>(3)?,
                r.get::<_, usize>(4)?,
            ))
        })? {
            let (kind, id, ordinal, z, bytes) = row?;
            let id = Id::parse(&id)?;
            // Nested embeds are base-layer objects. Their own source-layer
            // override remains independent and never mutates this canvas.
            let (rank, layer, default) = if kind == 0 {
                index
                    .get(&id)
                    .copied()
                    .unwrap_or((0, LayerRef::Base, state.base_visible))
            } else {
                (0, LayerRef::Base, state.base_visible)
            };
            if visible.as_ref().map_or(default, |v| v.contains(&layer)) {
                ordered.push((rank, z, ordinal, kind, id, layer, bytes));
            }
        }
        let total:i64=self.db.query_row("SELECT (SELECT COUNT(*) FROM blocks WHERE entity=?1)+(SELECT COUNT(*) FROM embeds WHERE host=?1)",[canvas.to_string()],|r|r.get(0))?;
        if total > 100_000 {
            return Err(Error::ResourceLimit);
        }
        ordered.sort_by_key(|(rank, z, ordinal, kind, id, _, _)| (*rank, *z, *kind, *ordinal, *id));
        let available = ordered.len();
        let mut items = vec![];
        let mut used = 16_384usize;
        const PAGE_BYTES: usize = 8 * 1024 * 1024;
        for (_, _, _, kind, id, layer, bytes) in ordered
            .into_iter()
            .skip(offset as usize)
            .take(limit as usize)
        {
            let permission = if kind == 0 {
                Self::authorise_block_metadata(&self.db, id, access)
            } else {
                Ok(())
            };
            let protected = matches!(permission, Err(Error::Denied));
            if !protected {
                permission?;
            }
            // Never parse an unbounded row before applying the page budget.
            if !protected && bytes > PAGE_BYTES - used {
                if items.is_empty() {
                    return Err(Error::ResourceLimit);
                }
                break;
            }
            let item = if protected {
                CanvasSceneContent::Block {
                    item: SurfaceItem::Protected {
                        block: id,
                        exercise: None,
                    },
                    layout: None,
                }
            } else if kind == 0 {
                CanvasSceneContent::Block {
                    item: SurfaceItem::Visible(Box::new(Self::read_block(&self.db, id, access)?)),
                    layout: self.layout(id, access)?,
                }
            } else {
                match Self::read_embed(&self.db, id, access) {
                    Ok(e) => CanvasSceneContent::Embed(e),
                    Err(Error::Denied) => CanvasSceneContent::ProtectedEmbed(id),
                    Err(Error::Unavailable) => CanvasSceneContent::UnavailableEmbed(id),
                    Err(e) => return Err(e),
                }
            };
            let item = CanvasSceneItem { layer, item };
            let bytes = serde_json::to_vec(&item)?.len();
            if bytes > PAGE_BYTES - used {
                if items.is_empty() {
                    return Err(Error::ResourceLimit);
                }
                break;
            }
            used += bytes;
            items.push(item);
        }
        let next = offset as usize + items.len();
        let next_offset = if !items.is_empty() && next < available {
            Some(next as u32)
        } else {
            None
        };
        Ok(CanvasScene {
            canvas,
            layer_revision: state.revision,
            entity_revision: Self::raw_entity(&self.db, canvas)?.revision,
            embed_revision,
            next_offset,
            unresolved,
            items,
        })
    }
}

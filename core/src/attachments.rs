//! Native v3 attachments. No executable code, remote locators or model resources.
use crate::{
    content::Content,
    digest,
    objects::Image,
    portable::NativeDocument,
    store::{CanvasLayers, Embed, LayerRef, Layout, ObjectGroup, ViewSettings},
    Error, Id, Result,
};

use serde::{Deserialize, Serialize};

use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeAsset {
    pub id: Id,
    pub entity: Id,
    pub mime: String,
    pub bytes: Vec<u8>,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeLayout {
    pub block: Id,
    pub layout: Layout,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attachments {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<CanvasLayers>,
    pub assets: Vec<NativeAsset>,
    pub layouts: Vec<NativeLayout>,
    pub groups: Vec<ObjectGroup>,
    pub embeds: Vec<Embed>,
    pub views: Vec<ViewSettings>,
}

impl Attachments {
    pub fn validate(&self, doc: &NativeDocument) -> Result<()> {
        let entities: BTreeMap<_, _> = doc.entities.iter().map(|e| (e.id, e)).collect();

        let blocks: BTreeMap<_, _> = doc
            .blocks
            .iter()
            .map(|b| (b.current.id, &b.current))
            .collect();

        let exercises: BTreeMap<_, _> = doc.exercises.iter().map(|e| (e.id, e)).collect();

        let mut ids: BTreeSet<_> = entities
            .keys()
            .chain(blocks.keys())
            .chain(exercises.keys())
            .copied()
            .collect();

        ids.extend(doc.links.iter().map(|l| l.id));

        ids.extend(
            doc.blocks
                .iter()
                .flat_map(|b| b.strokes.iter().map(|s| s.id)),
        );

        let mut layer_owners = BTreeSet::new();
        for state in &self.layers {
            state.validate()?;
            if !layer_owners.insert(state.canvas)
                || !entities
                    .get(&state.canvas)
                    .is_some_and(|e| e.kind == crate::store::Kind::Canvas)
            {
                return Err(Error::Invalid("native canvas layers"));
            }
            for layer in &state.layers {
                if !ids.insert(layer.id)
                    || layer
                        .blocks
                        .iter()
                        .any(|b| blocks.get(b).is_none_or(|b| b.entity != state.canvas))
                {
                    return Err(Error::Invalid("native layer member"));
                }
            }
        }
        let mut assets = BTreeMap::new();

        let mut bytes = 0usize;

        for a in &self.assets {
            bytes = bytes
                .checked_add(a.bytes.len())
                .ok_or(Error::ResourceLimit)?;

            if bytes > 64 * 1024 * 1024 {
                return Err(Error::ResourceLimit);
            }

            if !ids.insert(a.id)
                || !entities.get(&a.entity).is_some_and(|e| e.kind.is_surface())
                || digest(&a.bytes) != a.digest
                || !match a.mime.as_str() {
                    "image/png" => a.bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
                    "image/jpeg" => a.bytes.starts_with(b"\xff\xd8\xff"),
                    _ => false,
                }
            {
                return Err(Error::Invalid("native image asset"));
            }

            assets.insert(a.id, a);
        }

        for b in &doc.blocks {
            for content in std::iter::once(&b.current.content)
                .chain(b.history.iter().map(|h| &h.block.content))
            {
                if let Content::Image(Image { asset, .. }) = content {
                    if assets
                        .get(asset)
                        .is_none_or(|a| a.entity != b.current.entity)
                    {
                        return Err(Error::Invalid("native image reference"));
                    }
                }
            }
        }

        let mut layouts = BTreeSet::new();

        for l in &self.layouts {
            if !blocks.contains_key(&l.block)
                || !layouts.insert(l.block)
                || l.layout.revision > i64::MAX as u64
            {
                return Err(Error::Invalid("native layout"));
            }

            l.layout.bounds.validate()?;
        }

        for g in &self.groups {
            if !ids.insert(g.id)
                || g.members.is_empty()
                || g.members.len() > 4096
                || g.revision > i64::MAX as u64
                || g.members.iter().collect::<BTreeSet<_>>().len() != g.members.len()
                || g.members
                    .iter()
                    .any(|b| blocks.get(b).is_none_or(|b| b.entity != g.entity))
            {
                return Err(Error::Invalid("native group"));
            }
        }

        for e in &self.embeds {
            if e.visible_layers.as_ref().is_some_and(|v| {
                v.len() > 129 || v.iter().collect::<BTreeSet<_>>().len() != v.len()
            }) {
                return Err(Error::Invalid("native embed layers"));
            }
            e.camera.validate()?;

            e.frame.validate()?;

            if !ids.insert(e.id)
                || e.revision > i64::MAX as u64
                || !entities.get(&e.host).is_some_and(|x| x.kind.is_surface())
                || !entities
                    .get(&e.source)
                    .is_some_and(|x| x.kind == crate::store::Kind::Canvas)
            {
                return Err(Error::Invalid("native embed"));
            }

            let mut todo = vec![e.source];

            let mut visited = BTreeSet::new();

            while let Some(id) = todo.pop() {
                if id == e.host {
                    return Err(Error::Invalid("native embed cycle"));
                }

                if visited.insert(id) {
                    todo.extend(
                        self.embeds
                            .iter()
                            .filter(|next| next.host == id)
                            .map(|next| next.source),
                    );
                }
            }
        }

        for v in &self.views {
            v.validate()?;

            if !ids.insert(v.id)
                || v.revision > i64::MAX as u64
                || !entities.contains_key(&v.entity)
                || v.exercise
                    .is_some_and(|e| exercises.get(&e).is_none_or(|e| e.master != v.entity))
                || v.anchor
                    .is_some_and(|b| blocks.get(&b).is_none_or(|b| b.entity != v.entity))
            {
                return Err(Error::Invalid("native view"));
            }

            if let Some(bookmark) = &v.bookmark {
                for point in bookmark.points() {
                    let block = blocks
                        .get(&point.block)
                        .ok_or(Error::Invalid("native caret block"))?;
                    if block.entity != v.entity {
                        return Err(Error::Invalid("native caret scope"));
                    }
                    point.validate_block(block, |stroke| {
                        doc.blocks
                            .iter()
                            .flat_map(|b| &b.strokes)
                            .find(|s| s.id == stroke)
                            .map(|s| s.points.len())
                            .ok_or(Error::Invalid("native caret stroke"))
                    })?;
                }
            }
            if let (Some(e), Some(b)) = (v.exercise, v.anchor) {
                if !doc
                    .memberships
                    .iter()
                    .any(|m| m.block == b && m.exercise == e)
                {
                    return Err(Error::Invalid("native view membership"));
                }
            }
        }

        Ok(())
    }

    pub(crate) fn add_ids(&self, ids: &mut BTreeMap<Id, Id>) {
        for state in &self.layers {
            for layer in &state.layers {
                ids.entry(layer.id).or_default();
            }
        }
        for e in &self.embeds {
            for layer in e.visible_layers.iter().flatten() {
                if let LayerRef::Custom(id) = layer {
                    ids.entry(*id).or_default();
                }
            }
        }

        for bookmark in self.views.iter().filter_map(|v| v.bookmark.as_ref()) {
            for point in bookmark.points() {
                if let Some(region) = point.region {
                    ids.entry(region).or_default();
                }
            }
        }

        for id in self
            .assets
            .iter()
            .map(|a| a.id)
            .chain(self.groups.iter().map(|g| g.id))
            .chain(self.embeds.iter().map(|e| e.id))
            .chain(self.views.iter().map(|v| v.id))
        {
            ids.insert(id, Id::new());
        }
    }

    pub(crate) fn remap(&mut self, ids: &BTreeMap<Id, Id>) {
        for state in &mut self.layers {
            state.canvas = ids[&state.canvas];
            for layer in &mut state.layers {
                layer.id = ids[&layer.id];
                layer.blocks = layer.blocks.iter().map(|b| ids[b]).collect();
            }
        }

        for a in &mut self.assets {
            a.id = ids[&a.id];

            a.entity = ids[&a.entity];
        }

        for l in &mut self.layouts {
            l.block = ids[&l.block];
        }

        for g in &mut self.groups {
            g.id = ids[&g.id];

            g.entity = ids[&g.entity];

            g.members = g.members.iter().map(|b| ids[b]).collect();
        }

        for e in &mut self.embeds {
            e.id = ids[&e.id];

            e.host = ids[&e.host];

            e.source = ids[&e.source];
            for layer in e.visible_layers.iter_mut().flatten() {
                if let LayerRef::Custom(id) = layer {
                    *id = ids[id];
                }
            }
        }

        for v in &mut self.views {
            v.id = ids[&v.id];

            v.entity = ids[&v.entity];

            v.exercise = v.exercise.map(|e| ids[&e]);

            v.anchor = v.anchor.map(|b| ids[&b]);
            if let Some(bookmark) = &mut v.bookmark {
                bookmark.remap(ids);
            }
        }
    }
}

use crate::{security::Access, store::Store};

use rusqlite::{Connection, OptionalExtension};

impl Store {
    pub(crate) fn export_attachments(
        &self,
        doc: &mut NativeDocument,
        exercise: Option<Id>,
        access: &Access,
    ) -> Result<Attachments> {
        let mut out = Attachments::default();

        let selected: BTreeSet<_> = doc.entities.iter().map(|e| e.id).collect();

        let blocks: BTreeSet<_> = doc.blocks.iter().map(|b| b.current.id).collect();

        let mut assets = BTreeSet::new();

        for b in &doc.blocks {
            for content in std::iter::once(&b.current.content)
                .chain(b.history.iter().map(|h| &h.block.content))
            {
                if let Content::Image(i) = content {
                    assets.insert(i.asset);
                }
            }

            if let Some(layout) = self.layout(b.current.id, access)? {
                out.layouts.push(NativeLayout {
                    block: b.current.id,
                    layout,
                });
            }
        }

        for id in assets {
            let (mime, bytes) = self.image_bytes(id, access)?;

            let owner: String = self.db.query_row(
                "SELECT entity FROM image_assets WHERE id=?1",
                [id.to_string()],
                |r| r.get(0),
            )?;

            out.assets.push(NativeAsset {
                id,
                entity: Id::parse(&owner)?,
                mime,
                digest: digest(&bytes),
                bytes,
            });
        }

        for &entity in &selected {
            if Self::raw_entity(&self.db, entity)?.kind == crate::store::Kind::Canvas {
                let row: Option<String> = self
                    .db
                    .query_row(
                        "SELECT payload FROM canvas_layers WHERE canvas=?1",
                        [entity.to_string()],
                        |r| r.get(0),
                    )
                    .optional()?;
                if let Some(row) = row {
                    let mut state: CanvasLayers = serde_json::from_str(&row)?;
                    if exercise.is_some() {
                        for layer in &mut state.layers {
                            layer.blocks.retain(|b| blocks.contains(b));
                        }
                        state.layers.retain(|l| !l.blocks.is_empty());
                    }
                    out.layers.push(state);
                }
            }

            let mut q = self
                .db
                .prepare("SELECT payload FROM object_groups WHERE entity=?1")?;

            for row in q.query_map([entity.to_string()], |r| r.get::<_, String>(0))? {
                let g: ObjectGroup = serde_json::from_str(&row?)?;

                if g.members.iter().all(|b| blocks.contains(b)) {
                    self.group(g.id, access)?;

                    out.groups.push(g);
                } else if g.members.iter().any(|b| blocks.contains(b)) {
                    doc.omissions.push("A group crossing the selected boundary is omitted; selected working is retained.".into());
                }
            }

            let mut q = self.db.prepare("SELECT id FROM embeds WHERE host=?1")?;

            for row in q.query_map([entity.to_string()], |r| r.get::<_, String>(0))? {
                if exercise.is_some() {
                    doc.omissions.push(
                        "Master-level canvas embeds are outside this exercise selection.".into(),
                    );

                    continue;
                }

                let e = self.embed(Id::parse(&row?)?, access)?;

                if !selected.contains(&e.source) {
                    return Err(Error::ReviewRequired);
                }

                out.embeds.push(e);
            }

            let mut q = self
                .db
                .prepare("SELECT payload FROM view_settings WHERE entity=?1")?;

            for row in q.query_map([entity.to_string()], |r| r.get::<_, String>(0))? {
                let mut v: ViewSettings = serde_json::from_str(&row?)?;

                if exercise.is_some() && v.exercise != exercise {
                    continue;
                }

                if v.anchor.is_some_and(|b| !blocks.contains(&b))
                    || v.bookmark.as_ref().is_some_and(|bookmark| {
                        bookmark.points().iter().any(|p| !blocks.contains(&p.block))
                    })
                {
                    continue;
                }

                self.view(v.id, access)?;

                if exercise.is_some() {
                    v.exercise = None;
                }

                out.views.push(v);
            }
        }

        Ok(out)
    }

    pub(crate) fn import_assets(db: &Connection, a: &Attachments) -> Result<()> {
        for image in &a.assets {
            db.execute(
                "INSERT INTO image_assets VALUES(?1,?2,?3,?4,?5)",
                rusqlite::params![
                    image.id.to_string(),
                    image.entity.to_string(),
                    image.mime,
                    image.bytes,
                    image.digest
                ],
            )?;
        }

        Ok(())
    }

    pub(crate) fn import_attachment_relations(db: &Connection, a: &Attachments) -> Result<()> {
        for state in &a.layers {
            db.execute(
                "INSERT INTO canvas_layers VALUES(?1,?2)",
                rusqlite::params![state.canvas.to_string(), serde_json::to_string(state)?],
            )?;
        }

        for l in &a.layouts {
            db.execute(
                "INSERT INTO layouts VALUES(?1,?2)",
                rusqlite::params![l.block.to_string(), serde_json::to_string(&l.layout)?],
            )?;
        }

        for g in &a.groups {
            db.execute(
                "INSERT INTO object_groups VALUES(?1,?2,?3)",
                rusqlite::params![
                    g.id.to_string(),
                    g.entity.to_string(),
                    serde_json::to_string(g)?
                ],
            )?;
        }

        for e in &a.embeds {
            db.execute(
                "INSERT INTO embeds VALUES(?1,?2,?3,?4)",
                rusqlite::params![
                    e.id.to_string(),
                    e.host.to_string(),
                    e.source.to_string(),
                    serde_json::to_string(e)?
                ],
            )?;
        }

        for v in &a.views {
            db.execute(
                "INSERT INTO view_settings VALUES(?1,?2,?3)",
                rusqlite::params![
                    v.id.to_string(),
                    v.entity.to_string(),
                    serde_json::to_string(v)?
                ],
            )?;
        }

        Ok(())
    }
}

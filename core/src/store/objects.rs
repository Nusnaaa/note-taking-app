//! Technical-object persistence, explicit layout and independent view state.
use super::*;

use crate::{
    content::Content,
    digest,
    ink::Transform,
    objects::{CellValue, DiagramPart, Rect},
};

use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    pub bounds: Rect,
    pub z: i32,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectGroup {
    pub id: Id,
    pub entity: Id,
    pub members: Vec<Id>,
    pub revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Camera {
    pub centre_x: f64,
    pub centre_y: f64,
    pub zoom: f64,
    pub rotation_rad: f64,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            centre_x: 0.,
            centre_y: 0.,
            zoom: 1.,
            rotation_rad: 0.,
        }
    }
}

impl Camera {
    pub fn validate(self) -> Result<()> {
        if [self.centre_x, self.centre_y, self.zoom, self.rotation_rad]
            .iter()
            .any(|n| !n.is_finite())
            || self.zoom <= 0.
        {
            Err(Error::Invalid("camera"))
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Embed {
    pub id: Id,
    pub host: Id,
    pub source: Id,
    pub camera: Camera,
    pub frame: Rect,
    pub pinned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible_layers: Option<Vec<LayerRef>>,
    pub revision: u64,
}

impl Embed {
    pub(crate) fn validate_layers(&self) -> Result<()> {
        if self
            .visible_layers
            .as_ref()
            .is_some_and(|v| v.len() > 129 || v.iter().collect::<BTreeSet<_>>().len() != v.len())
        {
            return Err(Error::Invalid("embed layers"));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewSettings {
    pub id: Id,
    pub entity: Id,
    pub exercise: Option<Id>,
    pub camera: Camera,
    pub pen_width_mm: f64,
    pub colour_rgba: [u8; 4],
    pub finger_ink: bool,
    pub anchor: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bookmark: Option<SelectionBookmark>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authoring: Option<Box<AuthoringSettings>>,
    pub revision: u64,
}

impl ViewSettings {
    pub fn validate(&self) -> Result<()> {
        self.camera.validate()?;
        if let Some(a) = &self.authoring {
            a.validate()?;
            if a.selected().is_some_and(|p| {
                p.width_mm != self.pen_width_mm || p.colour_rgba != self.colour_rgba
            }) {
                return Err(Error::Invalid("selected preset appearance"));
            }
        }
        if let Some(bookmark) = &self.bookmark {
            bookmark.validate_shape()?;
        }
        if self.revision > i64::MAX as u64 {
            return Err(Error::Invalid("view revision"));
        }
        if !self.pen_width_mm.is_finite() || self.pen_width_mm <= 0. || self.pen_width_mm > 100. {
            Err(Error::Invalid("pen width"))
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SurfaceItem {
    Visible(Box<Block>),
    Protected { block: Id, exercise: Option<Id> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct MasterLocation {
    pub master: Id,
    pub block: Option<Id>,
}

impl Store {
    pub(crate) fn validate_asset_references(
        db: &Connection,
        owner: Id,
        content: &Content,
    ) -> Result<()> {
        if let Content::Image(image) = content {
            let entity: Option<String> = db
                .query_row(
                    "SELECT entity FROM image_assets WHERE id=?1",
                    [image.asset.to_string()],
                    |r| r.get(0),
                )
                .optional()?;

            if entity.as_deref() != Some(&owner.to_string()) {
                return Err(Error::Invalid("image asset owner"));
            }
        }

        Ok(())
    }

    /// Caller has deliberately selected local image bytes; no URL is fetched.
    pub fn import_image(
        &mut self,
        entity: Id,
        mime: &str,
        bytes: &[u8],
        consented: bool,
        access: &Access,
    ) -> Result<Id> {
        if !consented {
            return Err(Error::Denied);
        }

        if bytes.len() > 32 * 1024 * 1024 {
            return Err(Error::ResourceLimit);
        }

        let valid = match mime {
            "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
            "image/jpeg" => bytes.starts_with(b"\xff\xd8\xff"),
            _ => false,
        };

        if !valid {
            return Err(Error::Invalid("image signature"));
        }

        self.write_for(entity, |db| {
            if !Self::authorise(db, entity, access, false)?
                .kind
                .is_surface()
            {
                return Err(Error::Invalid("image owner"));
            }

            let id = Id::new();

            db.execute(
                "INSERT INTO image_assets VALUES(?1,?2,?3,?4,?5)",
                rusqlite::params![
                    id.to_string(),
                    entity.to_string(),
                    mime,
                    bytes,
                    digest(bytes)
                ],
            )?;

            Ok(id)
        })
    }

    /// Access is checked against every current/historical referencing block.
    pub fn image_bytes(&self, id: Id, access: &Access) -> Result<(String, Vec<u8>)> {
        let (entity, mime, bytes, hash): (String, String, Vec<u8>, String) = self
            .db
            .query_row(
                "SELECT entity,mime,bytes,digest FROM image_assets WHERE id=?1",
                [id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?
            .ok_or(Error::Unavailable)?;

        let entity = Id::parse(&entity)?;

        Self::authorise(&self.db, entity, access, false)?;

        let mut query = self.db.prepare(
            "SELECT payload FROM blocks WHERE entity=?1 UNION ALL SELECT h.payload FROM \
                history h JOIN blocks b ON h.block=b.id WHERE b.entity=?1",
        )?;

        for row in query.query_map([entity.to_string()], |r| r.get::<_, String>(0))? {
            let b: Block = serde_json::from_str(&row?)?;

            if matches!(&b.content,Content::Image(i) if i.asset==id) {
                Self::read_block(&self.db, b.id, access)?;
            }
        }

        if digest(&bytes) != hash {
            return Err(Error::Integrity);
        }

        Ok((mime, bytes))
    }

    pub fn edit_cell(
        &mut self,
        block: Id,
        revision: u64,
        cell: Id,
        value: CellValue,
        access: &Access,
    ) -> Result<Block> {
        let b = self.block(block, access)?;

        let Content::Table(mut table) = b.content else {
            return Err(Error::Invalid("table block"));
        };

        table.edit(cell, value)?;

        self.edit_content(block, revision, Content::Table(table), access)
    }

    pub fn edit_diagram_part(
        &mut self,
        block: Id,
        revision: u64,
        part: DiagramPart,
        access: &Access,
    ) -> Result<Block> {
        part.validate()?;

        let b = self.block(block, access)?;

        let Content::Diagram(mut d) = b.content else {
            return Err(Error::Invalid("diagram block"));
        };

        let old = d
            .parts
            .iter_mut()
            .find(|p| p.id == part.id)
            .ok_or(Error::Unavailable)?;

        *old = part;

        self.edit_content(block, revision, Content::Diagram(d), access)
    }

    pub fn layout(&self, block: Id, access: &Access) -> Result<Option<Layout>> {
        Self::read_block(&self.db, block, access)?;

        let json: Option<String> = self
            .db
            .query_row(
                "SELECT payload FROM layouts WHERE block=?1",
                [block.to_string()],
                |r| r.get(0),
            )
            .optional()?;

        json.as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(Into::into)
    }

    pub fn set_layout(
        &mut self,
        block: Id,
        expected: Option<u64>,
        bounds: Rect,
        z: i32,
        access: &Access,
    ) -> Result<Layout> {
        bounds.validate()?;

        let owner = self.block(block, access)?.entity;

        self.write_for(owner, |db| {
            Self::read_block(db, block, access)?;
            let old: Option<String> = db
                .query_row(
                    "SELECT payload FROM layouts WHERE block=?1",
                    [block.to_string()],
                    |r| r.get(0),
                )
                .optional()?;
            let old: Option<Layout> = old.as_deref().map(serde_json::from_str).transpose()?;
            if old.as_ref().map(|l| l.revision) != expected {
                return Err(Error::Conflict);
            }
            let value = Layout {
                bounds,
                z,
                revision: expected.map_or(0, |r| r + 1),
            };
            db.execute(
                "INSERT INTO layouts VALUES(?1,?2) ON CONFLICT(block) DO UPDATE SET \
                payload=excluded.payload",
                rusqlite::params![block.to_string(), serde_json::to_string(&value)?],
            )?;
            Self::touch_master(db, owner)?;
            Ok(value)
        })
    }

    pub fn create_group(
        &mut self,
        entity: Id,
        members: &[Id],
        access: &Access,
    ) -> Result<ObjectGroup> {
        if members.is_empty()
            || members.len() > 4096
            || members.iter().collect::<BTreeSet<_>>().len() != members.len()
        {
            return Err(Error::Invalid("group members"));
        }

        self.write_for(entity, |db| {
            Self::authorise(db, entity, access, false)?;

            for &id in members {
                if Self::read_block(db, id, access)?.entity != entity {
                    return Err(Error::Invalid("group owner"));
                }
            }

            let group = ObjectGroup {
                id: Id::new(),
                entity,
                members: members.to_vec(),
                revision: 0,
            };

            db.execute(
                "INSERT INTO object_groups VALUES(?1,?2,?3)",
                rusqlite::params![
                    group.id.to_string(),
                    entity.to_string(),
                    serde_json::to_string(&group)?
                ],
            )?;

            Self::touch_master(db, entity)?;

            Ok(group)
        })
    }

    fn read_group(db: &Connection, id: Id, access: &Access) -> Result<ObjectGroup> {
        let json: String = db
            .query_row(
                "SELECT payload FROM object_groups WHERE id=?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(Error::Unavailable)?;

        let group: ObjectGroup = serde_json::from_str(&json)?;

        Self::authorise(db, group.entity, access, false)?;

        for &member in &group.members {
            Self::read_block(db, member, access)?;
        }

        Ok(group)
    }

    pub fn group(&self, id: Id, access: &Access) -> Result<ObjectGroup> {
        Self::read_group(&self.db, id, access)
    }

    pub fn transform_group(
        &mut self,
        id: Id,
        revision: u64,
        delta: Transform,
        access: &Access,
    ) -> Result<ObjectGroup> {
        delta.validate()?;

        let owner = self.group(id, access)?.entity;

        self.write_for(owner, |db| {
            let mut group = Self::read_group(db, id, access)?;

            if group.revision != revision {
                return Err(Error::Conflict);
            }

            for &id in &group.members {
                let mut b = Self::read_block(db, id, access)?;

                let t = b.transform;

                b.transform = Transform {
                    a: delta.a * t.a + delta.c * t.b,
                    b: delta.b * t.a + delta.d * t.b,
                    c: delta.a * t.c + delta.c * t.d,
                    d: delta.b * t.c + delta.d * t.d,
                    tx: delta.a * t.tx + delta.c * t.ty + delta.tx,
                    ty: delta.b * t.tx + delta.d * t.ty + delta.ty,
                };

                b.revision += 1;

                Self::persist_block(db, &b)?;
            }

            group.revision += 1;

            db.execute(
                "UPDATE object_groups SET payload=?1 WHERE id=?2",
                rusqlite::params![serde_json::to_string(&group)?, id.to_string()],
            )?;

            Ok(group)
        })
    }

    pub fn ungroup(&mut self, id: Id, revision: u64, access: &Access) -> Result<()> {
        let owner = self.group(id, access)?.entity;

        self.write_for(owner, |db| {
            if Self::read_group(db, id, access)?.revision != revision {
                return Err(Error::Conflict);
            }

            db.execute("DELETE FROM object_groups WHERE id=?1", [id.to_string()])?;

            Self::touch_master(db, owner)
        })
    }

    pub(crate) fn read_embed(db: &Connection, id: Id, access: &Access) -> Result<Embed> {
        let json: String = db
            .query_row(
                "SELECT payload FROM embeds WHERE id=?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(Error::Unavailable)?;

        let e: Embed = serde_json::from_str(&json)?;
        e.validate_layers()?;

        Self::authorise(db, e.host, access, false)?;

        Self::authorise(db, e.source, access, false)?;

        Self::require_content_access(db, e.source, access)?;

        Ok(e)
    }

    pub fn embed(&self, id: Id, access: &Access) -> Result<Embed> {
        Self::read_embed(&self.db, id, access)
    }

    pub fn create_embed(
        &mut self,
        host: Id,
        source: Id,
        frame: Rect,
        access: &Access,
    ) -> Result<Embed> {
        frame.validate()?;

        self.write_for(host, |db| {
            if !Self::authorise(db, host, access, false)?.kind.is_surface()
                || Self::authorise(db, source, access, false)?.kind != Kind::Canvas
            {
                return Err(Error::Invalid("embed kind"));
            }
            Self::require_content_access(db, source, access)?;

            let cycle: bool = db.query_row(
                "WITH RECURSIVE chain(id) AS (SELECT ?1 UNION SELECT e.source FROM embeds e \
                JOIN chain c ON e.host=c.id) SELECT EXISTS(SELECT 1 FROM chain WHERE id=?2)",
                rusqlite::params![source.to_string(), host.to_string()],
                |r| r.get(0),
            )?;
            if cycle {
                return Err(Error::Invalid("embed cycle"));
            }

            let e = Embed {
                id: Id::new(),
                host,
                source,
                camera: Camera::default(),
                frame,
                pinned: false,
                visible_layers: None,
                revision: 0,
            };
            db.execute(
                "INSERT INTO embeds VALUES(?1,?2,?3,?4)",
                rusqlite::params![
                    e.id.to_string(),
                    host.to_string(),
                    source.to_string(),
                    serde_json::to_string(&e)?
                ],
            )?;
            Self::touch_master(db, host)?;
            Ok(e)
        })
    }

    pub fn edit_embed(
        &mut self,
        id: Id,
        revision: u64,
        camera: Camera,
        frame: Rect,
        pinned: bool,
        access: &Access,
    ) -> Result<Embed> {
        camera.validate()?;

        frame.validate()?;

        let host = self.embed(id, access)?.host;

        self.write_for(host, |db| {
            let mut e = Self::read_embed(db, id, access)?;

            if e.revision != revision {
                return Err(Error::Conflict);
            }
            // Pinned navigation is temporary; resizing never changes its saved world camera.
            if !e.pinned {
                e.camera = camera;
            }

            e.frame = frame;

            e.pinned = pinned;

            e.revision += 1;

            db.execute(
                "UPDATE embeds SET payload=?1 WHERE id=?2",
                rusqlite::params![serde_json::to_string(&e)?, id.to_string()],
            )?;

            Self::touch_master(db, host)?;

            Ok(e)
        })
    }

    pub fn duplicate_embed(&mut self, id: Id, access: &Access) -> Result<Embed> {
        let old = self.embed(id, access)?;

        self.write_for(old.host, |db| {
            let mut e = Self::read_embed(db, id, access)?;

            e.id = Id::new();

            e.revision = 0;

            db.execute(
                "INSERT INTO embeds VALUES(?1,?2,?3,?4)",
                rusqlite::params![
                    e.id.to_string(),
                    e.host.to_string(),
                    e.source.to_string(),
                    serde_json::to_string(&e)?
                ],
            )?;

            Self::touch_master(db, e.host)?;

            Ok(e)
        })
    }

    /// Deleting a reference does not require disclosure of a now-locked source.
    pub fn remove_embed(&mut self, id: Id, access: &Access) -> Result<()> {
        let host: String = self
            .db
            .query_row(
                "SELECT host FROM embeds WHERE id=?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(Error::Unavailable)?;

        let host = Id::parse(&host)?;

        self.write_for(host, |db| {
            Self::authorise(db, host, access, false)?;

            db.execute("DELETE FROM embeds WHERE id=?1", [id.to_string()])?;

            Self::touch_master(db, host)
        })
    }

    pub fn save_view(
        &mut self,
        view: ViewSettings,
        expected: Option<u64>,
        access: &Access,
    ) -> Result<ViewSettings> {
        view.validate()?;

        self.write_for(view.entity, |db| {
            Self::authorise(db, view.entity, access, false)?;
            if let Some(e) = view.exercise {
                if Self::read_exercise(db, e, access)?.master != view.entity {
                    return Err(Error::Invalid("view master"));
                }
            }

            if let Some(b) = view.anchor {
                if Self::read_block(db, b, access)?.entity != view.entity
                    || (view.exercise.is_some() && Self::block_exercise(db, b)? != view.exercise)
                {
                    return Err(Error::Invalid("view anchor"));
                }
            }

            if Self::bookmark_state(db, &view, access)? == BookmarkState::NeedsReview {
                return Err(Error::Conflict);
            }
            let old: Option<String> = db
                .query_row(
                    "SELECT payload FROM view_settings WHERE id=?1",
                    [view.id.to_string()],
                    |r| r.get(0),
                )
                .optional()?;
            let old: Option<ViewSettings> = old.as_deref().map(serde_json::from_str).transpose()?;
            if old.is_some() {
                Self::read_view_db(db, view.id, access)?;
            }
            if old.as_ref().map(|v| v.revision) != expected
                || old
                    .as_ref()
                    .is_some_and(|v| v.entity != view.entity || v.exercise != view.exercise)
            {
                return Err(Error::Conflict);
            }
            let mut view = view;
            view.revision = match expected {
                Some(r) => r
                    .checked_add(1)
                    .filter(|r| *r <= i64::MAX as u64)
                    .ok_or(Error::ResourceLimit)?,
                None => 0,
            };
            db.execute(
                "INSERT INTO view_settings VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET \
                payload=excluded.payload",
                rusqlite::params![
                    view.id.to_string(),
                    view.entity.to_string(),
                    serde_json::to_string(&view)?
                ],
            )?;
            Ok(view)
        })
    }

    /// Discover existing views without exposing identifiers behind current locks.
    /// Offset counts authorised views only; callers restore the chosen view afresh.
    pub fn views(
        &self,
        entity: Id,
        exercise: Option<Id>,
        offset: u32,
        limit: u32,
        access: &Access,
    ) -> Result<Vec<Id>> {
        Self::authorise(&self.db, entity, access, false)?;
        if let Some(exercise) = exercise {
            if Self::read_exercise(&self.db, exercise, access)?.master != entity {
                return Err(Error::Invalid("view master"));
            }
        }
        if limit > 512 || offset > 100_000 {
            return Err(Error::ResourceLimit);
        }
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut statement = self
            .db
            .prepare("SELECT id FROM view_settings WHERE entity=?1 ORDER BY id LIMIT 100001")?;
        let rows = statement.query_map([entity.to_string()], |r| r.get::<_, String>(0))?;
        let mut visible = 0;
        let mut result = Vec::new();
        for (scanned, row) in rows.enumerate() {
            if scanned == 100_000 {
                return Err(Error::ResourceLimit);
            }
            let id = Id::parse(&row?)?;
            let view = match Self::read_view_db(&self.db, id, access) {
                Ok(view) => view,
                Err(Error::Denied | Error::Unavailable) => continue,
                Err(error) => return Err(error),
            };
            if view.settings.exercise != exercise {
                continue;
            }
            if visible >= offset {
                result.push(id);
                if result.len() == limit as usize {
                    break;
                }
            }
            visible += 1;
        }
        Ok(result)
    }

    pub fn view(&self, id: Id, access: &Access) -> Result<ViewSettings> {
        Ok(Self::read_view_db(&self.db, id, access)?.settings)
    }
    pub(crate) fn read_view_db(db: &Connection, id: Id, access: &Access) -> Result<RestoredView> {
        let (entity, json): (String, String) = db
            .query_row(
                "SELECT entity,payload FROM view_settings WHERE id=?1",
                [id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .ok_or(Error::Unavailable)?;

        let v: ViewSettings = serde_json::from_str(&json)?;
        v.validate()?;
        if v.id != id || v.entity.to_string() != entity {
            return Err(Error::Integrity);
        }
        Self::authorise(db, v.entity, access, false)?;

        if let Some(e) = v.exercise {
            if Self::read_exercise(db, e, access)?.master != v.entity {
                return Err(Error::Integrity);
            }
        }

        if let Some(b) = v.anchor {
            Self::read_block(db, b, access)?;
        }

        let bookmark_state = Self::bookmark_state(db, &v, access)?;
        Ok(RestoredView {
            settings: v,
            bookmark_state,
        })
    }

    pub fn surface_items(
        &self,
        entity: Id,
        offset: u32,
        limit: u32,
        access: &Access,
    ) -> Result<Vec<SurfaceItem>> {
        Self::authorise(&self.db, entity, access, false)?;

        if limit > 512 {
            return Err(Error::ResourceLimit);
        }

        let mut stmt = self.db.prepare(
            "SELECT id FROM blocks WHERE entity=?1 ORDER BY ordinal,id LIMIT ?2 OFFSET ?3",
        )?;

        let mut out = Vec::new();

        for row in stmt.query_map(rusqlite::params![entity.to_string(), limit, offset], |r| {
            r.get::<_, String>(0)
        })? {
            let id = Id::parse(&row?)?;

            match Self::read_block(&self.db, id, access) {
                Ok(b) => out.push(SurfaceItem::Visible(Box::new(b))),
                Err(Error::Denied) => out.push(SurfaceItem::Protected {
                    block: id,
                    exercise: Self::block_exercise(&self.db, id)?,
                }),
                Err(e) => return Err(e),
            }
        }

        Ok(out)
    }

    pub fn open_in_master(
        &self,
        exercise: Id,
        block: Option<Id>,
        access: &Access,
    ) -> Result<MasterLocation> {
        let e = self.exercise(exercise, access)?;

        let block = if let Some(id) = block {
            if Self::block_exercise(&self.db, id)? != Some(exercise) {
                return Err(Error::Invalid("exercise anchor"));
            }

            self.block(id, access)?;

            Some(id)
        } else {
            self.exercise_blocks(exercise, 0, 1, access)?
                .first()
                .map(|b| b.id)
        };

        Ok(MasterLocation {
            master: e.master,
            block,
        })
    }
}

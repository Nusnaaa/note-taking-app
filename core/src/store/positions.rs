//! Revision-bound selection bookmarks. Offsets count Unicode scalar values, not
//! bytes or UTF-16 units. These are metadata; they never edit or repair content.
use super::*;
use crate::{
    content::{Content, MathForm, MathNode},
    objects::CellValue,
};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionBookmark {
    pub anchor: CaretPoint,
    pub focus: CaretPoint,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaretPoint {
    pub block: Id,
    pub revision: u64,
    pub region: Option<Id>,
    pub target: CaretTarget,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum InlinePoint {
    Text { offset: u32 },
    Maths { node: Id, offset: u32 },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum CaretTarget {
    Boundary { after: bool },
    Inline(InlinePoint),
    Cell { cell: Id, point: InlinePoint },
    DiagramLabel { part: Id, offset: u32 },
    ImageCaption { offset: u32 },
    GraphLabel { label: GraphLabel, offset: u32 },
    Ink { stroke: Id, sample: u32 },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum GraphLabel {
    XLabel,
    XUnit,
    YLabel,
    YUnit,
    Series(Id),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BookmarkState {
    None,
    Current,
    NeedsReview,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoredView {
    pub settings: ViewSettings,
    pub bookmark_state: BookmarkState,
}

fn offset(text: &str, value: u32) -> Result<()> {
    if value as usize <= text.chars().count() {
        Ok(())
    } else {
        Err(Error::Invalid("caret offset"))
    }
}
fn maths(root: &MathNode, id: Id, at: u32) -> Result<()> {
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        let children = n.children();
        if n.id == id {
            return match &n.form {
                MathForm::Token(t)
                | MathForm::Text(t)
                | MathForm::Unknown(t)
                | MathForm::Unparsed(t) => offset(t, at),
                _ if at as usize <= children.len() => Ok(()),
                _ => Err(Error::Invalid("maths caret boundary")),
            };
        }
        stack.extend(children);
    }
    Err(Error::Invalid("maths caret node"))
}
impl InlinePoint {
    fn content(&self, value: &Content) -> Result<()> {
        match (self, value) {
            (
                Self::Text { offset: at },
                Content::Text(t) | Content::Numeric(t) | Content::Code { text: t, .. },
            ) => offset(t, *at),
            (Self::Maths { node, offset: at }, Content::Maths(root)) => maths(root, *node, *at),
            _ => Err(Error::Invalid("caret content type")),
        }
    }
    fn cell(&self, value: &CellValue) -> Result<()> {
        match (self, value) {
            (Self::Text { offset: at }, CellValue::Text(t) | CellValue::Numeric(t)) => {
                offset(t, *at)
            }
            (Self::Maths { node, offset: at }, CellValue::Maths(root)) => maths(root, *node, *at),
            _ => Err(Error::Invalid("cell caret type")),
        }
    }
}
impl CaretPoint {
    pub(crate) fn validate_shape(&self) -> Result<()> {
        if self.revision > i64::MAX as u64 {
            return Err(Error::Invalid("caret revision"));
        }
        Ok(())
    }
    /// A revision mismatch is explicitly stale, even if an old offset still fits.
    pub(crate) fn validate_block(
        &self,
        b: &Block,
        samples: impl Fn(Id) -> Result<usize>,
    ) -> Result<bool> {
        self.validate_shape()?;
        if b.id != self.block {
            return Err(Error::Invalid("caret block"));
        }
        if self.revision > b.revision {
            return Err(Error::Invalid("future caret revision"));
        }
        if let CaretTarget::Ink { stroke, sample } = self.target {
            // Original observations outlive presentation/history revisions. Even
            // stale metadata cannot claim another block's source or an invented sample.
            if !b.source_ids.contains(&stroke) || sample as usize >= samples(stroke)? {
                return Err(Error::Invalid("caret observed sample"));
            }
        }
        if b.revision != self.revision || !b.visible {
            return Ok(false);
        }
        let content = if let Some(region) = self.region {
            let r = b
                .regions
                .iter()
                .find(|r| r.id == region)
                .ok_or(Error::Invalid("caret region"))?;
            if let CaretTarget::Ink { stroke, .. } = self.target {
                if !r.strokes.contains(&stroke) {
                    return Err(Error::Invalid("caret region stroke"));
                }
            }
            r.content.as_ref().unwrap_or(&Content::Ink)
        } else {
            &b.content
        };
        content.validate()?;
        match (&self.target, content) {
            (CaretTarget::Boundary { .. }, _) => {}
            (CaretTarget::Inline(point), value) => point.content(value)?,
            (CaretTarget::Cell { cell, point }, Content::Table(table)) => point.cell(
                &table
                    .rows
                    .iter()
                    .flatten()
                    .find(|c| c.id == *cell)
                    .ok_or(Error::Invalid("caret cell"))?
                    .value,
            )?,
            (CaretTarget::DiagramLabel { part, offset: at }, Content::Diagram(diagram)) => offset(
                &diagram
                    .parts
                    .iter()
                    .find(|p| p.id == *part)
                    .ok_or(Error::Invalid("caret diagram part"))?
                    .label,
                *at,
            )?,
            (CaretTarget::ImageCaption { offset: at }, Content::Image(image)) => {
                offset(&image.caption, *at)?
            }
            (CaretTarget::GraphLabel { label, offset: at }, Content::Graph(graph)) => {
                let text = match label {
                    GraphLabel::XLabel => &graph.x.label,
                    GraphLabel::YLabel => &graph.y.label,
                    GraphLabel::XUnit => graph
                        .x
                        .unit
                        .as_ref()
                        .ok_or(Error::Invalid("caret axis unit"))?,
                    GraphLabel::YUnit => graph
                        .y
                        .unit
                        .as_ref()
                        .ok_or(Error::Invalid("caret axis unit"))?,
                    GraphLabel::Series(id) => {
                        &graph
                            .series
                            .iter()
                            .find(|s| s.id == *id)
                            .ok_or(Error::Invalid("caret series"))?
                            .label
                    }
                };
                offset(text, *at)?;
            }
            (CaretTarget::Ink { stroke, .. }, _) => {
                if !b.visible_strokes.contains(stroke) {
                    return Err(Error::Invalid("caret observed sample"));
                }
            }
            _ => return Err(Error::Invalid("caret target type")),
        }
        Ok(true)
    }
    pub(crate) fn remap(&mut self, ids: &BTreeMap<Id, Id>) {
        self.block = ids[&self.block];
        self.region = self.region.map(|id| ids[&id]);
        if let CaretTarget::Ink { stroke, .. } = &mut self.target {
            *stroke = ids[stroke];
        }
        // Maths/cell/part/series identifiers are local to copied content, whose
        // immutable structure retains them. Block/region/source IDs are global.
    }
}
impl SelectionBookmark {
    pub(crate) fn points(&self) -> [&CaretPoint; 2] {
        [&self.anchor, &self.focus]
    }
    pub(crate) fn validate_shape(&self) -> Result<()> {
        self.anchor.validate_shape()?;
        self.focus.validate_shape()
    }
    pub(crate) fn remap(&mut self, ids: &BTreeMap<Id, Id>) {
        self.anchor.remap(ids);
        self.focus.remap(ids);
    }
}
impl Store {
    pub(crate) fn bookmark_state(
        db: &Connection,
        view: &ViewSettings,
        access: &Access,
    ) -> Result<BookmarkState> {
        let Some(bookmark) = &view.bookmark else {
            return Ok(BookmarkState::None);
        };
        let mut current = true;
        for point in bookmark.points() {
            let block = Self::read_block(db, point.block, access)?;
            if block.entity != view.entity {
                return Err(Error::Invalid("caret view owner"));
            }
            if view.exercise.is_some() && Self::block_exercise(db, block.id)? != view.exercise {
                current = false;
            }
            current &= point.validate_block(&block, |stroke| {
                let (json, hash): (String, String) = db.query_row(
                    "SELECT payload,digest FROM strokes WHERE id=?1 AND block=?2",
                    rusqlite::params![stroke.to_string(), block.id.to_string()],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?;
                if crate::digest(json.as_bytes()) != hash {
                    return Err(Error::Integrity);
                }
                let source: crate::ink::Stroke = serde_json::from_str(&json)?;
                source.validate_persisted()?;
                if source.id != stroke {
                    return Err(Error::Integrity);
                }
                Ok(source.points.len())
            })?;
        }
        Ok(if current {
            BookmarkState::Current
        } else {
            BookmarkState::NeedsReview
        })
    }
    pub fn restore_view(&self, id: Id, access: &Access) -> Result<RestoredView> {
        Self::read_view_db(&self.db, id, access)
    }
}

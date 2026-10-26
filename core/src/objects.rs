//! Editable technical objects. These retain authored data; they do not solve,
//! infer a best-fit curve, repair notation, or turn ink into recognition images.
use crate::{content::MathNode, Error, Id, Result};
mod table_layout;
pub use table_layout::{CellInkOrigin, TableLayout};

use serde::{Deserialize, Serialize};

use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Position {
    pub x: f64,
    pub y: f64,
}

impl Position {
    pub fn validate(self) -> Result<()> {
        if !self.x.is_finite() || !self.y.is_finite() {
            Err(Error::Invalid("position"))
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn validate(self) -> Result<()> {
        Position {
            x: self.x,
            y: self.y,
        }
        .validate()?;

        if !self.width.is_finite()
            || !self.height.is_finite()
            || self.width <= 0.
            || self.height <= 0.
            || !(self.x + self.width).is_finite()
            || !(self.y + self.height).is_finite()
        {
            Err(Error::Invalid("rectangle"))
        } else {
            Ok(())
        }
    }

    pub fn contains(self, p: Position) -> bool {
        p.x >= self.x && p.y >= self.y && p.x <= self.x + self.width && p.y <= self.y + self.height
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", deny_unknown_fields)]
pub enum CellValue {
    Text(String),
    Numeric(String),
    Maths(MathNode),
}

impl CellValue {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Maths(n) => n.validate(),
            Self::Text(s) | Self::Numeric(s) => label(s),
        }
    }

    pub fn literal(&self) -> String {
        match self {
            Self::Maths(n) => n.literal(),
            Self::Text(s) | Self::Numeric(s) => s.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cell {
    pub id: Id,
    pub value: CellValue,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Table {
    pub rows: Vec<Vec<Cell>>,
    pub header_rows: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<TableLayout>,
}

/// Explicit structural edits; inserted cells start as empty authored text.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum TableEdit {
    InsertRow { index: usize },
    RemoveRow { index: usize },
    InsertColumn { index: usize },
    RemoveColumn { index: usize },
    InsertSizedRow { index: usize, height: f64 },
    InsertSizedColumn { index: usize, width: f64 },
    ResizeRow { index: usize, height: f64 },
    ResizeColumn { index: usize, width: f64 },
}

impl Table {
    pub fn validate(&self) -> Result<()> {
        let columns = self.rows.first().map_or(0, Vec::len);

        if self.rows.is_empty()
            || columns == 0
            || self.rows.len() > 1000
            || columns > 256
            || self.rows.len() * columns > 10000
            || self.header_rows as usize > self.rows.len()
        {
            return Err(Error::Invalid("table dimensions"));
        }

        let mut ids = BTreeSet::new();

        for row in &self.rows {
            if row.len() != columns {
                return Err(Error::Invalid("ragged table"));
            }

            for c in row {
                if !ids.insert(c.id) {
                    return Err(Error::Invalid("duplicate cell"));
                }

                c.value.validate()?;
            }
        }

        if let Some(layout) = &self.layout {
            layout.validate(self)?;
        }
        Ok(())
    }

    /// Validate a detached candidate before replacing the table, including bounds.
    pub fn restructure(&mut self, edit: TableEdit) -> Result<()> {
        if let TableEdit::InsertSizedRow { index, height }
        | TableEdit::InsertSizedColumn {
            index,
            width: height,
        } = edit
        {
            self.validate()?;
            let mut next = self.clone();
            let mut layout = next
                .layout
                .take()
                .ok_or(Error::Invalid("table layout required"))?;
            match edit {
                TableEdit::InsertSizedRow { .. } => {
                    next.restructure(TableEdit::InsertRow { index })?;
                    layout.row_heights.insert(index, height);
                }
                _ => {
                    next.restructure(TableEdit::InsertColumn { index })?;
                    layout.column_widths.insert(index, height);
                }
            }
            next.layout = Some(layout);
            next.validate()?;
            *self = next;
            return Ok(());
        }
        self.validate()?;
        let mut next = self.clone();
        let rows = next.rows.len();
        let columns = next.rows[0].len();
        if next.layout.is_some()
            && matches!(
                edit,
                TableEdit::InsertRow { .. } | TableEdit::InsertColumn { .. }
            )
        {
            return Err(Error::Invalid("explicit inserted dimension required"));
        }
        let empty = || Cell {
            id: Id::new(),
            value: CellValue::Text(String::new()),
        };
        match edit {
            TableEdit::InsertRow { index } if index <= rows => {
                next.rows
                    .insert(index, (0..columns).map(|_| empty()).collect());
                // Insertion inside the header extends it; insertion at its boundary is body.
                if index < next.header_rows as usize {
                    next.header_rows += 1;
                }
            }
            TableEdit::RemoveRow { index } if index < rows && rows > 1 => {
                next.rows.remove(index);
                if let Some(layout) = &mut next.layout {
                    layout.row_heights.remove(index);
                }
                if index < next.header_rows as usize {
                    next.header_rows -= 1;
                }
            }
            TableEdit::InsertColumn { index } if index <= columns => {
                for row in &mut next.rows {
                    row.insert(index, empty());
                }
            }
            TableEdit::RemoveColumn { index } if index < columns && columns > 1 => {
                for row in &mut next.rows {
                    row.remove(index);
                }
                if let Some(layout) = &mut next.layout {
                    layout.column_widths.remove(index);
                }
            }
            TableEdit::ResizeRow { index, height } => {
                *next
                    .layout
                    .as_mut()
                    .ok_or(Error::Invalid("table layout required"))?
                    .row_heights
                    .get_mut(index)
                    .ok_or(Error::Invalid("table edit bounds"))? = height;
            }
            TableEdit::ResizeColumn { index, width } => {
                *next
                    .layout
                    .as_mut()
                    .ok_or(Error::Invalid("table layout required"))?
                    .column_widths
                    .get_mut(index)
                    .ok_or(Error::Invalid("table edit bounds"))? = width;
            }
            _ => return Err(Error::Invalid("table edit bounds")),
        }
        let cells: BTreeSet<_> = next.rows.iter().flatten().map(|c| c.id).collect();
        if let Some(layout) = &mut next.layout {
            layout.ink_origins.retain(|ink| cells.contains(&ink.cell));
        }
        next.validate()?;
        *self = next;
        Ok(())
    }

    pub fn edit(&mut self, id: Id, value: CellValue) -> Result<()> {
        value.validate()?;

        self.rows
            .iter_mut()
            .flatten()
            .find(|c| c.id == id)
            .ok_or(Error::Unavailable)?
            .value = value;

        Ok(())
    }

    pub fn literal(&self) -> String {
        self.rows
            .iter()
            .map(|r| {
                r.iter()
                    .map(|c| c.value.literal())
                    .collect::<Vec<_>>()
                    .join("\t")
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", deny_unknown_fields)]
pub enum Shape {
    Polyline(Vec<Position>),
    Rectangle(Rect),
    Ellipse(Rect),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagramPart {
    pub id: Id,
    pub shape: Shape,
    pub label: String,
}

impl DiagramPart {
    pub fn validate(&self) -> Result<()> {
        label(&self.label)?;

        match &self.shape {
            Shape::Polyline(v) => {
                if v.len() < 2 || v.len() > 10000 {
                    return Err(Error::Invalid("polyline"));
                }

                for p in v {
                    p.validate()?;
                }

                Ok(())
            }

            Shape::Rectangle(r) | Shape::Ellipse(r) => r.validate(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Diagram {
    pub parts: Vec<DiagramPart>,
}

impl Diagram {
    pub fn validate(&self) -> Result<()> {
        if self.parts.len() > 10000 {
            return Err(Error::ResourceLimit);
        }

        let mut ids = BTreeSet::new();

        for p in &self.parts {
            if !ids.insert(p.id) {
                return Err(Error::Invalid("duplicate diagram part"));
            }

            p.validate()?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Axis {
    pub label: String,
    pub unit: Option<String>,
    pub min: f64,
    pub max: f64,
}

impl Axis {
    fn validate(&self) -> Result<()> {
        label(&self.label)?;

        if let Some(u) = &self.unit {
            label(u)?;
        }

        if !self.min.is_finite()
            || !self.max.is_finite()
            || self.max <= self.min
            || !(self.max - self.min).is_finite()
        {
            return Err(Error::Invalid("axis"));
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Series {
    pub id: Id,
    pub label: String,
    pub points: Vec<Position>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Graph {
    pub x: Axis,
    pub y: Axis,
    pub series: Vec<Series>,
}

impl Graph {
    pub fn validate(&self) -> Result<()> {
        self.x.validate()?;

        self.y.validate()?;

        if self.series.len() > 10000 {
            return Err(Error::ResourceLimit);
        }

        let mut ids = BTreeSet::new();

        let mut count = 0;

        for s in &self.series {
            if !ids.insert(s.id) {
                return Err(Error::Invalid("duplicate series"));
            }

            label(&s.label)?;

            count += s.points.len();

            if count > 100000 {
                return Err(Error::ResourceLimit);
            }

            for p in &s.points {
                p.validate()?;
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Image {
    pub asset: Id,
    pub caption: String,
    pub crop: Option<Rect>,
}

impl Image {
    pub fn validate(&self) -> Result<()> {
        label(&self.caption)?;

        if let Some(r) = self.crop {
            r.validate()?;

            if r.x < 0. || r.y < 0. || r.x + r.width > 1. || r.y + r.height > 1. {
                return Err(Error::Invalid("normalised image crop"));
            }
        }

        Ok(())
    }
}

fn label(s: &str) -> Result<()> {
    if s.len() > 1_000_000 {
        Err(Error::ResourceLimit)
    } else {
        Ok(())
    }
}

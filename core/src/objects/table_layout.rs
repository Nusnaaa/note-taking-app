//! Source-unit cell geometry; resizing never scales recorded handwriting.
use super::*;
use crate::ink::Transform;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CellInkOrigin {
    pub cell: Id,
    pub stroke: Id,
    /// Source coordinate of the cell's top-left at capture/adoption.
    pub origin: Position,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TableLayout {
    pub coordinate_space: String,
    pub origin: Position,
    pub row_heights: Vec<f64>,
    pub column_widths: Vec<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ink_origins: Vec<CellInkOrigin>,
}
impl TableLayout {
    pub fn validate(&self, table: &Table) -> Result<()> {
        self.origin.validate()?;
        if self.coordinate_space.is_empty()
            || self.coordinate_space.len() > 256
            || self.row_heights.len() != table.rows.len()
            || self.column_widths.len() != table.rows.first().map_or(0, Vec::len)
            || self.ink_origins.len() > 4096
            || self
                .row_heights
                .iter()
                .chain(&self.column_widths)
                .any(|n| !n.is_finite() || *n <= 0.)
        {
            return Err(Error::Invalid("table layout"));
        }
        Rect {
            x: self.origin.x,
            y: self.origin.y,
            width: self.column_widths.iter().sum(),
            height: self.row_heights.iter().sum(),
        }
        .validate()?;
        let cells: BTreeSet<_> = table.rows.iter().flatten().map(|c| c.id).collect();
        let mut strokes = BTreeSet::new();
        for ink in &self.ink_origins {
            ink.origin.validate()?;
            if !cells.contains(&ink.cell) || !strokes.insert(ink.stroke) {
                return Err(Error::Invalid("table ink placement"));
            }
        }
        Ok(())
    }
    pub fn cell_bounds(&self, table: &Table, cell: Id) -> Result<Rect> {
        table.validate()?;
        self.validate(table)?;
        for (row, cells) in table.rows.iter().enumerate() {
            if let Some(column) = cells.iter().position(|c| c.id == cell) {
                let bounds = Rect {
                    x: self.origin.x + self.column_widths[..column].iter().sum::<f64>(),
                    y: self.origin.y + self.row_heights[..row].iter().sum::<f64>(),
                    width: self.column_widths[column],
                    height: self.row_heights[row],
                };
                bounds.validate()?;
                return Ok(bounds);
            }
        }
        Err(Error::Unavailable)
    }
    pub(crate) fn validate_source(&self, table: &Table, stroke: &crate::ink::Stroke) -> Result<()> {
        if let Some(ink) = self.ink_origins.iter().find(|ink| ink.stroke == stroke.id) {
            if stroke.coordinate_space != self.coordinate_space
                || stroke
                    .points
                    .iter()
                    .any(|p| !(p.x - ink.origin.x).is_finite() || !(p.y - ink.origin.y).is_finite())
            {
                return Err(Error::Invalid("table ink source coordinates"));
            }
            let transform = self.stroke_transform(table, stroke.id)?;
            if stroke
                .points
                .iter()
                .any(|p| !(p.x + transform.tx).is_finite() || !(p.y + transform.ty).is_finite())
            {
                return Err(Error::Invalid("table ink placement coordinates"));
            }
        }
        Ok(())
    }
    pub fn stroke_transform(&self, table: &Table, stroke: Id) -> Result<Transform> {
        let ink = self
            .ink_origins
            .iter()
            .find(|ink| ink.stroke == stroke)
            .ok_or(Error::Unavailable)?;
        let cell = self.cell_bounds(table, ink.cell)?;
        let transform = Transform {
            tx: cell.x - ink.origin.x,
            ty: cell.y - ink.origin.y,
            ..Transform::default()
        };
        transform.validate()?;
        Ok(transform)
    }
}

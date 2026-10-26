//! View preferences and guide derivatives are separate from immutable observations.
use super::*;
use crate::{
    ink::{PressureResponse, StrokeStyle},
    objects::Rect,
};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PenPreset {
    /// Scoped to its containing view, not a global object identity.
    pub id: Id,
    pub name: String,
    pub width_mm: f64,
    pub colour_rgba: [u8; 4],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressure_response: Option<PressureResponse>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EraserMode {
    WholeStroke,
    Selection,
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EraserSettings {
    pub mode: EraserMode,
    pub width_mm: f64,
}
impl EraserSettings {
    pub fn validate(self) -> Result<()> {
        if !self.width_mm.is_finite() || self.width_mm <= 0. || self.width_mm > 100. {
            return Err(Error::Invalid("eraser settings"));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewEraserSnapshot {
    pub mode: EraserMode,
    pub width: f64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GuideKind {
    Blank,
    Ruled,
    Dotted,
    Graph,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuideBackground {
    pub kind: GuideKind,
    pub spacing_mm: f64,
    pub line_width_mm: f64,
    pub origin_mm: [f64; 2],
    pub colour_rgba: [u8; 4],
    pub paper_rgba: [u8; 4],
    pub include_in_export: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthoringSettings {
    pub presets: Vec<PenPreset>,
    pub selected_preset: Option<Id>,
    pub background: Option<GuideBackground>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressure_response: Option<PressureResponse>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eraser: Option<EraserSettings>,
}
impl AuthoringSettings {
    pub fn validate(&self) -> Result<()> {
        if self.presets.len() > 32 {
            return Err(Error::ResourceLimit);
        }
        if let Some(response) = self.pressure_response {
            response.validate()?;
        }
        if let Some(eraser) = self.eraser {
            eraser.validate()?;
        }
        let mut ids = BTreeSet::new();
        for preset in &self.presets {
            if let Some(response) = preset.pressure_response {
                response.validate()?;
            }
            if !ids.insert(preset.id)
                || preset.name.trim().is_empty()
                || preset.name.len() > 256
                || !preset.width_mm.is_finite()
                || preset.width_mm <= 0.
                || preset.width_mm > 100.
            {
                return Err(Error::Invalid("pen preset"));
            }
        }
        if self.selected_preset.is_some_and(|id| !ids.contains(&id)) {
            return Err(Error::Invalid("selected preset"));
        }
        if let Some(g) = &self.background {
            if !g.spacing_mm.is_finite()
                || g.spacing_mm <= 0.
                || g.spacing_mm > 1000.
                || !g.line_width_mm.is_finite()
                || g.line_width_mm <= 0.
                || g.line_width_mm > g.spacing_mm
                || g.origin_mm.iter().any(|v| !v.is_finite())
            {
                return Err(Error::Invalid("guide background"));
            }
        }
        Ok(())
    }
    pub(crate) fn requires_v10(&self) -> bool {
        self.pressure_response.is_some()
            || self.eraser.is_some()
            || self.presets.iter().any(|p| p.pressure_response.is_some())
    }
    pub(crate) fn selected(&self) -> Option<&PenPreset> {
        self.selected_preset
            .and_then(|id| self.presets.iter().find(|p| p.id == id))
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewPenSnapshot {
    pub view: Id,
    pub view_revision: u64,
    pub entity: Id,
    pub exercise: Option<Id>,
    pub coordinate_space: String,
    pub units_per_mm: f64,
    pub style: StrokeStyle,
    pub finger_ink: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eraser: Option<ViewEraserSnapshot>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewGuideSnapshot {
    pub view: Id,
    pub view_revision: u64,
    pub configured: bool,
    pub omitted_for_export: bool,
    pub coordinate_space: String,
    pub svg: Option<String>,
}
fn calibration(space: &str, units: f64) -> Result<()> {
    if space.is_empty() || space.len() > 256 || !units.is_finite() || units <= 0. {
        return Err(Error::Invalid("source calibration"));
    }
    Ok(())
}
fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn colour(c: [u8; 4]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}
impl Store {
    fn update_authoring(
        &mut self,
        id: Id,
        expected: u64,
        access: &Access,
        change: impl FnOnce(&mut ViewSettings) -> Result<()>,
    ) -> Result<ViewSettings> {
        // Preferences do not consume an edit-Undo step or repair a stale bookmark.
        self.write(|db| {
            let mut view = Self::read_view_db(db, id, access)?.settings;
            if view.revision != expected {
                return Err(Error::Conflict);
            }
            change(&mut view)?;
            view.validate()?;
            view.revision = expected
                .checked_add(1)
                .filter(|r| *r <= i64::MAX as u64)
                .ok_or(Error::ResourceLimit)?;
            db.execute(
                "UPDATE view_settings SET payload=?1 WHERE id=?2",
                rusqlite::params![serde_json::to_string(&view)?, id.to_string()],
            )?;
            Ok(view)
        })
    }
    pub fn configure_view(
        &mut self,
        id: Id,
        expected: u64,
        mut authoring: Option<AuthoringSettings>,
        access: &Access,
    ) -> Result<ViewSettings> {
        if let Some(a) = &authoring {
            a.validate()?;
        }
        self.update_authoring(id, expected, access, |v| {
            if let Some(a) = authoring.as_mut() {
                if let Some(preset) = a.selected() {
                    v.pen_width_mm = preset.width_mm;
                    v.colour_rgba = preset.colour_rgba;
                    a.pressure_response = preset.pressure_response;
                }
            }
            v.authoring = authoring.map(Box::new);
            Ok(())
        })
    }
    pub fn apply_view_preset(
        &mut self,
        id: Id,
        expected: u64,
        preset: Id,
        access: &Access,
    ) -> Result<ViewSettings> {
        self.update_authoring(id, expected, access, |v| {
            let a = v.authoring.as_mut().ok_or(Error::Unavailable)?;
            let chosen = a
                .presets
                .iter()
                .find(|p| p.id == preset)
                .ok_or(Error::Unavailable)?;
            v.pen_width_mm = chosen.width_mm;
            v.colour_rgba = chosen.colour_rgba;
            a.pressure_response = chosen.pressure_response;
            a.selected_preset = Some(preset);
            Ok(())
        })
    }
    pub fn view_pen(
        &self,
        id: Id,
        space: &str,
        units_per_mm: f64,
        access: &Access,
    ) -> Result<ViewPenSnapshot> {
        calibration(space, units_per_mm)?;
        let v = self.view(id, access)?;
        let style = StrokeStyle {
            colour_rgba: v.colour_rgba,
            width: v.pen_width_mm * units_per_mm,
            pressure_response: v.authoring.as_ref().and_then(|a| a.pressure_response),
        };
        style.validate()?;
        let eraser = v
            .authoring
            .as_ref()
            .and_then(|a| a.eraser)
            .map(|e| ViewEraserSnapshot {
                mode: e.mode,
                width: e.width_mm * units_per_mm,
            });
        if eraser.is_some_and(|e| !e.width.is_finite() || e.width <= 0. || e.width > 1_000_000.) {
            return Err(Error::Invalid("source eraser width"));
        }
        Ok(ViewPenSnapshot {
            view: id,
            view_revision: v.revision,
            entity: v.entity,
            exercise: v.exercise,
            coordinate_space: space.into(),
            units_per_mm,
            style,
            finger_ink: v.finger_ink,
            eraser,
        })
    }
    pub fn view_guides(
        &self,
        id: Id,
        bounds: Rect,
        space: &str,
        units_per_mm: f64,
        for_export: bool,
        access: &Access,
    ) -> Result<ViewGuideSnapshot> {
        calibration(space, units_per_mm)?;
        bounds.validate()?;
        let v = self.view(id, access)?;
        let guide = v.authoring.as_ref().and_then(|a| a.background.as_ref());
        let mut result = ViewGuideSnapshot {
            view: id,
            view_revision: v.revision,
            configured: guide.is_some(),
            omitted_for_export: false,
            coordinate_space: space.into(),
            svg: None,
        };
        let Some(g) = guide else { return Ok(result) };
        if for_export && !g.include_in_export {
            result.omitted_for_export = true;
            return Ok(result);
        }
        let spacing = g.spacing_mm * units_per_mm;
        let width = g.line_width_mm * units_per_mm;
        let x = g.origin_mm[0] * units_per_mm;
        let y = g.origin_mm[1] * units_per_mm;
        if [spacing, width, x, y, x + spacing, y + spacing]
            .iter()
            .any(|v| !v.is_finite())
            || spacing <= 0.
            || width <= 0.
        {
            return Err(Error::Invalid("guide geometry"));
        }
        let nx = (bounds.width / spacing).ceil() + 1.;
        let ny = (bounds.height / spacing).ceil() + 1.;
        let cost = match g.kind {
            GuideKind::Blank => 0.,
            GuideKind::Ruled => ny,
            GuideKind::Graph => nx + ny,
            GuideKind::Dotted => nx * ny,
        };
        if !cost.is_finite() || cost > 1_000_000. {
            return Err(Error::ResourceLimit);
        }
        let primitive = match g.kind {
            GuideKind::Blank => String::new(),
            GuideKind::Ruled => format!("<path d=\"M0 0H{spacing}\"/>"),
            GuideKind::Graph => format!("<path d=\"M{spacing} 0H0V{spacing}\"/>"),
            GuideKind::Dotted => format!(
                "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{}\" stroke=\"none\"/>",
                spacing / 2.,
                spacing / 2.,
                width / 2.,
                colour(g.colour_rgba)
            ),
        };
        let paper = if g.paper_rgba[3] == 0 {
            String::new()
        } else {
            format!("<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\" fill-opacity=\"{}\"/>",bounds.x,bounds.y,bounds.width,bounds.height,colour(g.paper_rgba),f64::from(g.paper_rgba[3])/255.)
        };
        let overlay = if primitive.is_empty() {
            String::new()
        } else {
            format!("<defs><pattern id=\"guides\" x=\"{x}\" y=\"{y}\" width=\"{spacing}\" height=\"{spacing}\" patternUnits=\"userSpaceOnUse\" overflow=\"visible\"><g fill=\"none\" stroke=\"{}\" stroke-width=\"{width}\" opacity=\"{}\">{primitive}</g></pattern></defs><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"url(#guides)\"/>",colour(g.colour_rgba),f64::from(g.colour_rgba[3])/255.,bounds.x,bounds.y,bounds.width,bounds.height)
        };
        result.svg = Some(format!("<svg xmlns=\"http://www.w3.org/2000/svg\" data-coordinate-space=\"{}\" viewBox=\"{} {} {} {}\">{paper}{overlay}</svg>",xml(space),bounds.x,bounds.y,bounds.width,bounds.height));
        Ok(result)
    }
}

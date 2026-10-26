//! Deliberate derivative exports. Converted-only is the default contract; source
//! strokes are never obtained unless the reviewed choice explicitly includes them.
mod derivative;
mod image_crop;
mod snapshot;
mod source_ink;
pub use snapshot::DerivativeSource;
mod table_ink;
use crate::{
    content::Content,
    converters::TextConverter,
    objects::{Rect, Shape},
    processing::{Converter, Format, JobControl},
    security::{Access, Authenticator, Scope},
    store::{Block, Kind, Store},
    Error, Id, Result,
};

use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Selection {
    Document(Id),
    Exercise(Id),
    Blocks(Vec<Id>),
    CanvasRegion { canvas: Id, bounds: Rect },
}

#[derive(Debug, Clone)]
pub struct ExportBundle {
    pub markdown: String,
    pub assets: BTreeMap<String, Vec<u8>>,
    pub warnings: Vec<String>,
    pub includes_original_ink: bool,
}

pub struct DerivativePlan {
    library: String,
    generation: String,
    scopes: BTreeSet<Scope>,
    bundle: ExportBundle,
    render_contents: Vec<Content>,
    pub selected_blocks: usize,
    pub warnings: Vec<String>,
    pub includes_original_ink: bool,
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn svg_root(body: String, bounds: Rect) -> String {
    format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"{} {} {} {}\"><defs><clipPath id=\"scope\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/></clipPath></defs><g clip-path=\"url(#scope)\">{body}</g></svg>",bounds.x,bounds.y,bounds.width,bounds.height,bounds.x,bounds.y,bounds.width,bounds.height)
}

/// Vector technical-object derivative. Native JSON remains the editable source.
pub fn technical_svg(content: &Content, bounds: Rect) -> Result<String> {
    content.validate()?;

    bounds.validate()?;

    let mut body = String::new();

    match content {
        Content::Diagram(d) => {
            for p in &d.parts {
                let shape = match &p.shape {
                    Shape::Polyline(points) => format!(
                        "<polyline points=\"{}\"/>",
                        points
                            .iter()
                            .map(|p| format!("{},{}", p.x, p.y))
                            .collect::<Vec<_>>()
                            .join(" ")
                    ),
                    Shape::Rectangle(r) => format!(
                        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>",
                        r.x, r.y, r.width, r.height
                    ),
                    Shape::Ellipse(r) => format!(
                        "<ellipse cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\"/>",
                        r.x + r.width / 2.,
                        r.y + r.height / 2.,
                        r.width / 2.,
                        r.height / 2.
                    ),
                };

                let (x, y) = match &p.shape {
                    Shape::Polyline(points) => (points[0].x, points[0].y),
                    Shape::Rectangle(r) | Shape::Ellipse(r) => (r.x, r.y),
                };
                body += &format!("<g fill=\"none\" stroke=\"black\">{shape}</g><text x=\"{x}\" y=\"{}\" font-size=\"5\">{}</text>",y+5.,xml(&p.label));
            }
        }

        Content::Graph(g) => {
            // A clip path conceals pixels, not coordinates inside the SVG file.
            // Require review instead of exporting points outside the saved axes.
            if g.series
                .iter()
                .flat_map(|s| &s.points)
                .any(|p| p.x < g.x.min || p.x > g.x.max || p.y < g.y.min || p.y > g.y.max)
            {
                return Err(Error::ReviewRequired);
            }
            let project = |p: &crate::objects::Position| {
                format!(
                    "{},{}",
                    bounds.x + (p.x - g.x.min) / (g.x.max - g.x.min) * bounds.width,
                    bounds.y + bounds.height
                        - (p.y - g.y.min) / (g.y.max - g.y.min) * bounds.height
                )
            };

            let font = (bounds.width.min(bounds.height) * 0.04).max(1.);
            body += &format!("<path d=\"M{} {}V{}H{}\" fill=\"none\" stroke=\"black\"/><text x=\"{}\" y=\"{}\" font-size=\"{font}\">{} {}</text><text x=\"{}\" y=\"{}\" font-size=\"{font}\">{} {}</text>",bounds.x,bounds.y,bounds.y+bounds.height,bounds.x+bounds.width,bounds.x+bounds.width*0.35,bounds.y+bounds.height-font,xml(&g.x.label),xml(g.x.unit.as_deref().unwrap_or("")),bounds.x+font,bounds.y+font,xml(&g.y.label),xml(g.y.unit.as_deref().unwrap_or("")));

            for s in &g.series {
                body+=&format!("<polyline fill=\"none\" stroke=\"black\" points=\"{}\"><title>{}</title></polyline>",s.points.iter().map(project).collect::<Vec<_>>().join(" "),xml(&s.label));
            }
        }

        _ => return Err(Error::Unsupported),
    }

    Ok(svg_root(body, bounds))
}

impl Store {
    pub fn prepare_derivative(
        &self,
        selection: Selection,
        include_original_ink: bool,
        access: &Access,
    ) -> Result<DerivativePlan> {
        let plan = self
            .derivative_source(selection, include_original_ink, access)?
            .render(&JobControl::new(Duration::from_secs(30))?)?;
        if Self::setting(&self.db, "generation")? != plan.generation {
            return Err(Error::Conflict);
        }
        Ok(plan)
    }

    /// PDF export uses the same reviewed scope and fresh authentication as all
    /// derivative exports. This entry point is converted-only; unsupported image
    /// or embedded-view fidelity is an error, never a silent omission.
    pub fn finish_pdf_export(
        &self,
        plan: DerivativePlan,
        font: &[u8],
        page: Page,
        auth: &mut dyn Authenticator,
        confirmed: bool,
    ) -> Result<crate::render::PdfOutput> {
        if plan.includes_original_ink {
            return Err(Error::Unsupported);
        }
        if !confirmed {
            return Err(Error::ReviewRequired);
        }
        if Self::setting(&self.db, "library_id")? != plan.library
            || Self::setting(&self.db, "generation")? != plan.generation
        {
            return Err(Error::Conflict);
        }
        if !plan.scopes.is_empty() && !auth.verify(Id::new(), &plan.scopes) {
            return Err(Error::Denied);
        }
        let generation = plan.generation.clone();
        let output = plan.render_pdf(font, page, None)?;
        if Self::setting(&self.db, "generation")? != generation {
            return Err(Error::Conflict);
        }
        Ok(output)
    }

    pub fn finish_derivative(
        &self,
        plan: DerivativePlan,
        auth: &mut dyn Authenticator,
        confirmed: bool,
    ) -> Result<ExportBundle> {
        if !confirmed {
            return Err(Error::ReviewRequired);
        }

        if Self::setting(&self.db, "library_id")? != plan.library
            || Self::setting(&self.db, "generation")? != plan.generation
        {
            return Err(Error::Conflict);
        }

        if !plan.scopes.is_empty() && !auth.verify(Id::new(), &plan.scopes) {
            return Err(Error::Denied);
        }

        if Self::setting(&self.db, "generation")? != plan.generation {
            return Err(Error::Conflict);
        }

        Ok(plan.bundle)
    }
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Page {
    pub width_mm: f64,
    pub height_mm: f64,
    pub margin_mm: f64,
}

impl Default for Page {
    fn default() -> Self {
        Self {
            width_mm: 210.,
            height_mm: 297.,
            margin_mm: 12.,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PagePlacement {
    pub item: usize,
    pub page: usize,
    pub y_mm: f64,
    pub scale: f64,
}

/// Layout for indivisible equations, lines and diagrams. The host text shaper supplies
/// measured dimensions. Scaling oversized objects is a separate explicit choice.
pub fn paginate(items: &[(f64, f64)], page: Page, allow_scale: bool) -> Result<Vec<PagePlacement>> {
    if [page.width_mm, page.height_mm, page.margin_mm]
        .iter()
        .any(|v| !v.is_finite())
        || page.margin_mm < 0.
    {
        return Err(Error::Invalid("page"));
    }

    let width = page.width_mm - 2. * page.margin_mm;

    let height = page.height_mm - 2. * page.margin_mm;

    if width <= 0. || height <= 0. {
        return Err(Error::Invalid("page area"));
    }

    let mut result = Vec::new();

    let mut number = 0;

    let mut y = 0.;

    for (item, &(w, h)) in items.iter().enumerate() {
        if !w.is_finite() || !h.is_finite() || w <= 0. || h <= 0. {
            return Err(Error::Invalid("item dimensions"));
        }

        let scale = (width / w).min(height / h).min(1.);

        if scale < 1. && !allow_scale {
            return Err(Error::ReviewRequired);
        }

        if y + h * scale > height {
            number += 1;

            y = 0.;
        }

        result.push(PagePlacement {
            item,
            page: number,
            y_mm: y + page.margin_mm,
            scale,
        });

        y += h * scale;
    }

    Ok(result)
}

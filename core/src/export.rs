//! Deliberate derivative exports. Converted-only is the default contract; source
//! strokes are never obtained unless the reviewed choice explicitly includes them.
mod image_crop;
mod source_ink;
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

#[derive(Debug, Clone)]
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
        Self::require_app(&self.db, access)?;

        let generation = Self::setting(&self.db, "generation")?;

        let library = Self::setting(&self.db, "library_id")?;

        let mut scopes = BTreeSet::new();

        let mut warnings = Vec::new();

        let mut blocks: Vec<Block> = Vec::new();

        let mut crop = None;

        match selection {
            Selection::Blocks(ids) => {
                if ids.iter().collect::<BTreeSet<_>>().len() != ids.len() {
                    return Err(Error::Invalid("duplicate export block"));
                }

                for id in ids {
                    blocks.push(self.block(id, access)?);
                }
            }

            Selection::Document(id) => {
                Self::authorise(&self.db, id, access, false)?;

                scopes.extend(self.protection_scopes(id, None)?);

                let mut q = self.db.prepare(
                    "SELECT id FROM blocks WHERE entity=?1 ORDER BY ordinal,id LIMIT 10001",
                )?;

                for row in q.query_map([id.to_string()], |r| r.get::<_, String>(0))? {
                    blocks.push(self.block(Id::parse(&row?)?, access)?);
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
                    blocks.push(self.block(Id::parse(&row?)?, access)?);
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

                crop = Some(bounds);

                let mut q = self.db.prepare(
                    "SELECT id FROM blocks WHERE entity=?1 ORDER BY ordinal,id LIMIT 10001",
                )?;

                for row in q.query_map([canvas.to_string()], |r| r.get::<_, String>(0))? {
                    let id = Id::parse(&row?)?;

                    let b = self.block(id, access)?;

                    let layout = self.layout(id, access)?.ok_or(Error::ReviewRequired)?;

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

                        blocks.push(b);
                    }
                }
            }
        }

        if blocks.len() > 10000 {
            return Err(Error::ResourceLimit);
        }

        let mut markdown = String::new();

        let mut assets = BTreeMap::new();

        let mut included = false;

        let control = JobControl::new(Duration::from_secs(30))?;
        let mut layer_states = BTreeMap::new();
        for entity in blocks.iter().map(|b| b.entity).collect::<BTreeSet<_>>() {
            if Self::raw_entity(&self.db, entity)?.kind == Kind::Canvas {
                let state = Self::raw_canvas_layers(&self.db, entity)?;
                layer_states.insert(entity, (state.base_visible, state.index()));
            }
        }
        for b in &blocks {
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

            for region in b.regions.iter().filter(|_| !b.has_table_ink()) {
                if let Some(content) = &region.content {
                    let converted = TextConverter.convert(content, Format::Markdown, &control)?;
                    markdown += &converted.text;
                    markdown += "\n\n";
                    warnings.extend(converted.fidelity.unsupported);
                }
            }
            match &b.content {
                Content::Ink => {
                    warnings.push("Unconverted handwriting omitted from converted text.".into())
                }

                Content::Table(t) => {
                    if b.regions.iter().any(|region| region.content.is_none()) {
                        warnings.push("Unconverted table-cell handwriting omitted from converted text; existing editable cell values are retained.".into());
                    }

                    let text = t
                        .rows
                        .iter()
                        .map(|row| {
                            row.iter()
                                .map(|c| {
                                    c.value.literal().replace('|', "\\|").replace('\n', "<br>")
                                })
                                .collect::<Vec<_>>()
                                .join(" | ")
                        })
                        .collect::<Vec<_>>();

                    markdown += &format!(
                        "| {} |\n| {} |\n",
                        text[0],
                        vec!["---"; t.rows[0].len()].join(" | ")
                    );

                    for row in &text[1..] {
                        markdown += &format!("| {row} |\n");
                    }

                    warnings.push("Markdown table presentation uses the first row as a header; cell maths structure remains native-only.".into());
                }

                Content::Diagram(_) | Content::Graph(_) => {
                    let bounds = crop
                        .or(self.layout(b.id, access)?.map(|l| l.bounds))
                        .unwrap_or(Rect {
                            x: 0.,
                            y: 0.,
                            width: 100.,
                            height: 100.,
                        });

                    if crop.is_some() {
                        if let Content::Diagram(diagram) = &b.content {
                            for part in &diagram.parts {
                                let contained = match &part.shape {
                                    Shape::Polyline(points) => {
                                        points.iter().all(|p| bounds.contains(*p))
                                    }

                                    Shape::Rectangle(r) | Shape::Ellipse(r) => {
                                        bounds.contains(crate::objects::Position { x: r.x, y: r.y })
                                            && bounds.contains(crate::objects::Position {
                                                x: r.x + r.width,
                                                y: r.y + r.height,
                                            })
                                    }
                                };

                                if !contained {
                                    return Err(Error::ReviewRequired);
                                }
                            }
                        }
                    }

                    let name = format!("object-{}.svg", b.id);

                    assets.insert(
                        name.clone(),
                        technical_svg(&b.content, bounds)?.into_bytes(),
                    );

                    markdown += &format!("![Technical object]({name})\n");

                    warnings.push("Vector derivative retains drawn data; component editability requires native format.".into());
                }

                Content::Image(i) => {
                    let (mime, bytes) = self.image_bytes(i.asset, access)?;
                    let (name, output) = if let Some(crop) = i.crop {
                        let (cropped, rounded) =
                            image_crop::crop_image(&mime, &bytes, crop, &control)?;
                        warnings.push("Cropped image exported as 8-bit RGBA PNG without original metadata; original bytes remain native-only.".into());
                        if rounded {
                            warnings.push(
                                "Image crop rounded inward to complete source pixels.".into(),
                            );
                        }
                        (format!("image-{}-crop-{}.png", i.asset, b.id), cropped)
                    } else {
                        (
                            format!(
                                "image-{}.{}",
                                i.asset,
                                if mime == "image/png" { "png" } else { "jpg" }
                            ),
                            bytes,
                        )
                    };
                    assets.insert(name.clone(), output);
                    markdown +=
                        &format!("![{}]({name})\n", i.caption.replace(['[', ']', '\n'], " "));
                }

                c => {
                    let converted = TextConverter.convert(c, Format::Markdown, &control)?;

                    markdown += &converted.text;

                    warnings.extend(converted.fidelity.unsupported);
                }
            }

            if include_original_ink && !b.source_ids.is_empty() {
                let source = self.source_ink(b.id, access)?;

                let mut body = String::new();

                let mut min_x = f64::INFINITY;

                let mut min_y = f64::INFINITY;

                let mut max_x = f64::NEG_INFINITY;

                let mut max_y = f64::NEG_INFINITY;

                for s in source {
                    if !b.visible_strokes.contains(&s.id) {
                        continue;
                    }

                    let (colour, width) = if let Some(style) = s.style {
                        (style.colour_rgba, style.width)
                    } else {
                        warnings.push("Legacy ink appearance was not recorded; original-ink SVG uses a black, unit-width fallback.".into());
                        ([0, 0, 0, 255], 1.)
                    };
                    if let Some((derived, bounds)) = source_ink::pressure_svg(&s)? {
                        if s.points.iter().any(|p| p.pressure.is_none()) {
                            warnings.push("Pressure was unavailable for some source samples; their recorded base width is used.".into());
                        }
                        if s.points
                            .iter()
                            .any(|p| p.tilt_rad.is_some() || p.orientation_rad.is_some())
                        {
                            warnings.push(
                                "Tilt/orientation-dependent nib appearance is not reconstructed."
                                    .into(),
                            );
                        }
                        if s.tool == crate::ink::Tool::Eraser {
                            warnings.push("Eraser observations are shown as source paths, not replayed as destructive edits.".into());
                        }
                        min_x = min_x.min(bounds.x);
                        min_y = min_y.min(bounds.y);
                        max_x = max_x.max(bounds.x + bounds.width);
                        max_y = max_y.max(bounds.y + bounds.height);
                        body += &derived;
                        continue;
                    }
                    if s.points.iter().any(|p| {
                        p.pressure.is_some() || p.tilt_rad.is_some() || p.orientation_rad.is_some()
                    }) {
                        warnings.push("Original-ink SVG preserves recorded base colour/width; pressure/tilt-dependent appearance is not reconstructed.".into());
                    }
                    if s.tool == crate::ink::Tool::Eraser {
                        warnings.push("Eraser observations are shown as source paths, not replayed as destructive edits.".into());
                    }
                    let margin = width / 2.;
                    for p in &s.points {
                        min_x = min_x.min(p.x - margin);
                        min_y = min_y.min(p.y - margin);
                        max_x = max_x.max(p.x + margin);
                        max_y = max_y.max(p.y + margin);
                    }
                    let rgb = format!("#{:02x}{:02x}{:02x}", colour[0], colour[1], colour[2]);
                    let opacity = colour[3] as f64 / 255.;
                    if s.points.len() == 1 {
                        let p = &s.points[0];
                        body += &format!("<circle cx=\"{}\" cy=\"{}\" r=\"{margin}\" fill=\"{rgb}\" fill-opacity=\"{opacity}\"/>", p.x, p.y);
                    } else {
                        body += &format!("<polyline fill=\"none\" stroke=\"{rgb}\" stroke-opacity=\"{opacity}\" stroke-width=\"{width}\" stroke-linecap=\"round\" stroke-linejoin=\"round\" points=\"{}\"/>",s.points.iter().map(|p| format!("{},{}", p.x, p.y)).collect::<Vec<_>>().join(" "));
                    }
                }

                if !body.is_empty() {
                    let bounds = Rect {
                        x: min_x - 1.,
                        y: min_y - 1.,
                        width: (max_x - min_x + 2.).max(2.),
                        height: (max_y - min_y + 2.).max(2.),
                    };

                    bounds.validate()?;

                    let name = format!("original-{}.svg", b.id);

                    assets.insert(name.clone(), svg_root(body, bounds).into_bytes());

                    markdown += &format!("\n![Original handwriting]({name})\n");

                    included = true;
                }
            }

            markdown += "\n\n";
        }

        warnings.sort();

        warnings.dedup();

        if Self::setting(&self.db, "generation")? != generation {
            return Err(Error::Conflict);
        }

        let mut render_contents = Vec::new();
        for block in &blocks {
            if !block.visible {
                continue;
            }
            render_contents.push(block.content.clone());
            if !block.has_table_ink() {
                render_contents.extend(block.regions.iter().filter_map(|r| r.content.clone()));
            }
        }
        let bundle = ExportBundle {
            markdown,
            assets,
            warnings: warnings.clone(),
            includes_original_ink: included,
        };

        Ok(DerivativePlan {
            library,
            generation,
            scopes,
            bundle,
            render_contents,
            selected_blocks: blocks.len(),
            warnings,
            includes_original_ink: included,
        })
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
        let mut output = crate::render::render_pdf(&plan.render_contents, font, page)?;
        if Self::setting(&self.db, "generation")? != plan.generation {
            return Err(Error::Conflict);
        }
        output.warnings.extend(plan.warnings);
        output.warnings.sort();
        output.warnings.dedup();
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

#[derive(Debug, Clone, Copy)]
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

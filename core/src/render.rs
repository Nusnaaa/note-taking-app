//! Deterministic vector PDF derivatives. The caller supplies a local font explicitly;
//! no fonts, stylesheets, images or other resources are fetched from a URI.
//! PDF uses points internally (the standard); the public page contract uses mm.
use crate::{
    content::{Content, MathForm, MathNode},
    export::Page,
    Error, Result,
};
use pdf_writer::{Finish, Name, Pdf, Ref};
use std::collections::HashMap;

pub struct PdfOutput {
    pub bytes: Vec<u8>,
    pub pages: usize,
    pub warnings: Vec<String>,
}
struct BoxLayout {
    svg: String,
    width: f64,
    height: f64,
    baseline: f64,
}
struct Typesetter<'a> {
    font: &'a [u8],
}
fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn at(b: &BoxLayout, x: f64, y: f64) -> String {
    format!("<g transform=\"translate({x},{y})\">{}</g>", b.svg)
}
impl Typesetter<'_> {
    fn text(&self, text: &str, size: f64) -> Result<BoxLayout> {
        let face = rustybuzz::Face::from_slice(self.font, 0).ok_or(Error::Invalid("font"))?;
        if text.chars().any(|c| face.glyph_index(c).is_none()) {
            return Err(Error::Unsupported);
        }
        let mut buffer = rustybuzz::UnicodeBuffer::new();
        buffer.push_str(text);
        buffer.guess_segment_properties();
        let shaped = rustybuzz::shape(&face, &[], buffer);
        let width = shaped
            .glyph_positions()
            .iter()
            .map(|p| p.x_advance as f64)
            .sum::<f64>()
            * size
            / face.units_per_em() as f64;
        Ok(BoxLayout {
            svg: format!(
                "<text x=\"0\" y=\"{size}\" font-size=\"{size}\" xml:space=\"preserve\">{}</text>",
                xml(text)
            ),
            width: width.max(0.),
            height: size * 1.3,
            baseline: size,
        })
    }
    fn row(&self, boxes: Vec<BoxLayout>) -> BoxLayout {
        let baseline = boxes.iter().map(|b| b.baseline).fold(0., f64::max);
        let descent = boxes
            .iter()
            .map(|b| b.height - b.baseline)
            .fold(0., f64::max);
        let mut x = 0.;
        let mut svg = String::new();
        for b in boxes {
            svg += &at(&b, x, baseline - b.baseline);
            x += b.width;
        }
        BoxLayout {
            svg,
            width: x,
            height: baseline + descent,
            baseline,
        }
    }
    fn math(&self, n: &MathNode, size: f64) -> Result<BoxLayout> {
        Ok(match &n.form {
            MathForm::Token(t)
            | MathForm::Text(t)
            | MathForm::Unknown(t)
            | MathForm::Unparsed(t) => self.text(t, size)?,
            MathForm::Hole => self.text("□", size)?,
            MathForm::Row(v) => self.row(
                v.iter()
                    .map(|n| self.math(n, size))
                    .collect::<Result<_>>()?,
            ),
            MathForm::Fraction {
                numerator,
                denominator,
            } => {
                let a = self.math(numerator, size * 0.9)?;
                let b = self.math(denominator, size * 0.9)?;
                let width = a.width.max(b.width) + 8.;
                let line = a.height + 3.;
                BoxLayout {
                    svg: format!(
                        "{}<path d=\"M0 {line}H{width}\" stroke=\"black\" stroke-width=\"0.7\"/>{}",
                        at(&a, (width - a.width) / 2., 0.),
                        at(&b, (width - b.width) / 2., line + 3.)
                    ),
                    width,
                    height: line + 3. + b.height,
                    baseline: line + size * 0.3,
                }
            }
            MathForm::Scripts { base, sub, sup } => {
                let b = self.math(base, size)?;
                let sub = sub
                    .as_deref()
                    .map(|n| self.math(n, size * 0.7))
                    .transpose()?;
                let sup = sup
                    .as_deref()
                    .map(|n| self.math(n, size * 0.7))
                    .transpose()?;
                let top = sup.as_ref().map_or(0., |s| s.height * 0.7);
                let mut svg = at(&b, 0., top);
                let mut width: f64 = 0.;
                let mut bottom: f64 = 0.;
                if let Some(s) = sup {
                    svg += &at(&s, b.width, 0.);
                    width = width.max(s.width);
                }
                if let Some(s) = sub {
                    svg += &at(&s, b.width, top + b.baseline);
                    width = width.max(s.width);
                    bottom = s.height;
                }
                BoxLayout {
                    svg,
                    width: b.width + width,
                    height: top + b.height.max(b.baseline + bottom),
                    baseline: top + b.baseline,
                }
            }
            MathForm::Root { radicand, index } => {
                let b = self.math(radicand, size)?;
                let symbol = self.text("√", size * 1.2)?;
                let mut result = self.row(vec![symbol, b]);
                let y = 1.;
                result.svg += &format!(
                    "<path d=\"M{} {y}H{}\" stroke=\"black\" stroke-width=\"0.7\"/>",
                    size * 0.8,
                    result.width
                );
                if let Some(i) = index {
                    let i = self.math(i, size * 0.5)?;
                    result.svg = at(&i, 0., 0.)
                        + &format!(
                            "<g transform=\"translate(0,{})\">{}</g>",
                            i.height * 0.4,
                            result.svg
                        );
                    result.height += i.height * 0.4;
                    result.baseline += i.height * 0.4;
                }
                result
            }
            MathForm::Fence { open, body, close } => {
                let mut v = vec![self.text(open, size)?, self.math(body, size)?];
                if let Some(c) = close {
                    v.push(self.text(c, size)?)
                }
                self.row(v)
            }
            MathForm::Matrix(rows) => {
                let rows = rows
                    .iter()
                    .map(|r| {
                        r.iter()
                            .map(|n| self.math(n, size))
                            .collect::<Result<Vec<_>>>()
                    })
                    .collect::<Result<Vec<_>>>()?;
                let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
                let mut widths = vec![0f64; columns];
                for r in &rows {
                    for (i, b) in r.iter().enumerate() {
                        widths[i] = widths[i].max(b.width)
                    }
                }
                let mut y = 0.;
                let mut svg = String::new();
                for r in rows {
                    let baseline = r.iter().map(|b| b.baseline).fold(0., f64::max);
                    let height = r.iter().map(|b| b.height).fold(0., f64::max);
                    let mut x = 0.;
                    for (i, b) in r.iter().enumerate() {
                        svg += &at(b, x, y + baseline - b.baseline);
                        x += widths[i] + size;
                    }
                    y += height + size * 0.3;
                }
                BoxLayout {
                    svg,
                    width: widths.iter().sum::<f64>() + size * columns.saturating_sub(1) as f64,
                    height: y,
                    baseline: y / 2. + size * 0.3,
                }
            }
            MathForm::AlignedLines(lines) => {
                let mut svg = String::new();
                let mut y = 0.;
                let mut width: f64 = 0.;
                for n in lines {
                    let b = self.math(n, size)?;
                    svg += &at(&b, 0., y);
                    width = width.max(b.width);
                    y += b.height + 4.;
                }
                BoxLayout {
                    svg,
                    width,
                    height: y,
                    baseline: size,
                }
            }
            MathForm::Alternatives(v) => {
                let mut boxes = vec![];
                for (i, n) in v.iter().enumerate() {
                    if i > 0 {
                        boxes.push(self.text(" | ", size)?)
                    }
                    boxes.push(self.math(n, size)?);
                }
                self.row(boxes)
            }
        })
    }
}

/// Pure rendering of an already authorised selection. Does not accept arbitrary SVG.
/// Font coverage errors fail explicitly; missing glyphs are never silently omitted.
pub fn render_pdf(contents: &[Content], font: &[u8], page: Page) -> Result<PdfOutput> {
    render_pdf_control(contents, font, page, None)
}
pub(crate) fn render_pdf_control(
    contents: &[Content],
    font: &[u8],
    page: Page,
    control: Option<&crate::processing::JobControl>,
) -> Result<PdfOutput> {
    let check = || control.map_or(Ok(()), |control| control.check());
    check()?;

    if font.len() > 32 * 1024 * 1024 || contents.len() > 10000 {
        return Err(Error::ResourceLimit);
    }
    if [page.width_mm, page.height_mm, page.margin_mm]
        .iter()
        .any(|x| !x.is_finite())
        || page.margin_mm < 0.
        || page.width_mm <= page.margin_mm * 2.
        || page.height_mm <= page.margin_mm * 2.
        || page.width_mm > 2000.
        || page.height_mm > 2000.
    {
        return Err(Error::Invalid("page"));
    }
    let mut options = svg2pdf::usvg::Options::default();
    options.fontdb_mut().load_font_data(font.to_vec());
    let family = options
        .fontdb
        .faces()
        .next()
        .and_then(|f| f.families.first())
        .map(|f| f.0.clone())
        .ok_or(Error::Invalid("font"))?;
    options.font_family = family.clone();
    // Resolve generated inline data only. Never permit arbitrary filesystem references.
    options.resources_dir = None;
    let typesetter = Typesetter { font };
    let mut items = Vec::new();
    let mut warnings = vec![];
    let points = 72. / 25.4;
    let width = page.width_mm * points;
    let height = page.height_mm * points;
    let margin = page.margin_mm * points;
    let available = width - 2. * margin;
    for content in contents {
        check()?;
        content.validate()?;
        match content {
            Content::Maths(n) => items.push(typesetter.math(n, 12.)?),
            Content::Text(s) | Content::Numeric(s) | Content::Code { text: s, .. } => {
                // Preserve code characters/indentation; explicit visual wrapping introduces
                // no changes to the native editable string.
                for line in s.split('\n') {
                    check()?;
                    let mut part = String::new();
                    for c in line.chars() {
                        check()?;
                        let appended = if c == '\t' {
                            "    ".into()
                        } else {
                            c.to_string()
                        };
                        let candidate = part.clone() + &appended;
                        if !part.is_empty() && typesetter.text(&candidate, 12.)?.width > available {
                            items.push(typesetter.text(&part, 12.)?);
                            part.clear();
                        }
                        part += &appended;
                    }
                    items.push(typesetter.text(&part, 12.)?);
                }
                if s.contains('\t') {
                    warnings
                        .push("Tabs use four spaces in this PDF; native code is unchanged.".into())
                }
            }
            Content::Table(t) => {
                // Cell contents are typeset independently. Each row is indivisible.
                let columns = t.rows[0].len();
                let cell_width = available / columns as f64;
                for row in &t.rows {
                    check()?;
                    let mut boxes = Vec::new();
                    for cell in row {
                        boxes.push(match &cell.value {
                            crate::objects::CellValue::Maths(n) => typesetter.math(n, 11.)?,
                            v => typesetter.text(&v.literal(), 11.)?,
                        });
                    }
                    let h = boxes.iter().map(|b| b.height).fold(0., f64::max) + 10.;
                    let mut svg = String::new();
                    for (i, b) in boxes.iter().enumerate() {
                        if b.width + 8. > cell_width {
                            return Err(Error::ReviewRequired);
                        }
                        let x = i as f64 * cell_width;
                        svg+=&format!("<rect x=\"{x}\" y=\"0\" width=\"{cell_width}\" height=\"{h}\" fill=\"none\" stroke=\"black\" stroke-width=\"0.5\"/>{}",at(b,x+4.,4.));
                    }
                    items.push(BoxLayout {
                        svg,
                        width: available,
                        height: h,
                        baseline: 12.,
                    });
                }
            }
            Content::Diagram(_) | Content::Graph(_) => {
                let svg = crate::export::technical_svg(
                    content,
                    crate::objects::Rect {
                        x: 0.,
                        y: 0.,
                        width: available,
                        height: 180.,
                    },
                )?;
                items.push(BoxLayout {
                    svg,
                    width: available,
                    height: 180.,
                    baseline: 12.,
                });
            }
            Content::Ink => {
                warnings.push("Unconverted handwriting is omitted from converted-only PDF.".into())
            }
            Content::Image(_) => return Err(Error::Unsupported),
        }
    }
    let mut pages = vec![];
    let mut body = String::new();
    let mut y = margin;
    for mut b in items {
        check()?;
        if b.width > available {
            let scale = available / b.width;
            b.svg = format!("<g transform=\"scale({scale})\">{}</g>", b.svg);
            b.width *= scale;
            b.height *= scale;
            warnings.push("A wide object was scaled to the page width.".into());
        }
        if b.height > height - margin * 2. {
            return Err(Error::ReviewRequired);
        }
        if y + b.height > height - margin {
            pages.push(body);
            body = String::new();
            y = margin;
        }
        body += &at(&b, margin, y);
        y += b.height + 6.;
    }
    if !body.is_empty() || pages.is_empty() {
        pages.push(body)
    }
    if pages.len() > 10000 {
        return Err(Error::ResourceLimit);
    }
    let mut alloc = Ref::new(1);
    let catalog = alloc.bump();
    let tree_id = alloc.bump();
    let page_ids = pages.iter().map(|_| alloc.bump()).collect::<Vec<_>>();
    let mut pdf = Pdf::new();
    pdf.catalog(catalog).pages(tree_id);
    pdf.pages(tree_id)
        .kids(page_ids.iter().copied())
        .count(page_ids.len() as i32);
    for (body, id) in pages.iter().zip(page_ids) {
        check()?;
        let svg=format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\"><g font-family=\"{}\" fill=\"black\">{body}</g></svg>",xml(&family));
        let tree = svg2pdf::usvg::Tree::from_str(&svg, &options)
            .map_err(|_| Error::Invalid("rendered SVG"))?;
        let (chunk, svg_id) = svg2pdf::to_chunk(
            &tree,
            svg2pdf::ConversionOptions {
                pdfa: true,
                ..Default::default()
            },
        )
        .map_err(|_| Error::Unsupported)?;
        check()?;
        let mut map = HashMap::new();
        let chunk = chunk.renumber(|old| *map.entry(old).or_insert_with(|| alloc.bump()));
        let svg_id = map[&svg_id];
        let content_id = alloc.bump();
        let mut page = pdf.page(id);
        page.parent(tree_id)
            .media_box(pdf_writer::Rect::new(0., 0., width as f32, height as f32))
            .contents(content_id);
        page.resources()
            .x_objects()
            .pair(Name(b"PageArtwork"), svg_id);
        page.finish();
        let mut stream = pdf_writer::Content::new();
        stream
            .transform([width as f32, 0., 0., height as f32, 0., 0.])
            .x_object(Name(b"PageArtwork"));
        pdf.stream(content_id, &stream.finish());
        if chunk.as_bytes().len() > (64usize * 1024 * 1024).saturating_sub(pdf.as_bytes().len()) {
            return Err(Error::ResourceLimit);
        }
        pdf.extend(&chunk);
    }
    warnings.sort();
    warnings.dedup();
    let bytes = pdf.finish();
    if bytes.len() > 64 * 1024 * 1024 {
        return Err(Error::ResourceLimit);
    }
    check()?;
    Ok(PdfOutput {
        bytes,
        pages: pages.len(),
        warnings,
    })
}

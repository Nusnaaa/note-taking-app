//! Explicit current table geometry, separate from original-coordinate assets.
use super::*;
use crate::ink::{Stroke, Transform};
const LIMIT: usize = 8 * 1024 * 1024;
fn matrix(t: Transform) -> String {
    format!("matrix({} {} {} {} {} {})", t.a, t.b, t.c, t.d, t.tx, t.ty)
}
fn extend(bounds: &mut [f64; 4], rect: Rect, t: Transform) -> Result<()> {
    rect.validate()?;
    t.validate()?;
    for (x, y) in [
        (rect.x, rect.y),
        (rect.x + rect.width, rect.y),
        (rect.x, rect.y + rect.height),
        (rect.x + rect.width, rect.y + rect.height),
    ] {
        let (x, y) = (t.a * x + t.c * y + t.tx, t.b * x + t.d * y + t.ty);
        if !x.is_finite() || !y.is_finite() {
            return Err(Error::Invalid("table SVG geometry"));
        }
        bounds[0] = bounds[0].min(x);
        bounds[1] = bounds[1].min(y);
        bounds[2] = bounds[2].max(x);
        bounds[3] = bounds[3].max(y);
    }
    Ok(())
}
fn append(body: &mut String, value: &str) -> Result<()> {
    if value.len() > LIMIT.saturating_sub(body.len()) {
        return Err(Error::ResourceLimit);
    }
    body.push_str(value);
    Ok(())
}
fn fixed_stroke(s: &Stroke) -> Result<(String, Rect)> {
    s.validate_persisted()?;
    let (colour, width) = s
        .style
        .map_or(([0, 0, 0, 255], 1.), |s| (s.colour_rgba, s.width));
    let (mut left, mut top, mut right, mut bottom) = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    let mut points = String::new();
    for p in &s.points {
        left = left.min(p.x - width / 2.);
        top = top.min(p.y - width / 2.);
        right = right.max(p.x + width / 2.);
        bottom = bottom.max(p.y + width / 2.);
        append(&mut points, &format!("{},{} ", p.x, p.y))?;
    }
    let rgb = format!("#{:02x}{:02x}{:02x}", colour[0], colour[1], colour[2]);
    let opacity = f64::from(colour[3]) / 255.;
    let svg = if s.points.len() == 1 {
        let p = &s.points[0];
        format!(
            "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{rgb}\" fill-opacity=\"{opacity}\"/>",
            p.x,
            p.y,
            width / 2.
        )
    } else {
        format!("<polyline fill=\"none\" stroke=\"{rgb}\" stroke-opacity=\"{opacity}\" stroke-width=\"{width}\" stroke-linecap=\"round\" stroke-linejoin=\"round\" points=\"{points}\"/>")
    };
    Ok((
        svg,
        Rect {
            x: left,
            y: top,
            width: (right - left).max(1.),
            height: (bottom - top).max(1.),
        },
    ))
}
pub(super) fn table_ink_svg(
    block: &Block,
    source: &[Stroke],
    control: &JobControl,
) -> Result<String> {
    let Content::Table(table) = &block.content else {
        return Err(Error::Invalid("table SVG"));
    };
    let layout = table.layout.as_ref().ok_or(Error::ReviewRequired)?;
    table.validate()?;
    layout.validate(table)?;
    let mut body = String::new();
    let mut bounds = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    append(
        &mut body,
        &format!(
            "<g transform=\"{}\" fill=\"none\" stroke=\"black\" stroke-width=\"1\">",
            matrix(block.transform)
        ),
    )?;
    // Linear traversal; no repeated per-cell table validation/search.
    let mut y = layout.origin.y;
    for height in &layout.row_heights {
        control.check()?;
        let mut x = layout.origin.x;
        for width in &layout.column_widths {
            let rect = Rect {
                x: x - 0.5,
                y: y - 0.5,
                width: width + 1.,
                height: height + 1.,
            };
            extend(&mut bounds, rect, block.transform)?;
            append(
                &mut body,
                &format!("<rect x=\"{x}\" y=\"{y}\" width=\"{width}\" height=\"{height}\"/>"),
            )?;
            x += width;
        }
        y += height;
    }
    append(&mut body, "</g>")?;
    for stroke in source {
        control.check()?;
        let placement = Store::place_table_stroke(block, stroke.clone())?;
        let s = &placement.stroke;
        let (svg, rect) =
            match source_ink::pressure_svg_budget(s, LIMIT.saturating_sub(body.len()))? {
                Some(value) => value,
                None => fixed_stroke(s)?,
            };
        extend(&mut bounds, rect, placement.source_to_surface)?;
        append(
            &mut body,
            &format!("<g transform=\"{}\">", matrix(placement.source_to_surface)),
        )?;
        append(&mut body, &svg)?;
        append(&mut body, "</g>")?;
    }
    let rect = Rect {
        x: bounds[0],
        y: bounds[1],
        width: bounds[2] - bounds[0],
        height: bounds[3] - bounds[1],
    };
    rect.validate()?;
    let svg = svg_root(body, rect);
    if svg.len() > LIMIT {
        return Err(Error::ResourceLimit);
    }
    Ok(svg)
}

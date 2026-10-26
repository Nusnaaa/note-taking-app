//! Vector derivative of explicitly chosen pressure appearance. Observations and
//! source fingerprints are never modified; no pressure samples are fabricated.
use crate::{ink::Stroke, objects::Rect, Error, Result};
pub(super) fn pressure_svg(stroke: &Stroke) -> Result<Option<(String, Rect)>> {
    let Some(style) = stroke.style.filter(|s| s.pressure_response.is_some()) else {
        return Ok(None);
    };
    stroke.validate_persisted()?;
    let colour = style.colour_rgba;
    let mut svg = format!(
        "<g fill=\"#{:02x}{:02x}{:02x}\" opacity=\"{}\">",
        colour[0],
        colour[1],
        colour[2],
        f64::from(colour[3]) / 255.
    );
    let (mut left, mut top, mut right, mut bottom) = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    for p in &stroke.points {
        let radius = style.width_for(p.pressure)? / 2.;
        left = left.min(p.x - radius);
        right = right.max(p.x + radius);
        top = top.min(p.y - radius);
        bottom = bottom.max(p.y + radius);
        svg += &format!("<circle cx=\"{}\" cy=\"{}\" r=\"{radius}\"/>", p.x, p.y);
    }
    for pair in stroke.points.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let length = dx.hypot(dy);
        if !length.is_finite() {
            return Err(Error::Invalid("pressure geometry"));
        }
        if length == 0. {
            continue;
        }
        let (nx, ny) = (-dy / length, dx / length);
        let ra = style.width_for(a.pressure)? / 2.;
        let rb = style.width_for(b.pressure)? / 2.;
        let vertices = [
            (a.x + nx * ra, a.y + ny * ra),
            (b.x + nx * rb, b.y + ny * rb),
            (b.x - nx * rb, b.y - ny * rb),
            (a.x - nx * ra, a.y - ny * ra),
        ];
        if vertices
            .iter()
            .any(|(x, y)| !x.is_finite() || !y.is_finite())
        {
            return Err(Error::Invalid("pressure geometry"));
        }
        svg += &format!(
            "<polygon points=\"{}\"/>",
            vertices
                .iter()
                .map(|(x, y)| format!("{x},{y}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
    }
    svg += "</g>";
    let bounds = Rect {
        x: left,
        y: top,
        width: (right - left).max(1.),
        height: (bottom - top).max(1.),
    };
    bounds.validate()?;
    Ok(Some((svg, bounds)))
}

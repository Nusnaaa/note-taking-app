//! Vector derivative of explicitly chosen pressure appearance. Observations and
//! source fingerprints are never modified; no pressure samples are fabricated.
use crate::{ink::Stroke, objects::Rect, Error, Result};
#[cfg(test)]
pub(super) fn pressure_svg(stroke: &Stroke) -> Result<Option<(String, Rect)>> {
    pressure_svg_budget(stroke, 8 * 1024 * 1024)
}
fn push(svg: &mut String, value: &str, limit: usize) -> Result<()> {
    if value.len() > limit.saturating_sub(svg.len()) {
        return Err(Error::ResourceLimit);
    }
    svg.push_str(value);
    Ok(())
}
pub(super) fn pressure_svg_budget(stroke: &Stroke, limit: usize) -> Result<Option<(String, Rect)>> {
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
        push(
            &mut svg,
            &format!("<circle cx=\"{}\" cy=\"{}\" r=\"{radius}\"/>", p.x, p.y),
            limit,
        )?;
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
        push(
            &mut svg,
            &format!(
                "<polygon points=\"{}\"/>",
                vertices
                    .iter()
                    .map(|(x, y)| format!("{x},{y}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
            limit,
        )?;
    }
    push(&mut svg, "</g>", limit)?;
    let bounds = Rect {
        x: left,
        y: top,
        width: (right - left).max(1.),
        height: (bottom - top).max(1.),
    };
    bounds.validate()?;
    Ok(Some((svg, bounds)))
}

pub(super) fn point_list(
    stroke: &Stroke,
    control: &crate::processing::JobControl,
) -> Result<String> {
    let mut points = String::new();
    for (index, p) in stroke.points.iter().enumerate() {
        control.check()?;
        if index > 0 {
            push(&mut points, " ", 8 * 1024 * 1024)?;
        }
        push(&mut points, &format!("{},{}", p.x, p.y), 8 * 1024 * 1024)?;
    }
    Ok(points)
}

#[cfg(test)]
mod budget_tests {
    use super::*;
    use crate::{
        ink::{Point, PressureResponse, StrokeStyle, Tool},
        Id,
    };
    #[test]
    fn pressure_geometry_budget_rejects_partial_svg_and_preserves_observations() {
        let point = Point {
            x: 1.,
            y: 2.,
            time_us: 1,
            pressure: Some(0.),
            tilt_rad: None,
            orientation_rad: None,
            buttons: None,
        };
        let stroke = Stroke {
            id: Id::new(),
            capture_session: Id::new(),
            sequence: 0,
            pointer_id: 1,
            coordinate_space: "synthetic-budget".into(),
            tool: Tool::Pen,
            end: Default::default(),
            style: Some(StrokeStyle {
                colour_rgba: [0, 0, 0, 255],
                width: 2.,
                pressure_response: Some(PressureResponse {
                    input_min: 0.,
                    input_max: 1.,
                    minimum_factor: 0.5,
                    maximum_factor: 1.,
                    exponent: 1.,
                }),
            }),
            points: vec![
                point.clone(),
                Point {
                    x: 3.,
                    time_us: 2,
                    ..point
                },
            ],
        };
        let original = serde_json::to_vec(&stroke).unwrap();
        let (svg, bounds) = pressure_svg(&stroke).unwrap().unwrap();
        assert!(svg.contains("<circle"));
        assert!(svg.contains("<polygon"));
        assert!(matches!(
            pressure_svg_budget(&stroke, svg.len() - 1),
            Err(Error::ResourceLimit)
        ));
        assert_eq!(
            pressure_svg_budget(&stroke, svg.len()).unwrap(),
            Some((svg, bounds))
        );
        assert_eq!(serde_json::to_vec(&stroke).unwrap(), original);
    }
}

//! Explicit source-space whole-stroke eraser geometry. The caller maps a gesture
//! into the named source space; this module never guesses a view/device mapping.
use super::Stroke;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EraserGesture {
    pub coordinate_space: String,
    pub path: Vec<[f64; 2]>,
    /// Distance to the observed centreline, in the named source-space units.
    pub radius: f64,
}
impl EraserGesture {
    pub fn validate(&self) -> Result<()> {
        if self.coordinate_space.is_empty()
            || self.coordinate_space.len() > 256
            || self.path.is_empty()
            || !self.radius.is_finite()
            || self.radius <= 0.
            || self.path.iter().flatten().any(|v| !v.is_finite())
        {
            return Err(Error::Invalid("eraser gesture"));
        }
        if self.path.len() > 4096 {
            return Err(Error::ResourceLimit);
        }
        Ok(())
    }
    pub(crate) fn hits(&self, stroke: &Stroke, remaining: &mut usize) -> Result<bool> {
        if stroke.coordinate_space != self.coordinate_space {
            return Err(Error::Invalid("eraser source space"));
        }
        // Reserve the entire candidate before inspecting geometry, including
        // early hits, so work admission does not depend on contact location.
        let work = self
            .path
            .len()
            .saturating_sub(1)
            .max(1)
            .checked_mul(stroke.points.len().saturating_sub(1).max(1))
            .ok_or(Error::ResourceLimit)?;
        *remaining = remaining.checked_sub(work).ok_or(Error::ResourceLimit)?;
        if stroke.tool == super::Tool::Eraser {
            return Ok(false);
        }
        for i in 0..self.path.len().saturating_sub(1).max(1) {
            let a = self.path[i];
            let b = self.path[(i + 1).min(self.path.len() - 1)];
            for j in 0..stroke.points.len().saturating_sub(1).max(1) {
                let p = &stroke.points[j];
                let q = &stroke.points[(j + 1).min(stroke.points.len() - 1)];
                if segment_distance(a, b, [p.x, p.y], [q.x, q.y])? <= self.radius {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
}
fn vector(a: [f64; 2], b: [f64; 2]) -> Result<[f64; 2]> {
    let v = [b[0] - a[0], b[1] - a[1]];
    if v.iter().any(|n| !n.is_finite()) {
        return Err(Error::Invalid("eraser geometry"));
    }
    Ok(v)
}
fn dot(a: [f64; 2], b: [f64; 2]) -> Result<f64> {
    let n = a[0] * b[0] + a[1] * b[1];
    if !n.is_finite() {
        return Err(Error::Invalid("eraser geometry"));
    }
    Ok(n)
}
fn point_distance(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> Result<f64> {
    let ab = vector(a, b)?;
    let ap = vector(a, p)?;
    let length = ab[0].hypot(ab[1]);
    if !length.is_finite() {
        return Err(Error::Invalid("eraser geometry"));
    }
    if length == 0. {
        let distance = ap[0].hypot(ap[1]);
        return if distance.is_finite() {
            Ok(distance)
        } else {
            Err(Error::Invalid("eraser geometry"))
        };
    }
    let unit = [ab[0] / length, ab[1] / length];
    let along = dot(ap, unit)?;
    let distance = if along <= 0. {
        ap[0].hypot(ap[1])
    } else if along >= length {
        let bp = vector(b, p)?;
        bp[0].hypot(bp[1])
    } else {
        dot(ap, [-unit[1], unit[0]])?.abs()
    };
    if !distance.is_finite() {
        return Err(Error::Invalid("eraser geometry"));
    }
    Ok(distance)
}
fn sides(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> Result<bool> {
    let ab = vector(a, b)?;
    let length = ab[0].hypot(ab[1]);
    if !length.is_finite() {
        return Err(Error::Invalid("eraser geometry"));
    }
    if length == 0. {
        return Ok(false);
    }
    let normal = [-ab[1] / length, ab[0] / length];
    let c = dot(vector(a, c)?, normal)?;
    let d = dot(vector(a, d)?, normal)?;
    Ok((c <= 0. && d >= 0.) || (d <= 0. && c >= 0.))
}
fn segment_distance(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> Result<f64> {
    let overlap =
        (0..2).all(|i| a[i].min(b[i]) <= c[i].max(d[i]) && c[i].min(d[i]) <= a[i].max(b[i]));
    if overlap && sides(a, b, c, d)? && sides(c, d, a, b)? {
        return Ok(0.);
    }
    Ok(point_distance(a, c, d)?
        .min(point_distance(b, c, d)?)
        .min(point_distance(c, a, b)?)
        .min(point_distance(d, a, b)?))
}

//! Recorded observations are immutable. Transforms/predictions belong to views.
use crate::{Error, Id, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Point {
    pub x: f64,
    pub y: f64,
    /// Monotonic microseconds relative to the named capture session.
    pub time_us: u64,
    /// Raw device values: null is unavailable, distinct from measured zero.
    pub pressure: Option<f64>,
    pub tilt_rad: Option<f64>,
    pub orientation_rad: Option<f64>,
    pub buttons: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tool {
    Pen,
    Eraser,
    ExplicitFinger,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stroke {
    pub id: Id,
    pub capture_session: Id,
    pub sequence: u64,
    pub pointer_id: u32,
    pub coordinate_space: String,
    pub tool: Tool,
    pub points: Vec<Point>,
}
impl Stroke {
    pub fn validate(&self) -> Result<()> {
        if self.coordinate_space.is_empty()
            || self.coordinate_space.len() > 256
            || self.points.is_empty()
        {
            return Err(Error::Invalid("stroke"));
        }
        if self.points.len() > 100_000 {
            return Err(Error::ResourceLimit);
        }
        let mut previous = 0;
        for p in &self.points {
            if !p.x.is_finite()
                || !p.y.is_finite()
                || p.time_us < previous
                || [p.pressure, p.tilt_rad, p.orientation_rad]
                    .into_iter()
                    .flatten()
                    .any(|v| !v.is_finite())
            {
                return Err(Error::Invalid("stroke samples"));
            }
            previous = p.time_us;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transform {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub tx: f64,
    pub ty: f64,
}
impl Default for Transform {
    fn default() -> Self {
        Self {
            a: 1.,
            b: 0.,
            c: 0.,
            d: 1.,
            tx: 0.,
            ty: 0.,
        }
    }
}
impl Transform {
    pub fn validate(&self) -> Result<()> {
        if [self.a, self.b, self.c, self.d, self.tx, self.ty]
            .iter()
            .any(|v| !v.is_finite())
            || (self.a * self.d - self.b * self.c).abs() < f64::EPSILON
        {
            return Err(Error::Invalid("transform"));
        }
        Ok(())
    }
}

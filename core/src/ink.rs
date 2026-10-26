//! Recorded observations are immutable. Transforms/predictions belong to views.
mod erasure;
mod pressure;
use crate::{Error, Id, Result};
pub use erasure::{EraserGesture, EraserGestureTarget};
pub use pressure::PressureResponse;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Interruption {
    Lifecycle,
    Recovery,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum StrokeEnd {
    #[default]
    Completed,
    InProgress,
    Interrupted(Interruption),
}
impl StrokeEnd {
    pub fn is_completed(&self) -> bool {
        *self == Self::Completed
    }
}

/// Chosen appearance, not inferred sensor data. Width uses the stroke's named
/// coordinate-space units, just like x/y; physical-unit conversion is explicit
/// platform work. None on a legacy stroke means its appearance was not recorded.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrokeStyle {
    pub colour_rgba: [u8; 4],
    pub width: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressure_response: Option<PressureResponse>,
}
impl StrokeStyle {
    pub fn validate(self) -> Result<()> {
        if !self.width.is_finite() || self.width <= 0. || self.width > 1_000_000. {
            return Err(Error::Invalid("stroke style"));
        }
        if let Some(response) = self.pressure_response {
            response.validate()?;
            let maximum = self.width * response.maximum_factor;
            if !maximum.is_finite() || maximum > 1_000_000. {
                return Err(Error::Invalid("pressure width"));
            }
        }
        Ok(())
    }
    pub fn width_for(self, pressure: Option<f64>) -> Result<f64> {
        self.validate()?;
        if pressure.is_some_and(|p| !p.is_finite()) {
            return Err(Error::Invalid("observed pressure"));
        }
        Ok(self.width
            * self
                .pressure_response
                .map_or(Ok(1.), |p| p.factor(pressure))?)
    }
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<StrokeStyle>,
    pub points: Vec<Point>,
    /// Absence is a legacy completed stroke. An interrupted prefix has no
    /// fabricated pen-up sample; InProgress is only valid in a capture journal.
    #[serde(default, skip_serializing_if = "StrokeEnd::is_completed")]
    pub end: StrokeEnd,
}
impl Stroke {
    pub fn validate_persisted(&self) -> Result<()> {
        self.validate()?;
        if self.end == StrokeEnd::InProgress {
            return Err(Error::Invalid("unfinished capture"));
        }
        Ok(())
    }
    pub fn validate(&self) -> Result<()> {
        if let Some(style) = self.style {
            style.validate()?;
        }
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

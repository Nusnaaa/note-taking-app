//! Explicit chosen pressure response. Raw observations are never normalised or
//! replaced; clamping/interpolation affect derived width only.
use crate::{Error, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PressureResponse {
    pub input_min: f64,
    pub input_max: f64,
    pub minimum_factor: f64,
    pub maximum_factor: f64,
    pub exponent: f64,
}
impl PressureResponse {
    pub fn validate(self) -> Result<()> {
        let range = self.input_max - self.input_min;
        if [
            self.input_min,
            self.input_max,
            self.minimum_factor,
            self.maximum_factor,
            self.exponent,
        ]
        .iter()
        .any(|v| !v.is_finite())
            || !range.is_finite()
            || range <= 0.
            || self.minimum_factor < 0.
            || self.maximum_factor <= 0.
            || self.minimum_factor > self.maximum_factor
            || self.maximum_factor > 1024.
            || self.exponent <= 0.
            || self.exponent > 1024.
        {
            return Err(Error::Invalid("pressure response"));
        }
        Ok(())
    }
    /// Unavailable pressure uses the recorded base width. Measured zero is an
    /// ordinary observed value and follows the explicitly selected response.
    pub fn factor(self, pressure: Option<f64>) -> Result<f64> {
        self.validate()?;
        let Some(pressure) = pressure else {
            return Ok(1.);
        };
        if !pressure.is_finite() {
            return Err(Error::Invalid("observed pressure"));
        }
        // Branch before subtracting to avoid overflow for extreme raw values.
        let unit = if pressure <= self.input_min {
            0.
        } else if pressure >= self.input_max {
            1.
        } else {
            (pressure - self.input_min) / (self.input_max - self.input_min)
        };
        Ok(self.minimum_factor
            + (self.maximum_factor - self.minimum_factor) * unit.powf(self.exponent))
    }
}

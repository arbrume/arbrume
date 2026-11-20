//! Water from Python: the `water` keyword and the `water` key of surroundings.
//!
//! `water` is the height of the water surface above the base of the plant (a number),
//! or a dict with the keys of the `[water]` table of a surroundings file: `surface`,
//! `current` (`[x, y, z]` in m/s), `attenuation` (per meter) and `density` (kg/m³)
//! (`docs/underwater.md`). It replaces the water of the `surroundings` keyword. Only
//! aquatic species read it.

use std::sync::Arc;

use arbrume_gen::{Surroundings, Water};

use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;

use crate::convert::py_to_json;

/// The water of `value`: a number (the height of the surface) or a dict.
pub fn parse(value: &Bound<'_, PyAny>) -> PyResult<Water> {
    let water = if let Ok(surface) = value.extract::<f64>() {
        Water::still(surface as f32)
    } else if value.cast::<PyDict>().is_ok() {
        serde_json::from_value(py_to_json(value)?)
            .map_err(|error| PyValueError::new_err(format!("water: {error}")))?
    } else {
        return Err(PyTypeError::new_err(
            "`water` must be a number (the height of the surface) or a dict",
        ));
    };
    water.validate().map_err(PyValueError::new_err)?;
    Ok(water)
}

/// `surroundings` with `water`.
pub fn merge(surroundings: Option<&Surroundings>, water: Water) -> Arc<Surroundings> {
    let mut merged = surroundings.cloned().unwrap_or_default();
    merged.water = Some(water);
    Arc::new(merged)
}

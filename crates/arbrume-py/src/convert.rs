//! Conversions between Python objects, JSON values and TOML tables, and the mapping of
//! Rust errors to Python exceptions.
//!
//! Nested values cross the boundary as JSON text through the standard `json` module:
//! `json.loads` builds plain dicts and lists (and keeps 64-bit seeds exact), and
//! `json.dumps` turns override values (numbers, strings, booleans, lists, dicts) into
//! JSON that deserializes into a TOML table.

use arbrume_export::GlbError;
use arbrume_gen::GenError;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyDict;

/// The Python object (dict, list, number, ...) equal to the JSON `value`.
pub fn json_to_py<'py>(py: Python<'py>, value: &serde_json::Value) -> PyResult<Bound<'py, PyAny>> {
    let text = serde_json::to_string(value).map_err(|e| PyValueError::new_err(e.to_string()))?;
    py.import("json")?.call_method1("loads", (text,))
}

/// The JSON value equal to `object`, which must be JSON-serializable.
pub fn py_to_json(object: &Bound<'_, PyAny>) -> PyResult<serde_json::Value> {
    let text: String = object
        .py()
        .import("json")?
        .call_method1("dumps", (object,))?
        .extract()?;
    serde_json::from_str(&text).map_err(|e| PyValueError::new_err(e.to_string()))
}

/// The TOML table equal to the dict `overrides`, keys kept as given (dotted or not).
pub fn dict_to_toml(overrides: &Bound<'_, PyDict>) -> PyResult<toml::Table> {
    let value = py_to_json(overrides.as_any())?;
    serde_json::from_value(value)
        .map_err(|e| PyValueError::new_err(format!("invalid species override: {e}")))
}

/// `ValueError` with the message of a generation error.
pub fn gen_error(error: &GenError) -> PyErr {
    PyValueError::new_err(error.to_string())
}

/// `OSError` for an i/o failure of the glb writer, `ValueError` otherwise.
pub fn glb_error(error: GlbError) -> PyErr {
    match error {
        GlbError::Io(io) => io.into(),
        other => PyValueError::new_err(other.to_string()),
    }
}

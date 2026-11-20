//! The `guides` keyword of `generate`, `generate_many` and `growth_frames`, and the report
//! of what the guides did (`Tree.guides`).
//!
//! The keyword takes a dict with the keys of a guides JSON file of the command line
//! (`strokes`, `freezes`; see `docs/brush.md`), the JSON text of such a file, or its path
//! (a `str` ending in `.json` or an `os.PathLike`). `None` or an empty dict leaves the
//! plant unguided.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use arbrume_gen::Guides;
use pyo3::exceptions::{PyFileNotFoundError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyString};

use crate::convert::py_to_json;

/// The guides of the `guides` keyword (see the module documentation); `None` for `None`.
pub fn parse(value: &Bound<'_, PyAny>) -> PyResult<Option<Arc<Guides>>> {
    if value.is_none() {
        return Ok(None);
    }
    let guides: Guides = if value.is_instance_of::<PyDict>() {
        let json = py_to_json(value)?;
        serde_json::from_value(json).map_err(|e| invalid(&e))?
    } else if let Ok(text) = value.cast::<PyString>() {
        let text = text.to_cow()?;
        if text.trim_start().starts_with('{') {
            serde_json::from_str(&text).map_err(|e| invalid(&e))?
        } else {
            read(Path::new(text.as_ref()))?
        }
    } else if let Ok(path) = value.extract::<PathBuf>() {
        read(&path)?
    } else {
        return Err(PyTypeError::new_err(
            "guides must be a dict, JSON text or the path of a JSON file",
        ));
    };
    guides
        .validate()
        .map_err(|reason| PyValueError::new_err(format!("invalid guides: {reason}")))?;
    Ok(Some(Arc::new(guides)))
}

/// The guides of the JSON file at `path`.
fn read(path: &Path) -> PyResult<Guides> {
    let text = std::fs::read_to_string(path).map_err(|error| {
        let message = format!("guides file {}: {error}", path.display());
        match error.kind() {
            std::io::ErrorKind::NotFound => PyFileNotFoundError::new_err(message),
            _ => PyErr::from(error),
        }
    })?;
    serde_json::from_str(&text).map_err(|e| invalid(&e))
}

/// `ValueError` for guides that do not parse.
fn invalid(error: &serde_json::Error) -> PyErr {
    PyValueError::new_err(format!("invalid guides: {error}"))
}

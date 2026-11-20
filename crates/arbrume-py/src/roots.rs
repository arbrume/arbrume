//! The `roots` and `soil` keywords of `generate` and `generate_many`.
//!
//! `roots` reaches the species as an override and chooses its root system
//! (`docs/roots.md`): `False` removes the `[roots]` layer and the overrides of its
//! fields, `True` keeps it or adds the default one (the heart habit), a habit name
//! (`"tap"`, `"heart"`, `"plate"`, `"fibrous"`) replaces it with the values of that
//! habit, and a dict is merged into it as the table of any other section is (it creates
//! the layer when the species has none, from the values of its `habit` key or of the
//! heart habit). Dotted overrides such as `**{"roots.exposed": 0.5}` change one field of
//! the layer.
//!
//! `soil` is the ground and the resource patches the roots grow in: a dict with the keys
//! of a `--soil` file of the command line (`ground`, `resources`), or the path of such a
//! TOML file. Its obstacles are the blocking occluders of `surroundings`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use arbrume_gen::Soil;
use pyo3::exceptions::{PyFileNotFoundError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyString};
use toml::{Table, Value};

use crate::convert::py_to_json;

/// The species text `text` with the `roots` override of `overrides` applied (see the
/// module documentation), and the other overrides. Without that override both are
/// returned as they are.
pub fn resolve(text: &str, overrides: &Table) -> PyResult<(String, Table)> {
    let Some(choice) = overrides.get("roots") else {
        return Ok((text.to_owned(), overrides.clone()));
    };
    let mut species: Table =
        toml::from_str(text).map_err(|error| PyValueError::new_err(format!("species: {error}")))?;
    match choice {
        Value::Boolean(false) => {
            species.remove("roots");
        }
        Value::Boolean(true) => {
            species
                .entry("roots")
                .or_insert_with(|| Value::Table(Table::new()));
        }
        Value::String(habit) => {
            let mut table = Table::new();
            table.insert("habit".to_owned(), Value::String(habit.clone()));
            species.insert("roots".to_owned(), Value::Table(table));
        }
        Value::Table(_) => return Ok((text.to_owned(), overrides.clone())),
        other => {
            return Err(PyTypeError::new_err(format!(
                "`roots` must be a bool, a habit name or a dict, not a {}",
                other.type_str()
            )));
        }
    }
    let mut rest = overrides.clone();
    rest.remove("roots");
    if matches!(choice, Value::Boolean(false)) {
        rest.retain(|key, _| !key.starts_with("roots."));
    }
    Ok((species.to_string(), rest))
}

/// The soil of the `soil` keyword (see the module documentation); `None` for `None`.
pub fn parse_soil(value: &Bound<'_, PyAny>) -> PyResult<Option<Arc<Soil>>> {
    if value.is_none() {
        return Ok(None);
    }
    let soil: Soil = if value.cast::<PyDict>().is_ok() {
        serde_json::from_value(py_to_json(value)?)
            .map_err(|error| PyValueError::new_err(format!("soil: {error}")))?
    } else if value.is_instance_of::<PyString>() || value.hasattr("__fspath__")? {
        from_file(&value.extract::<PathBuf>()?)?
    } else {
        return Err(PyTypeError::new_err(
            "`soil` must be a dict or the path of a TOML file",
        ));
    };
    soil.validate()
        .map_err(|reason| PyValueError::new_err(format!("soil: {reason}")))?;
    Ok(Some(Arc::new(soil)))
}

/// The soil of a TOML file.
fn from_file(path: &Path) -> PyResult<Soil> {
    let text = std::fs::read_to_string(path).map_err(|error| {
        let message = format!("soil file {}: {error}", path.display());
        match error.kind() {
            std::io::ErrorKind::NotFound => PyFileNotFoundError::new_err(message),
            _ => PyErr::from(error),
        }
    })?;
    toml::from_str(&text)
        .map_err(|error| PyValueError::new_err(format!("{}: {error}", path.display())))
}

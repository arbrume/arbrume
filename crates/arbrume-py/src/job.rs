//! Turning the Python arguments of `generate` and `generate_many` into species and
//! generation parameters.
//!
//! A species argument is, in this order: a preset name, a path to an existing species
//! file (a `str` or an `os.PathLike`), or the TOML text of a species. Text that is not a
//! preset nor a file and looks like neither TOML (no `=`, no `[`, no newline) is reported
//! as an unknown preset or a missing file.
//!
//! The keyword options are `scale`, `quality` and `overrides` (a dict of dotted paths);
//! every other keyword is one more override, so `**{"foliage.length": 0.2}` and
//! `overrides={"foliage.length": 0.2}` are the same.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use arbrume_core::species::Species;
use arbrume_gen::{GenParams, preset_toml, presets, species_with_overrides};
use pyo3::exceptions::{PyFileNotFoundError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyString};

use crate::convert::{dict_to_toml, gen_error};

/// Keyword options shared by `generate` and `generate_many`.
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    /// Scale factor.
    pub scale: f32,
    /// Mesh quality.
    pub quality: f32,
    /// Species overrides by dotted path.
    pub overrides: toml::Table,
}

impl Options {
    /// Reads `kwargs`: `scale`, `quality`, `overrides` and dotted overrides.
    pub fn parse(kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
        let mut options = Self {
            scale: 1.0,
            quality: 1.0,
            overrides: toml::Table::new(),
        };
        let Some(kwargs) = kwargs else {
            return Ok(options);
        };
        let extra = PyDict::new(kwargs.py());
        if let Some(explicit) = kwargs.get_item("overrides")?
            && !explicit.is_none()
        {
            extra.update(explicit.cast::<PyDict>()?.as_mapping())?;
        }
        for (key, value) in kwargs.iter() {
            match key.extract::<String>()?.as_str() {
                "scale" => options.scale = number(&value, "scale")?,
                "quality" => options.quality = number(&value, "quality")?,
                "overrides" => {}
                _ => extra.set_item(key, value)?,
            }
        }
        if !(options.quality > 0.0 && options.quality.is_finite()) {
            let reason = format!("quality {} is not positive and finite", options.quality);
            return Err(PyValueError::new_err(reason));
        }
        options.overrides = dict_to_toml(&extra)?;
        Ok(options)
    }
}

/// `value` as `f32`; `TypeError` naming `name` when it is not a number.
fn number(value: &Bound<'_, PyAny>, name: &str) -> PyResult<f32> {
    value
        .extract::<f64>()
        .map(|number| number as f32)
        .map_err(|_| PyTypeError::new_err(format!("`{name}` must be a number")))
}

/// `seed` checked to fit a `u64`.
pub fn seed(seed: i128) -> PyResult<u64> {
    u64::try_from(seed)
        .map_err(|_| PyValueError::new_err(format!("seed {seed} is outside [0, 2**64)")))
}

/// `age` checked to fit a `u16`; `None` gives the default age.
pub fn age(age: Option<i64>) -> PyResult<u16> {
    match age {
        None => Ok(GenParams::default().age),
        Some(age) => u16::try_from(age)
            .map_err(|_| PyValueError::new_err(format!("age {age} is outside [0, 65535]"))),
    }
}

/// Generation parameters of one plant, as the command line builds them: the plant and
/// mesh seeded with `seed`, then scale and quality applied.
pub fn params(seed: u64, age: u16, options: &Options) -> GenParams {
    GenParams::new(seed, age)
        .with_scale(options.scale)
        .with_quality(options.quality)
}

/// The TOML text of a species argument (see the module documentation).
pub fn species_text(species: &Bound<'_, PyAny>) -> PyResult<String> {
    if let Ok(text) = species.cast::<PyString>() {
        let text = text.to_cow()?;
        return text_of_str(&text);
    }
    match species.extract::<PathBuf>() {
        Ok(path) => read(&path),
        Err(_) => Err(PyTypeError::new_err(
            "species must be a preset name, a path or TOML text",
        )),
    }
}

/// [`species_text`] of a `str`.
fn text_of_str(text: &str) -> PyResult<String> {
    if let Some(preset) = preset_toml(text) {
        return Ok(preset.to_owned());
    }
    let path = Path::new(text);
    if !text.contains('\n') && path.is_file() {
        return read(path);
    }
    if text.contains(['=', '[', '\n']) {
        return Ok(text.to_owned());
    }
    let names: Vec<&str> = presets().iter().map(|(name, _)| *name).collect();
    let message = format!(
        "species `{text}` is neither a preset ({}), an existing file nor TOML text",
        names.join(", ")
    );
    let is_toml_file = path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("toml"));
    Err(if is_toml_file || text.contains(['/', '\\']) {
        PyFileNotFoundError::new_err(message)
    } else {
        PyValueError::new_err(message)
    })
}

/// The contents of the species file `path`.
fn read(path: &Path) -> PyResult<String> {
    std::fs::read_to_string(path).map_err(|error| {
        let message = format!("species file {}: {error}", path.display());
        match error.kind() {
            std::io::ErrorKind::NotFound => PyFileNotFoundError::new_err(message),
            _ => PyErr::from(error),
        }
    })
}

/// Parsed species, keyed by their text and overrides, so that a batch parses each
/// distinct species once.
#[derive(Debug, Default)]
pub struct SpeciesCache {
    parsed: HashMap<(String, String), Arc<Species>>,
}

impl SpeciesCache {
    /// The species of `text` with `overrides` applied.
    pub fn get(&mut self, text: String, overrides: &toml::Table) -> PyResult<Arc<Species>> {
        let key = (text, overrides.to_string());
        if let Some(species) = self.parsed.get(&key) {
            return Ok(Arc::clone(species));
        }
        let species = species_with_overrides(&key.0, overrides).map_err(|e| gen_error(&e))?;
        let species = Arc::new(species);
        self.parsed.insert(key, Arc::clone(&species));
        Ok(species)
    }
}

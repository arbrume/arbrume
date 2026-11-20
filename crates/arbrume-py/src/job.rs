//! Turning the Python arguments of `generate` and `generate_many` into species and
//! generation parameters.
//!
//! A species argument is, in this order: a preset name, a path to an existing species
//! file (a `str` or an `os.PathLike`), a species `pack_id/name` of a loaded or
//! installed pack (see `packs`), or the TOML text of a species. Texture paths of a
//! species file are relative to its directory, those of a pack species to the pack. Text that is not a
//! preset nor a file and looks like neither TOML (no `=`, no `[`, no newline) is reported
//! as an unknown preset or a missing file.
//!
//! The keyword options are `scale`, `quality`, `textures` (a bool: procedural textures,
//! see `docs/textures.md`), `junctions` (`"tier-a"` or `"tier-b"`, see
//! `docs/junctions.md`), `surroundings` (see [`crate::surroundings`]), `guides` (see
//! [`crate::guides`]), `roots` and `soil` (see [`crate::roots`]) and `overrides`
//! (a dict of dotted paths); every other keyword is one more override, so
//! `**{"foliage.length": 0.2}` and `overrides={"foliage.length": 0.2}` are the same.
//!
//! Textures come from one cache for the whole process, so that regenerating a plant of
//! the same species does not synthesize them again; it is emptied when it holds more
//! than [`MAX_TEXTURED_SPECIES`] species, which bounds its memory while species are
//! edited.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use arbrume_core::species::Species;
use arbrume_gen::{
    GenParams, Guides, Junctions, Surroundings, TextureCache, TextureOptions, preset_toml, presets,
    species_with_overrides,
};
use pyo3::exceptions::{PyFileNotFoundError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyString};

use crate::convert::{dict_to_toml, gen_error};

/// Keyword options shared by `generate`, `generate_many` and `growth_frames`.
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    /// Scale factor.
    pub scale: f32,
    /// Mesh quality.
    pub quality: f32,
    /// Species overrides by dotted path.
    pub overrides: toml::Table,
    /// Whether the plants carry procedural textures.
    pub textures: bool,
    /// How branches are joined.
    pub junctions: Junctions,
    /// What stands around the plant.
    pub surroundings: Option<Arc<Surroundings>>,
    /// Strokes and freezes that steer the growth.
    pub guides: Option<Arc<Guides>>,
    /// Ground and resources the roots grow in.
    pub soil: Option<Arc<arbrume_gen::Soil>>,
}

/// Largest number of species in the texture cache before it is emptied.
pub const MAX_TEXTURED_SPECIES: usize = 32;

/// The texture cache of the process.
pub fn texture_cache() -> Arc<TextureCache> {
    static CACHE: Mutex<Option<Arc<TextureCache>>> = Mutex::new(None);
    let mut slot = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
    match slot.as_ref() {
        Some(cache) if cache.len() <= MAX_TEXTURED_SPECIES => Arc::clone(cache),
        _ => {
            let cache = Arc::new(TextureCache::new(TextureOptions::default()));
            *slot = Some(Arc::clone(&cache));
            cache
        }
    }
}

impl Options {
    /// Reads `kwargs`: `scale`, `quality`, `textures`, `junctions`, `surroundings`,
    /// `guides`, `overrides` and dotted overrides.
    pub fn parse(kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
        let mut options = Self {
            scale: 1.0,
            quality: 1.0,
            overrides: toml::Table::new(),
            textures: false,
            junctions: Junctions::TierA,
            surroundings: None,
            guides: None,
            soil: None,
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
                "junctions" => options.junctions = junctions(&value)?,
                "surroundings" => options.surroundings = crate::surroundings::parse(&value)?,
                "guides" => options.guides = crate::guides::parse(&value)?,
                "soil" => options.soil = crate::roots::parse_soil(&value)?,
                "roots" if value.is_none() => {}
                "textures" => {
                    options.textures = value.extract::<bool>().map_err(|_| {
                        PyTypeError::new_err("`textures` must be a bool".to_owned())
                    })?;
                }
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

/// `value` as a junction tier: `"tier-a"` or `"tier-b"`.
fn junctions(value: &Bound<'_, PyAny>) -> PyResult<Junctions> {
    let text: String = value
        .extract()
        .map_err(|_| PyTypeError::new_err("`junctions` must be a str".to_owned()))?;
    match text.as_str() {
        "tier-a" => Ok(Junctions::TierA),
        "tier-b" => Ok(Junctions::TierB),
        other => Err(PyValueError::new_err(format!(
            "junctions `{other}` is not \"tier-a\" or \"tier-b\""
        ))),
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
    let mut params = GenParams::new(seed, age)
        .with_scale(options.scale)
        .with_quality(options.quality);
    params.mesh.junctions = options.junctions;
    params.surroundings.clone_from(&options.surroundings);
    params.guides.clone_from(&options.guides);
    params.soil.clone_from(&options.soil);
    params
}

/// A species argument resolved: its TOML text and the directory its texture paths
/// are relative to.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SpeciesArg {
    /// TOML text of the species.
    pub text: String,
    /// Directory of the species file or pack, if any.
    pub asset_dir: Option<PathBuf>,
}

impl SpeciesArg {
    /// A species given by its text alone.
    fn text(text: String) -> Self {
        Self {
            text,
            asset_dir: None,
        }
    }
}

/// The TOML text of a species argument (see the module documentation).
pub fn species_text(species: &Bound<'_, PyAny>) -> PyResult<SpeciesArg> {
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
fn text_of_str(text: &str) -> PyResult<SpeciesArg> {
    if let Some(preset) = preset_toml(text) {
        return Ok(SpeciesArg::text(preset.to_owned()));
    }
    let path = Path::new(text);
    if !text.contains('\n') && path.is_file() {
        return read(path);
    }
    if let Some(species) = crate::packs::resolve(text)? {
        return Ok(SpeciesArg {
            text: species.text,
            asset_dir: species.asset_dir,
        });
    }
    if text.contains(['=', '[', '\n']) {
        return Ok(SpeciesArg::text(text.to_owned()));
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

/// The contents of the species file `path`, its texture paths relative to its
/// directory.
fn read(path: &Path) -> PyResult<SpeciesArg> {
    let text = std::fs::read_to_string(path).map_err(|error| {
        let message = format!("species file {}: {error}", path.display());
        match error.kind() {
            std::io::ErrorKind::NotFound => PyFileNotFoundError::new_err(message),
            _ => PyErr::from(error),
        }
    })?;
    Ok(SpeciesArg {
        text,
        asset_dir: path.parent().map(Path::to_path_buf),
    })
}

/// Parsed species, keyed by their text, asset directory and overrides, so that a batch
/// parses each distinct species once.
#[derive(Debug, Default)]
pub struct SpeciesCache {
    parsed: HashMap<(SpeciesArg, String), Arc<Species>>,
}

impl SpeciesCache {
    /// The species of `arg` with `overrides` applied.
    pub fn get(&mut self, arg: SpeciesArg, overrides: &toml::Table) -> PyResult<Arc<Species>> {
        let key = (arg, overrides.to_string());
        if let Some(species) = self.parsed.get(&key) {
            return Ok(Arc::clone(species));
        }
        let (text, overrides) = crate::roots::resolve(&key.0.text, overrides)?;
        let mut species = species_with_overrides(&text, &overrides).map_err(|e| gen_error(&e))?;
        species.asset_dir.clone_from(&key.0.asset_dir);
        let species = Arc::new(species);
        self.parsed.insert(key, Arc::clone(&species));
        Ok(species)
    }
}

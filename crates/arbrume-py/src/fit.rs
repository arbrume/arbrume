//! `fit` and `distance`: parameter fitting to target skeletons ([`arbrume_fit`]).
//!
//! A skeleton argument is a `Tree`, a path to an `.ann.npz`, `.glb`, `.csv` or `.json`
//! skeleton, or a dict of `position` (n by 3), `radius` (n) and `parent` (n, `-1` for a
//! root) sequences or arrays. Both functions run without the GIL.

use std::path::PathBuf;

use arbrume_fit::{
    Checkpoint, DistanceOptions, FitError, FitOptions, TreeSkeleton, Up, distance as fit_distance,
    fit_with_checkpoint,
};
use pyo3::exceptions::{PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyString};

use crate::tree::Tree;

/// The up axis named `up`.
fn up_axis(up: &str) -> PyResult<Up> {
    match up {
        "y" | "Y" => Ok(Up::Y),
        "z" | "Z" => Ok(Up::Z),
        other => Err(PyValueError::new_err(format!(
            "up `{other}` is not `y` or `z`"
        ))),
    }
}

/// A skeleton argument (see the module documentation).
fn skeleton(value: &Bound<'_, PyAny>, up: Up) -> PyResult<TreeSkeleton> {
    if let Ok(tree) = value.cast::<Tree>() {
        return Ok(TreeSkeleton::from_graph(&tree.get().data().plant.graph));
    }
    if let Ok(dict) = value.cast::<PyDict>() {
        let field = |key: &str| {
            dict.get_item(key)?
                .ok_or_else(|| PyValueError::new_err(format!("skeleton dict lacks `{key}`")))
        };
        let skeleton = TreeSkeleton {
            position: field("position")?.extract()?,
            radius: field("radius")?.extract()?,
            parent: field("parent")?.extract()?,
        }
        .with_up(up);
        skeleton.validate().map_err(|e| value_error(&e))?;
        return Ok(skeleton);
    }
    let path: PathBuf = value.extract().map_err(|_| {
        PyTypeError::new_err("a skeleton is a Tree, a path or a dict of position, radius, parent")
    })?;
    TreeSkeleton::read(&path, up).map_err(|e| value_error(&e))
}

fn value_error(error: &FitError) -> PyErr {
    PyValueError::new_err(error.to_string())
}

/// The TOML text and label of a base species: a preset name, a path or TOML text.
fn base_species(species: &Bound<'_, PyAny>) -> PyResult<(String, String)> {
    if let Ok(text) = species.cast::<PyString>() {
        let text = text.to_cow()?;
        if let Some(preset) = arbrume_gen::preset_toml(&text) {
            return Ok((preset.to_owned(), text.into_owned()));
        }
        if text.contains(['=', '[', '\n']) {
            return Ok((text.into_owned(), "species".to_owned()));
        }
    }
    let path: PathBuf = species
        .extract()
        .map_err(|_| PyTypeError::new_err("species must be a preset name, a path or TOML text"))?;
    let text = std::fs::read_to_string(&path)
        .map_err(|e| PyValueError::new_err(format!("{}: {e}", path.display())))?;
    let label = path.file_stem().map_or_else(
        || "species".to_owned(),
        |s| s.to_string_lossy().into_owned(),
    );
    Ok((text, label))
}

/// A JSON value as a Python object.
fn to_python<'py>(py: Python<'py>, value: &serde_json::Value) -> PyResult<Bound<'py, PyAny>> {
    let text = serde_json::to_string(value).map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
    py.import("json")?.call_method1("loads", (text,))
}

/// Distance between two tree skeletons, as a dict of the terms (`envelope`, `density`,
/// `geometry`, `counts`, `features`) and their weighted `total`, each in `[0, 1]`.
///
/// `min_radius` prunes nodes thinner than it (meters) with their subtrees, `step` is
/// the resampling step (default: the taller height over 100), `smooth` the number of
/// smoothing passes, `up` the up axis (`"y"` or `"z"`) of CSV, JSON and dict skeletons.
#[pyfunction]
#[pyo3(signature = (a, b, *, min_radius=0.0, step=None, smooth=0, up="y"))]
pub fn distance<'py>(
    py: Python<'py>,
    a: &Bound<'py, PyAny>,
    b: &Bound<'py, PyAny>,
    min_radius: f32,
    step: Option<f32>,
    smooth: u32,
    up: &str,
) -> PyResult<Bound<'py, PyDict>> {
    let up = up_axis(up)?;
    let (a, b) = (skeleton(a, up)?, skeleton(b, up)?);
    let options = DistanceOptions {
        min_radius,
        step,
        smooth,
        ..DistanceOptions::default()
    };
    let terms = py
        .detach(|| fit_distance(&a, &b, &options))
        .map_err(|e| value_error(&e))?;
    let out = PyDict::new(py);
    for (name, value) in arbrume_fit::Terms::NAMES.iter().zip(terms.values()) {
        out.set_item(name, value)?;
    }
    Ok(out)
}

/// Keyword settings of `fit`.
const FIT_KEYS: [&str; 13] = [
    "params",
    "seeds",
    "budget",
    "seed",
    "holdout",
    "max_factor",
    "min_radius",
    "step",
    "smooth",
    "up",
    "checkpoint",
    "resume",
    "threads",
];

/// The keyword settings of `fit`: options, up axis, checkpoint, resume and threads.
#[derive(Debug, Clone)]
struct FitSettings {
    options: FitOptions,
    up: Up,
    checkpoint: Option<PathBuf>,
    resume: bool,
    threads: Option<usize>,
}

impl FitSettings {
    /// Parses the keywords of `fit`.
    fn parse(kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
        let mut settings = Self {
            options: FitOptions::default(),
            up: Up::Y,
            checkpoint: None,
            resume: false,
            threads: None,
        };
        let Some(kwargs) = kwargs else {
            return Ok(settings);
        };
        for (key, value) in kwargs.iter() {
            let key: String = key.extract()?;
            if !FIT_KEYS.contains(&key.as_str()) {
                return Err(PyTypeError::new_err(format!(
                    "fit() got an unknown keyword `{key}`"
                )));
            }
            settings.set(&key, &value)?;
        }
        Ok(settings)
    }

    /// Sets the keyword `key`.
    fn set(&mut self, key: &str, value: &Bound<'_, PyAny>) -> PyResult<()> {
        let options = &mut self.options;
        match key {
            "params" => options.parameters = value.extract()?,
            "seeds" => options.seeds = value.extract()?,
            "budget" => options.budget = value.extract()?,
            "seed" => options.seed = value.extract()?,
            "holdout" => options.holdout = value.extract()?,
            "max_factor" => options.max_factor = value.extract()?,
            "min_radius" => options.distance.min_radius = value.extract()?,
            "step" => options.distance.step = value.extract()?,
            "smooth" => options.distance.smooth = value.extract()?,
            "up" => self.up = up_axis(&value.extract::<String>()?)?,
            "checkpoint" => self.checkpoint = value.extract()?,
            "resume" => self.resume = value.extract()?,
            "threads" => self.threads = value.extract()?,
            _ => {}
        }
        Ok(())
    }
}

/// Reads the checkpoint at `path` when it exists.
fn load_checkpoint(path: Option<&PathBuf>) -> PyResult<Option<Checkpoint>> {
    let Some(path) = path.filter(|p| p.exists()) else {
        return Ok(None);
    };
    let text = std::fs::read_to_string(path)
        .map_err(|e| PyValueError::new_err(format!("{}: {e}", path.display())))?;
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|e| PyValueError::new_err(format!("{}: {e}", path.display())))
}

/// Fits the parameters of a species to a target skeleton.
///
/// `target` is a skeleton (a `Tree`, a path or a dict, see `distance`), `species` the
/// base species (a preset name, a species file or TOML text). Keyword settings, with the
/// defaults of `arbrume fit`: `params` (list of `"age"`, `"scale"` and jitterable names;
/// default: those that change the skeleton, plus age and scale), `seeds` (4), `budget`
/// (1200 candidates), `seed` (0), `holdout` (16), `max_factor` (3.0), `min_radius`
/// (0.0), `step` (None), `smooth` (0), `up` (`"y"`), `checkpoint` (a path written after
/// every generation), `resume` (continue from `checkpoint` when it exists) and
/// `threads` (default: the global pool). Returns a dict: `toml` (the fitted species
/// file), `age`, `scale`, `factors` and `report`. The result does not depend on the
/// number of threads.
#[pyfunction]
#[pyo3(signature = (target, species, **options))]
pub fn fit<'py>(
    py: Python<'py>,
    target: &Bound<'py, PyAny>,
    species: &Bound<'py, PyAny>,
    options: Option<&Bound<'py, PyDict>>,
) -> PyResult<Bound<'py, PyDict>> {
    let settings = FitSettings::parse(options)?;
    let target = skeleton(target, settings.up)?;
    let (text, label) = base_species(species)?;
    let resume = if settings.resume {
        load_checkpoint(settings.checkpoint.as_ref())?
    } else {
        None
    };
    let path = settings.checkpoint.clone();
    let options = settings.options.clone();
    let mut save = |state: &Checkpoint| {
        if let Some(path) = &path {
            let text = serde_json::to_string(state).map_err(|e| FitError::Io(e.to_string()))?;
            std::fs::write(path, text)
                .map_err(|e| FitError::Io(format!("{}: {e}", path.display())))?;
        }
        Ok(())
    };
    let run = || fit_with_checkpoint(&target, &text, &label, &options, resume, &mut save);
    let result = py.detach(|| match settings.threads {
        Some(threads) => rayon::ThreadPoolBuilder::new()
            .num_threads(threads.max(1))
            .build()
            .map_err(|e| FitError::Io(e.to_string()))
            .and_then(|pool| pool.install(run)),
        None => run(),
    });
    let result = result.map_err(|e| value_error(&e))?;
    let out = PyDict::new(py);
    out.set_item("toml", &result.toml)?;
    out.set_item("age", result.fitted.age)?;
    out.set_item("scale", result.fitted.scale)?;
    let factors = PyDict::new(py);
    for (name, factor) in &result.fitted.factors {
        factors.set_item(name, factor)?;
    }
    out.set_item("factors", factors)?;
    out.set_item("report", to_python(py, &result.report)?)?;
    Ok(out)
}

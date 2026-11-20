//! `forest_build`: the forest of a config, placed and grown without the GIL.
//!
//! The Python wrapper `arbrume.forest` (in `python/arbrume/forest.py`) turns the raw
//! result into `NumPy` arrays. The config is a dict with the keys of `forest.toml`, the
//! TOML text of one, or the path of a file (`docs/forest.md`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use arbrume_export::{ExportOptions, WindLayout};
use arbrume_forest::{Forest, ForestConfig, ForestError, write_forest};
use arbrume_gen::{LodKind, Plant};
use pyo3::exceptions::{PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyList};

use crate::convert::py_to_json;
use crate::data::TreeData;
use crate::tree::Tree;

/// The config and the directory its relative paths start from.
fn read_config(
    config: &Bound<'_, PyAny>,
    base: Option<PathBuf>,
) -> PyResult<(ForestConfig, PathBuf)> {
    let fail = |e: ForestError| PyValueError::new_err(e.to_string());
    if let Ok(dict) = config.cast::<PyDict>() {
        let value = py_to_json(dict.as_any())?;
        let parsed = serde_json::from_value(value)
            .map_err(|e| PyValueError::new_err(format!("forest config: {e}")))?;
        return Ok((parsed, base.unwrap_or_else(|| PathBuf::from("."))));
    }
    let path: PathBuf = config
        .extract()
        .map_err(|_| PyTypeError::new_err("config must be a dict, a str or a path"))?;
    let text = path.to_string_lossy();
    if !text.contains('\n') && path.is_file() {
        let parsed = ForestConfig::from_path(&path).map_err(fail)?;
        let dir = path
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        return Ok((parsed, base.unwrap_or(dir)));
    }
    let parsed = ForestConfig::from_toml_str(&text).map_err(fail)?;
    Ok((parsed, base.unwrap_or_else(|| PathBuf::from("."))))
}

/// A pool of `threads` threads, or `None` for the global pool.
fn pool(threads: Option<i64>) -> PyResult<Option<rayon::ThreadPool>> {
    let Some(count) = threads else {
        return Ok(None);
    };
    let count = usize::try_from(count)
        .ok()
        .filter(|count| *count > 0)
        .ok_or_else(|| PyValueError::new_err(format!("threads {count} is not >= 1")))?;
    rayon::ThreadPoolBuilder::new()
        .num_threads(count)
        .build()
        .map(Some)
        .map_err(|e| PyRuntimeError::new_err(e.to_string()))
}

/// A tree object of `plant` with the mesh `mesh`.
fn tree_of(
    species: &Arc<arbrume_core::species::Species>,
    plant: &Plant,
    mesh: &arbrume_core::mesh::TreeMesh,
) -> Tree {
    let mut copy = plant.clone();
    copy.mesh.clone_from(mesh);
    copy.stats.set_mesh(&copy.mesh);
    Tree::new(TreeData::new(Arc::clone(species), copy))
}

/// The variants of every species as lists of dicts.
fn variants<'py>(py: Python<'py>, forest: &Forest) -> PyResult<Bound<'py, PyList>> {
    let out = PyList::empty(py);
    for species in &forest.library {
        let list = PyList::empty(py);
        for variant in &species.variants {
            let individual = Arc::new(variant.individual.clone());
            let dict = PyDict::new(py);
            let levels = PyList::empty(py);
            let (kinds, heights) = (PyList::empty(py), PyList::empty(py));
            for level in &variant.levels {
                levels.append(tree_of(&individual, &variant.plant, &level.mesh))?;
                kinds.append(match level.kind {
                    LodKind::Mesh => "mesh",
                    LodKind::Billboard => "billboard",
                })?;
                heights.append(level.max_screen_height)?;
            }
            dict.set_item("seed", variant.seed)?;
            dict.set_item("crown_radius", variant.crown_radius)?;
            dict.set_item("trunk_radius", variant.trunk_radius)?;
            dict.set_item("height", variant.height)?;
            dict.set_item("levels", levels)?;
            dict.set_item("kinds", kinds)?;
            dict.set_item("max_screen_height", heights)?;
            list.append(dict)?;
        }
        out.append(list)?;
    }
    Ok(out)
}

/// The placement report and the rules as a dict.
fn report<'py>(py: Python<'py>, forest: &Forest) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    let species = PyList::empty(py);
    for (s, counts) in forest.placement.species.iter().enumerate() {
        let entry = PyDict::new(py);
        entry.set_item("name", &forest.specs[s].name)?;
        entry.set_item("kept", counts.kept)?;
        entry.set_item("candidates", counts.candidates)?;
        entry.set_item("expected_open", counts.expected_open)?;
        entry.set_item("saturated_cells", counts.saturated_cells)?;
        entry.set_item("spacing", forest.rules[s].spacing)?;
        species.append(entry)?;
    }
    dict.set_item("species", species)?;
    dict.set_item("cell", forest.placement.cell)?;
    dict.set_item("place_seconds", forest.placement.time.as_secs_f64())?;
    dict.set_item("library_seconds", forest.library_time.as_secs_f64())?;
    Ok(dict)
}

/// The result of [`forest_build`] as a dict of raw values.
fn result<'py>(py: Python<'py>, forest: &Forest) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    let mut table = Vec::new();
    arbrume_forest::table::write_binary(&forest.instances, &mut table)
        .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
    dict.set_item(
        "records",
        PyBytes::new(py, &table[arbrume_forest::table::HEADER_BYTES..]),
    )?;
    dict.set_item("count", forest.instances.len())?;
    let names: Vec<&str> = forest.specs.iter().map(|s| s.name.as_str()).collect();
    dict.set_item("names", names)?;
    dict.set_item("variants", variants(py, forest)?)?;
    let terrain = PyDict::new(py);
    terrain.set_item("origin", forest.terrain.origin())?;
    terrain.set_item("size", forest.terrain.size())?;
    terrain.set_item("samples", forest.terrain.samples())?;
    let heights: Vec<u8> = forest
        .terrain
        .heights()
        .iter()
        .flat_map(|h| h.to_le_bytes())
        .collect();
    terrain.set_item("heights", PyBytes::new(py, &heights))?;
    dict.set_item("terrain", terrain)?;
    dict.set_item("report", report(py, forest)?)?;
    let coupled = PyList::empty(py);
    if let Some(grown) = &forest.coupled {
        for tree in &grown.trees {
            let species = Arc::new(
                forest.specs[forest.instances[tree.instance].species as usize]
                    .species
                    .clone(),
            );
            let item = (
                tree.instance,
                tree_of(&species, &tree.plant, &tree.plant.mesh),
            );
            coupled.append(item)?;
        }
    }
    dict.set_item("coupled", coupled)?;
    Ok(dict)
}

/// Places (and with `grow`, grows) the forest of `config` without the GIL; writes its
/// files into `out` when given. Returns raw records and lists; use `arbrume.forest`.
#[pyfunction]
#[pyo3(signature = (config, *, grow=true, base=None, out=None, threads=None))]
pub fn forest_build<'py>(
    py: Python<'py>,
    config: &Bound<'py, PyAny>,
    grow: bool,
    base: Option<PathBuf>,
    out: Option<PathBuf>,
    threads: Option<i64>,
) -> PyResult<Bound<'py, PyDict>> {
    let (config, base) = read_config(config, base)?;
    let pool = pool(threads)?;
    let run = move || -> Result<Forest, ForestError> {
        let forest = Forest::build(&config, &base, grow)?;
        if let Some(dir) = &out {
            let export = ExportOptions {
                wind: if config.library.wind {
                    WindLayout::TexCoord
                } else {
                    WindLayout::None
                },
                ..ExportOptions::default()
            };
            write_forest(&forest, &config.output, &export, dir)?;
        }
        Ok(forest)
    };
    let forest = py
        .detach(|| match &pool {
            Some(pool) => pool.install(run),
            None => run(),
        })
        .map_err(|e| match e {
            ForestError::Io(_) | ForestError::Glb(_) => PyRuntimeError::new_err(e.to_string()),
            other => PyValueError::new_err(other.to_string()),
        })?;
    result(py, &forest)
}

//! `generate_stand`: several trees grown together, so that they shade one another.
//!
//! Each plant is a dict with the keys of a job of `generate_many` plus `position`, three
//! floats in meters (Y up). The plants are grown in one environment with
//! `arbrume_gen::Generator::generate_group`, then meshed one by one, without the GIL.
//! The keyword options are defaults for every plant; `surroundings` are those of the
//! whole stand, in its frame, and every plant must give the same. The result does not
//! depend on the order of the plants nor on the number of threads.

use arbrume_gen::{Generator, GroupEntry};
use pyo3::exceptions::{PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use crate::convert::gen_error;
use crate::data::TreeData;
use crate::job;
use crate::tree::Tree;

/// The plant dicts of `plants` without their `position`, and the positions.
fn split_positions<'py>(
    plants: &Bound<'py, PyAny>,
) -> PyResult<(Bound<'py, PyList>, Vec<[f32; 3]>)> {
    let py = plants.py();
    let stripped = PyList::empty(py);
    let mut positions = Vec::new();
    for (index, item) in plants.try_iter()?.enumerate() {
        let item = item?;
        let plant = item
            .cast::<PyDict>()
            .map_err(|_| PyTypeError::new_err(format!("plant {index} is not a dict")))?;
        let plant = plant.copy()?;
        let position = plant
            .get_item("position")?
            .ok_or_else(|| PyValueError::new_err(format!("plant {index} has no `position`")))?;
        let position: Vec<f32> = position.extract()?;
        let position: [f32; 3] = position.try_into().map_err(|_| {
            PyValueError::new_err(format!("the position of plant {index} is not 3 numbers"))
        })?;
        plant.del_item("position")?;
        stripped.append(plant)?;
        positions.push(position);
    }
    Ok((stripped, positions))
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

/// Generates a stand of trees grown together, without the GIL.
///
/// `plants` is a list of dicts, each with `species` and `position` (three floats in
/// meters, Y up) and the other keys of a job of `generate_many` (`seed`, `age`,
/// `scale`, `quality`, `textures`, `junctions`, `overrides`, dotted overrides). Keyword
/// options are defaults for every plant; `surroundings` are those of the stand, in its
/// frame. `textures` on any plant textures every plant. `threads` sets the number of
/// worker threads (default: the global `rayon` pool). Returns one tree per plant, in
/// the order of `plants`, each in its own frame with its base at the origin: tree `i`
/// stands at `plants[i]["position"]`. Raises `ValueError` for an invalid species,
/// parameter or position, or plants with different surroundings.
#[pyfunction]
#[pyo3(signature = (plants, age=None, *, threads=None, **options))]
pub fn generate_stand(
    py: Python<'_>,
    plants: &Bound<'_, PyAny>,
    age: Option<i64>,
    threads: Option<i64>,
    options: Option<&Bound<'_, PyDict>>,
) -> PyResult<Vec<Tree>> {
    let (stripped, positions) = split_positions(plants)?;
    let jobs = crate::dict_jobs(stripped.as_any(), age, options)?;
    let textured = jobs.iter().any(|job| job.2);
    let pool = pool(threads)?;
    let run = || {
        let mut generator = Generator::new();
        generator.set_textures(textured.then(job::texture_cache));
        let entries: Vec<GroupEntry> = jobs
            .iter()
            .zip(&positions)
            .map(|((species, params, _), position)| (species.as_ref(), params.clone(), *position))
            .collect();
        generator.generate_group(&entries)
    };
    let plants = py
        .detach(|| match &pool {
            Some(pool) => pool.install(run),
            None => run(),
        })
        .map_err(|e| gen_error(&e))?;
    Ok(jobs
        .into_iter()
        .zip(plants)
        .map(|((species, _, _), plant)| Tree::new(TreeData::new(species, plant)))
        .collect())
}

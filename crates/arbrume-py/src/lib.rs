//! Python bindings of Arbrume (`PyO3`, stable ABI `abi3-py311`).
//!
//! The extension module is `arbrume._arbrume`; the `arbrume` package re-exports it. One
//! wheel per platform loads in every `CPython` from 3.11 on, which covers Blender 4.2 LTS
//! and later.
//!
//! * `generate` grows and meshes one tree, without the GIL.
//! * `generate_many` runs a batch on the `rayon` pool without the GIL, with the same
//!   results as `generate` on each job whatever the number of threads.
//! * A `Tree` exposes its mesh parts, skeleton and organ sites as read-only `NumPy`
//!   arrays over the Rust memory, through the buffer protocol (see [`buffer`]).
//! * `fit` fits species parameters to a target skeleton and `distance` compares two
//!   skeletons, without the GIL (see [`fit`]).
//! * `load_pack`, `packs`, `install_pack`, `remove_pack` and `packs_dir` handle species
//!   packs, whose species `generate` accepts as `"pack_id/name"` (see [`packs`]).
//! * `voxel_mask` builds a voxel volume, the `surroundings` keyword places a plant in an
//!   envelope and among obstacles, and `growth_frames` meshes one plant at several ages
//!   (see [`surroundings`]).
//! * The `guides` keyword steers the growth with painted strokes, and `Tree.guides`
//!   reports what each stroke did (see [`guides`]).
//!
//! The module is tested from Python (`tests/`, pytest) and documented in
//! `docs/python.md`.

mod buffer;
mod convert;
mod data;
mod fit;
mod forest;
mod guides;
mod job;
mod packs;
mod roots;
mod stand;
mod surroundings;
mod tree;

use std::sync::Arc;

use arbrume_core::species::Species;
use arbrume_gen::{GenError, GenParams, VERSION, plant_seed as gen_plant_seed, run_batch};
use pyo3::exceptions::{PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyString};

use crate::buffer::Buffer;
use crate::convert::gen_error;
use crate::data::{ORGAN_KINDS, TreeData};
use crate::job::{Options, SpeciesCache, age, params, seed, species_text};
use crate::tree::{MeshPart, Organs, Skeleton, Tree};

/// One job of a batch: a parsed species, the parameters of one plant and whether it
/// carries textures.
type Job = (Arc<Species>, GenParams, bool);

/// Grows and meshes every job on the current `rayon` pool, in input order.
fn run_jobs(jobs: &[Job]) -> Vec<Result<TreeData, GenError>> {
    let cache = jobs.iter().any(|job| job.2).then(job::texture_cache);
    run_batch(jobs.len(), |generator, _, index| {
        let (species, params, textured) = &jobs[index];
        generator.set_textures(cache.clone().filter(|_| *textured));
        generator
            .generate(species, params)
            .map(|plant| TreeData::new(Arc::clone(species), plant))
    })
}

/// Runs `jobs` without the GIL, on a pool of `threads` threads or on the global pool.
fn run_detached(py: Python<'_>, jobs: &[Job], threads: Option<i64>) -> PyResult<Vec<Tree>> {
    let pool = match threads {
        None => None,
        Some(count) => {
            let count = usize::try_from(count)
                .ok()
                .filter(|count| *count > 0)
                .ok_or_else(|| PyValueError::new_err(format!("threads {count} is not >= 1")))?;
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(count)
                .build()
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
            Some(pool)
        }
    };
    let results = py.detach(|| match &pool {
        Some(pool) => pool.install(|| run_jobs(jobs)),
        None => run_jobs(jobs),
    });
    results
        .into_iter()
        .enumerate()
        .map(|(index, result)| {
            result
                .map(Tree::new)
                .map_err(|e| PyValueError::new_err(format!("job {index}: {e}")))
        })
        .collect()
}

/// Generates one tree.
///
/// `species` is a preset name, a path to a species TOML file or TOML text. `seed` seeds
/// the growth and the mesh; `age` is in growth cycles (default 10). Keyword options:
/// `scale` (default 1), `quality` (mesh quality, default 1), `textures` (procedural
/// bark and leaf textures on the materials, default `False`), `junctions` (`"tier-a"`,
/// the default, or `"tier-b"` for one closed bark surface), `surroundings` (a dict with
/// `envelope` and `occluders`, or the path of a surroundings TOML file; read by species
/// in the self-organizing mode), `guides` (strokes and freezes: a dict, JSON text or the
/// path of a JSON file; read by species in the self-organizing mode), `roots` (a bool, a
/// habit name or a dict: the root system, see `docs/roots.md`), `soil` (a dict or the path of
/// a TOML file: ground and resources for the roots), `overrides` (a dict of
/// dotted species paths to values), and any other keyword is one more override, e.g.
/// `**{"foliage.length": 0.2}`. Raises `ValueError` for an invalid species or parameter.
#[pyfunction]
#[pyo3(signature = (species, seed=0, age=None, **options))]
fn generate(
    py: Python<'_>,
    species: &Bound<'_, PyAny>,
    seed: i128,
    age: Option<i64>,
    options: Option<&Bound<'_, PyDict>>,
) -> PyResult<Tree> {
    let text = species_text(species)?;
    let options = Options::parse(options)?;
    let species = SpeciesCache::default().get(text, &options.overrides)?;
    let params = params(job::seed(seed)?, job::age(age)?, &options);
    let cache = options.textures.then(job::texture_cache);
    let plant = py.detach(|| {
        let mut generator = arbrume_gen::Generator::new();
        generator.set_textures(cache);
        generator.generate(&species, &params)
    });
    let plant = plant.map_err(|e| gen_error(&e))?;
    Ok(Tree::new(TreeData::new(species, plant)))
}

/// Builds the jobs of one species with many seeds.
fn seed_jobs(
    species: &Bound<'_, PyAny>,
    seeds: &Bound<'_, PyAny>,
    plant_age: Option<i64>,
    options: Option<&Bound<'_, PyDict>>,
) -> PyResult<Vec<Job>> {
    let text = species_text(species)?;
    let options = Options::parse(options)?;
    let species = SpeciesCache::default().get(text, &options.overrides)?;
    let plant_age = age(plant_age)?;
    seeds
        .try_iter()?
        .map(|item| {
            let plant_seed = seed(item?.extract::<i128>()?)?;
            Ok((
                Arc::clone(&species),
                params(plant_seed, plant_age, &options),
                options.textures,
            ))
        })
        .collect()
}

/// Builds the jobs of a list of job dicts, `defaults` filling the keys a job lacks.
fn dict_jobs(
    jobs: &Bound<'_, PyAny>,
    default_age: Option<i64>,
    defaults: Option<&Bound<'_, PyDict>>,
) -> PyResult<Vec<Job>> {
    let py = jobs.py();
    let mut cache = SpeciesCache::default();
    let mut built = Vec::new();
    for (index, item) in jobs.try_iter()?.enumerate() {
        let merged = match defaults {
            Some(defaults) => defaults.copy()?,
            None => PyDict::new(py),
        };
        let item = item?;
        let job = item.cast::<PyDict>().map_err(|_| {
            PyTypeError::new_err(format!(
                "job {index} is not a dict; pass seeds= for one species"
            ))
        })?;
        merged.update(job.as_mapping())?;
        let species = merged
            .get_item("species")?
            .ok_or_else(|| PyValueError::new_err(format!("job {index} has no `species`")))?;
        let plant_seed = take(&merged, "seed")?.map_or(Ok(0), |v| v.extract::<i128>())?;
        let plant_age = match take(&merged, "age")? {
            Some(value) => value.extract::<Option<i64>>()?,
            None => default_age,
        };
        merged.del_item("species")?;
        let options = Options::parse(Some(&merged))?;
        let species = cache.get(species_text(&species)?, &options.overrides)?;
        let params = params(seed(plant_seed)?, age(plant_age)?, &options);
        built.push((species, params, options.textures));
    }
    Ok(built)
}

/// Removes `key` from `dict` and returns its value.
fn take<'py>(dict: &Bound<'py, PyDict>, key: &str) -> PyResult<Option<Bound<'py, PyAny>>> {
    let value = dict.get_item(key)?;
    if value.is_some() {
        dict.del_item(key)?;
    }
    Ok(value)
}

/// Generates many trees in parallel, without the GIL.
///
/// Either `generate_many(species, seeds, age=None, **options)`, one tree per seed, or
/// `generate_many(jobs, **defaults)` where `jobs` is a list of dicts with the keys of
/// `generate` (`species` required; `seed`, `age`, `scale`, `quality`, `textures`, `junctions`,
/// `surroundings`, `overrides`, dotted overrides). `threads` sets the number of worker threads (default: the global
/// `rayon` pool, all cores or `RAYON_NUM_THREADS`). The trees are in input order and
/// equal those of `generate` on each job, whatever the number of threads.
#[pyfunction]
#[pyo3(signature = (species, seeds=None, age=None, *, threads=None, **options))]
fn generate_many(
    py: Python<'_>,
    species: &Bound<'_, PyAny>,
    seeds: Option<&Bound<'_, PyAny>>,
    age: Option<i64>,
    threads: Option<i64>,
    options: Option<&Bound<'_, PyDict>>,
) -> PyResult<Vec<Tree>> {
    let single = species.is_instance_of::<PyString>() || species.hasattr("__fspath__")?;
    let jobs = match (single, seeds) {
        (_, Some(seeds)) => seed_jobs(species, seeds, age, options)?,
        (false, None) => dict_jobs(species, age, options)?,
        (true, None) => {
            return Err(PyTypeError::new_err(
                "generate_many(species, seeds) needs seeds for a single species",
            ));
        }
    };
    run_detached(py, &jobs, threads)
}

/// Names of the built-in species presets, sorted.
#[pyfunction]
fn presets() -> Vec<&'static str> {
    arbrume_gen::presets()
        .iter()
        .map(|(name, _)| *name)
        .collect()
}

/// TOML text of the preset `name`; `ValueError` if there is no such preset.
#[pyfunction]
fn preset_toml(name: &str) -> PyResult<&'static str> {
    arbrume_gen::preset_toml(name)
        .ok_or_else(|| PyValueError::new_err(format!("no preset named `{name}`")))
}

/// Seed of plant `index` of a dataset with base seed `seed`, as the command line draws
/// it.
#[pyfunction]
fn plant_seed(seed: u64, index: u32) -> u64 {
    gen_plant_seed(seed, index)
}

/// The extension module `arbrume._arbrume`.
#[pymodule(name = "_arbrume")]
fn arbrume_module(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add("__version__", VERSION)?;
    let kinds: Vec<String> = ORGAN_KINDS
        .iter()
        .map(|kind| format!("{kind:?}").to_lowercase())
        .collect();
    module.add(
        "ORGAN_KINDS",
        pyo3::types::PyTuple::new(module.py(), kinds)?,
    )?;
    module.add_function(wrap_pyfunction!(generate, module)?)?;
    module.add_function(wrap_pyfunction!(generate_many, module)?)?;
    module.add_function(wrap_pyfunction!(stand::generate_stand, module)?)?;
    module.add_function(wrap_pyfunction!(forest::forest_build, module)?)?;
    module.add_function(wrap_pyfunction!(presets, module)?)?;
    module.add_function(wrap_pyfunction!(preset_toml, module)?)?;
    module.add_function(wrap_pyfunction!(plant_seed, module)?)?;
    module.add_function(wrap_pyfunction!(fit::fit, module)?)?;
    module.add_function(wrap_pyfunction!(fit::distance, module)?)?;
    module.add_function(wrap_pyfunction!(packs::load_pack, module)?)?;
    module.add_function(wrap_pyfunction!(packs::packs, module)?)?;
    module.add_function(wrap_pyfunction!(packs::install_pack, module)?)?;
    module.add_function(wrap_pyfunction!(packs::remove_pack, module)?)?;
    module.add_function(wrap_pyfunction!(packs::packs_dir, module)?)?;
    module.add_function(wrap_pyfunction!(surroundings::voxel_mask, module)?)?;
    module.add_function(wrap_pyfunction!(surroundings::growth_frames, module)?)?;
    module.add_class::<surroundings::PyVoxelMask>()?;
    module.add_class::<Tree>()?;
    module.add_class::<MeshPart>()?;
    module.add_class::<Skeleton>()?;
    module.add_class::<Organs>()?;
    module.add_class::<Buffer>()?;
    Ok(())
}

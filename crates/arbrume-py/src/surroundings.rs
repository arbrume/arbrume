//! Surroundings, voxel masks and growth frames from Python.
//!
//! * The `surroundings` keyword of `generate`, `generate_many` and `growth_frames` is a
//!   dict with the keys of a `--surroundings` file of the command line (`envelope`,
//!   `occluders`), or the path of such a TOML file. Where a shape is expected, a
//!   [`VoxelMask`] made by `voxel_mask` may stand: as the `envelope`, as an item of
//!   `occluders` (a solid obstacle), or as the `shape` of an occluder dict that also
//!   gives `blocks` and `shade`.
//! * `voxel_mask(origin, cell, shape, mask)` builds a voxel mask from a 3-D bool array
//!   indexed `[x, y, z]`, in meters in the frame of the plant (Y up).
//! * `growth_frames` grows a plant once and meshes it at several ages
//!   (`arbrume_gen::Generator::timeline` and `frame`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use arbrume_gen::{GenError, Generator, Occluder, Plant, Shape, Surroundings};
use glam::Vec3;
use pyo3::exceptions::{PyFileNotFoundError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyString};

use crate::convert::{gen_error, py_to_json};
use crate::data::TreeData;
use crate::job::{self, Options, SpeciesCache, params, species_text};
use crate::tree::Tree;

/// Largest number of voxels of a mask.
const MAX_VOXELS: u64 = 1 << 30;

/// A volume given as a grid of filled and empty cubic voxels, made by `voxel_mask`.
#[pyclass(frozen, name = "VoxelMask", module = "arbrume._arbrume")]
#[derive(Debug, Clone)]
pub struct PyVoxelMask {
    mask: Arc<arbrume_gen::VoxelMask>,
}

#[pymethods]
impl PyVoxelMask {
    /// Corner of smallest coordinates, in meters.
    #[getter]
    fn origin(&self) -> (f32, f32, f32) {
        self.mask.bounds().min.into()
    }

    /// Edge of a voxel, in meters.
    #[getter]
    fn cell(&self) -> f32 {
        self.mask.cell()
    }

    /// Number of voxels along X, Y and Z.
    #[getter]
    fn shape(&self) -> (u32, u32, u32) {
        let [x, y, z] = self.mask.size();
        (x, y, z)
    }

    /// Number of filled voxels.
    #[getter]
    fn filled(&self) -> usize {
        self.mask.filled().count()
    }

    /// Smallest and largest corner of the grid.
    #[getter]
    fn bounds(&self) -> ((f32, f32, f32), (f32, f32, f32)) {
        let bounds = self.mask.bounds();
        (bounds.min.into(), bounds.max.into())
    }

    /// Whether `point` lies in a filled voxel.
    fn contains(&self, point: (f32, f32, f32)) -> bool {
        self.mask.contains(Vec3::from(point))
    }

    fn __repr__(&self) -> String {
        let [x, y, z] = self.mask.size();
        format!(
            "<arbrume.VoxelMask {x}x{y}x{z} cell={} filled={}>",
            self.mask.cell(),
            self.filled()
        )
    }
}

/// Builds a voxel mask: `shape` voxels `(nx, ny, nz)` of edge `cell` meters, the corner of
/// smallest coordinates at `origin`, voxel `[x, y, z]` filled where `mask[x, y, z]` is
/// true. `mask` is anything `numpy.asarray` turns into a 3-D array of that shape.
#[pyfunction]
pub fn voxel_mask(
    origin: (f32, f32, f32),
    cell: f32,
    shape: (u32, u32, u32),
    mask: &Bound<'_, PyAny>,
) -> PyResult<PyVoxelMask> {
    let origin = Vec3::from(origin);
    if !origin.is_finite() {
        return Err(PyValueError::new_err("origin is not finite"));
    }
    if !(cell > 0.0 && cell.is_finite()) {
        return Err(PyValueError::new_err(format!(
            "cell {cell} is not positive and finite"
        )));
    }
    let size = [shape.0, shape.1, shape.2];
    let count = size.iter().map(|&n| u64::from(n)).product::<u64>();
    if count == 0 || count > MAX_VOXELS {
        let reason = format!("shape {shape:?} must have between 1 and {MAX_VOXELS} voxels");
        return Err(PyValueError::new_err(reason));
    }
    let bytes = mask_bytes(mask, size)?;
    let mut voxels = arbrume_gen::VoxelMask::new(origin, cell, size);
    let [_, ny, nz] = size.map(|n| n as usize);
    for (index, _) in bytes.iter().enumerate().filter(|(_, byte)| **byte != 0) {
        let (x, y, z) = (index / (ny * nz), (index / nz) % ny, index % nz);
        voxels.set([x as u32, y as u32, z as u32], true);
    }
    Ok(PyVoxelMask {
        mask: Arc::new(voxels),
    })
}

/// The bytes of `mask` as a C-ordered bool array, checked to have the shape `size`.
fn mask_bytes(mask: &Bound<'_, PyAny>, size: [u32; 3]) -> PyResult<Vec<u8>> {
    let numpy = mask.py().import("numpy")?;
    let kwargs = PyDict::new(mask.py());
    kwargs.set_item("dtype", numpy.getattr("bool_")?)?;
    let array = numpy.call_method("ascontiguousarray", (mask,), Some(&kwargs))?;
    let found: Vec<usize> = array.getattr("shape")?.extract()?;
    if found != size.map(|n| n as usize) {
        let reason = format!("mask has shape {found:?}, expected {size:?}");
        return Err(PyValueError::new_err(reason));
    }
    array.call_method0("tobytes")?.extract()
}

/// The surroundings of the `surroundings` keyword (see the module documentation);
/// `None` for `None`.
pub fn parse(value: &Bound<'_, PyAny>) -> PyResult<Option<Arc<Surroundings>>> {
    if value.is_none() {
        return Ok(None);
    }
    let surroundings = if let Ok(dict) = value.cast::<PyDict>() {
        from_dict(dict)?
    } else if value.is_instance_of::<PyString>() || value.hasattr("__fspath__")? {
        from_file(&value.extract::<PathBuf>()?)?
    } else {
        return Err(PyTypeError::new_err(
            "`surroundings` must be a dict or the path of a TOML file",
        ));
    };
    surroundings
        .validate()
        .map_err(|reason| PyValueError::new_err(format!("surroundings: {reason}")))?;
    Ok(Some(Arc::new(surroundings)))
}

/// The surroundings of a TOML file.
fn from_file(path: &Path) -> PyResult<Surroundings> {
    let text = std::fs::read_to_string(path).map_err(|error| {
        let message = format!("surroundings file {}: {error}", path.display());
        match error.kind() {
            std::io::ErrorKind::NotFound => PyFileNotFoundError::new_err(message),
            _ => PyErr::from(error),
        }
    })?;
    toml::from_str(&text)
        .map_err(|error| PyValueError::new_err(format!("{}: {error}", path.display())))
}

/// The surroundings of a dict.
fn from_dict(dict: &Bound<'_, PyDict>) -> PyResult<Surroundings> {
    let mut surroundings = Surroundings::none();
    for (key, value) in dict.iter() {
        match key.extract::<String>()?.as_str() {
            "envelope" if value.is_none() => {}
            "envelope" => surroundings.envelope = Some(shape(&value, "envelope")?),
            "occluders" => {
                for (index, item) in value.try_iter()?.enumerate() {
                    surroundings.occluders.push(occluder(&item?, index)?);
                }
            }
            other => {
                return Err(PyValueError::new_err(format!(
                    "surroundings: unknown key `{other}`, expected `envelope` or `occluders`"
                )));
            }
        }
    }
    Ok(surroundings)
}

/// A shape: a voxel mask, or a dict with a `shape` key.
fn shape(value: &Bound<'_, PyAny>, what: &str) -> PyResult<Shape> {
    if let Ok(mask) = value.cast::<PyVoxelMask>() {
        return Ok(Shape::Voxels(Arc::clone(&mask.get().mask)));
    }
    serde_json::from_value(py_to_json(value)?)
        .map_err(|error| PyValueError::new_err(format!("surroundings {what}: {error}")))
}

/// Occluder `index`: a voxel mask (a solid obstacle) or a dict.
fn occluder(item: &Bound<'_, PyAny>, index: usize) -> PyResult<Occluder> {
    let what = format!("occluder {index}");
    let Ok(dict) = item.cast::<PyDict>() else {
        return Ok(Occluder::obstacle(shape(item, &what)?));
    };
    let Some(mask) = dict
        .get_item("shape")?
        .filter(PyAnyMethods::is_instance_of::<PyVoxelMask>)
    else {
        return serde_json::from_value(py_to_json(item)?)
            .map_err(|error| PyValueError::new_err(format!("surroundings {what}: {error}")));
    };
    let mut occluder = Occluder::obstacle(shape(&mask, &what)?);
    for (key, value) in dict.iter() {
        match key.extract::<String>()?.as_str() {
            "shape" => {}
            "blocks" => occluder.blocks = value.extract()?,
            "shade" => occluder.shade = value.extract()?,
            other => {
                let reason = format!("surroundings {what}: unknown key `{other}`");
                return Err(PyValueError::new_err(reason));
            }
        }
    }
    Ok(occluder)
}

/// Grows one plant once and meshes it at `frames` ages spread evenly over its growth,
/// the last being its full age; fewer frames when the plant has fewer cycles.
///
/// Takes the arguments and keyword options of `generate`. Returns one tree per frame, in
/// growing order; `tree.max_cycle` is the last growth cycle the frame shows.
#[pyfunction]
#[pyo3(signature = (species, seed=0, age=None, frames=10, **options))]
pub fn growth_frames(
    py: Python<'_>,
    species: &Bound<'_, PyAny>,
    seed: i128,
    age: Option<i64>,
    frames: i64,
    options: Option<&Bound<'_, PyDict>>,
) -> PyResult<Vec<Tree>> {
    let frames = u16::try_from(frames)
        .ok()
        .filter(|frames| *frames > 0)
        .ok_or_else(|| PyValueError::new_err(format!("frames {frames} is not in [1, 65535]")))?;
    let text = species_text(species)?;
    let options = Options::parse(options)?;
    let species = SpeciesCache::default().get(text, &options.overrides)?;
    let params = params(job::seed(seed)?, job::age(age)?, &options);
    let cache = options.textures.then(job::texture_cache);
    let plants = py.detach(|| -> Result<Vec<Plant>, GenError> {
        let mut generator = Generator::new();
        generator.set_textures(cache);
        let timeline = generator.timeline(&species, &params)?;
        timeline
            .frame_cycles(frames)
            .into_iter()
            .map(|cycle| {
                let mut plant = Plant::default();
                generator.frame(&species, &timeline, cycle, &mut plant)?;
                Ok(plant)
            })
            .collect()
    });
    let plants = plants.map_err(|e| gen_error(&e))?;
    Ok(plants
        .into_iter()
        .map(|plant| Tree::new(TreeData::new(Arc::clone(&species), plant)))
        .collect())
}

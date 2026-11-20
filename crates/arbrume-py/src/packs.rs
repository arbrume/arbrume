//! Species packs from Python: `load_pack`, `packs`, `install_pack`, `remove_pack`,
//! `packs_dir`, and the resolution of `pack_id/name` species for `generate`.
//!
//! A pack is described by a dict (see [`describe`]). Packs given to `load_pack` are
//! kept for the life of the process and take precedence over installed packs of the
//! same id; installed packs are looked up in the pack directory (`ARBRUME_PACKS_DIR`
//! or the user data directory) each time a species is resolved, so packs installed
//! meanwhile are found.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use arbrume_pack::{Pack, PackError, PackSource, PackSpecies, PackStore, split_species_id};
use pyo3::exceptions::{PyFileNotFoundError, PyOSError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

/// Packs loaded with `load_pack`, by id.
static LOADED: Mutex<BTreeMap<String, Arc<Pack>>> = Mutex::new(BTreeMap::new());

/// A pack error as a Python exception: `FileNotFoundError` for a missing pack or
/// species, `OSError` for a file that cannot be read or written, `ValueError` else.
pub fn pack_error(error: &PackError) -> PyErr {
    match error {
        PackError::NotFound(message) => PyFileNotFoundError::new_err(message.clone()),
        PackError::Io { .. } => PyOSError::new_err(error.to_string()),
        _ => PyValueError::new_err(error.to_string()),
    }
}

/// The store in `dir`, or the default one.
fn store(dir: Option<PathBuf>) -> PyResult<PackStore> {
    match dir {
        Some(dir) => Ok(PackStore::new(dir)),
        None => PackStore::open_default().map_err(|e| pack_error(&e)),
    }
}

/// The species `id` (`pack_id/name`) of a loaded or installed pack, or `None` when `id`
/// is not of that form.
pub fn resolve(id: &str) -> PyResult<Option<PackSpecies>> {
    let Some((pack_id, name)) = split_species_id(id) else {
        return Ok(None);
    };
    let loaded = LOADED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(pack_id)
        .cloned();
    let species = match loaded {
        Some(pack) => pack.species(name),
        None => store(None)?.species(id),
    };
    species.map(Some).map_err(|e| pack_error(&e))
}

/// The dict describing `pack`: `id`, `name`, `version`, `authors`, `description`,
/// `content_licence`, `licence_file`, `min_schema_version`, `path`, `archive` (whether
/// it was read from an archive) and `species`, a list of dicts with `id`, `name`,
/// `display_name`, `category`, `thumbnail` (a file path or `None`) and `preview`.
fn describe<'py>(py: Python<'py>, pack: &Pack) -> PyResult<Bound<'py, PyDict>> {
    let manifest = pack.manifest();
    let dict = PyDict::new(py);
    dict.set_item("id", &manifest.id)?;
    dict.set_item("name", &manifest.name)?;
    dict.set_item("version", &manifest.version)?;
    dict.set_item("authors", &manifest.authors)?;
    dict.set_item("description", &manifest.description)?;
    dict.set_item("content_licence", &manifest.content_licence)?;
    dict.set_item("licence_file", &manifest.licence_file)?;
    dict.set_item("min_schema_version", manifest.min_schema_version)?;
    dict.set_item("path", pack.source().path())?;
    dict.set_item("archive", matches!(pack.source(), PackSource::Archive(_)))?;
    let species = PyList::empty(py);
    for entry in &manifest.species {
        let resolved = pack.species(&entry.name).map_err(|e| pack_error(&e))?;
        let item = PyDict::new(py);
        item.set_item("id", &resolved.id)?;
        item.set_item("name", &entry.name)?;
        item.set_item("display_name", &entry.display_name)?;
        item.set_item("category", &entry.category)?;
        item.set_item("thumbnail", resolved.thumbnail.as_deref())?;
        item.set_item("preview", entry.preview.as_deref())?;
        species.append(item)?;
    }
    dict.set_item("species", species)?;
    Ok(dict)
}

/// Opens the pack at `path` (a pack directory or a `.arbpack` file), checks its
/// structure and keeps it for this process, so that `generate` accepts its species as
/// `"pack_id/name"`. Returns the dict of the pack (see `packs`). Raises
/// `FileNotFoundError` or `OSError` when it cannot be read and `ValueError` for an
/// invalid pack.
#[pyfunction]
pub fn load_pack(py: Python<'_>, path: PathBuf) -> PyResult<Bound<'_, PyDict>> {
    let pack = py
        .detach(move || Pack::open(&path))
        .map_err(|e| pack_error(&e))?;
    let described = describe(py, &pack)?;
    let mut loaded = LOADED.lock().unwrap_or_else(PoisonError::into_inner);
    loaded.insert(pack.id().to_owned(), Arc::new(pack));
    Ok(described)
}

/// Every installed pack (in `dir`, by default the user pack directory), then every pack
/// loaded with `load_pack` whose id is not installed, as dicts with the keys `id`,
/// `name`, `version`, `authors`, `description`, `content_licence`, `licence_file`,
/// `min_schema_version`, `path`, `archive` and `species`. Installed packs that fail to
/// open are left out.
#[pyfunction]
#[pyo3(signature = (dir=None))]
pub fn packs(py: Python<'_>, dir: Option<PathBuf>) -> PyResult<Vec<Bound<'_, PyDict>>> {
    let store = store(dir)?;
    let installed = py.detach(|| store.list()).map_err(|e| pack_error(&e))?;
    let mut described = Vec::new();
    let mut ids = Vec::new();
    for pack in installed.into_iter().flatten() {
        ids.push(pack.id().to_owned());
        described.push(describe(py, &pack)?);
    }
    let loaded = LOADED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    for (id, pack) in loaded {
        if !ids.contains(&id) {
            described.push(describe(py, &pack)?);
        }
    }
    Ok(described)
}

/// Validates the pack at `path` (a `.arbpack` file or a pack directory) and installs
/// it into `dir` (by default the user pack directory), replacing an installed pack of
/// the same id. Returns the dict of the installed pack. Raises `ValueError` listing the
/// errors of an invalid pack, which is then not installed.
#[pyfunction]
#[pyo3(signature = (path, dir=None))]
pub fn install_pack(
    py: Python<'_>,
    path: PathBuf,
    dir: Option<PathBuf>,
) -> PyResult<Bound<'_, PyDict>> {
    let store = store(dir)?;
    let outcome = py
        .detach(move || store.install(&path))
        .map_err(|e| pack_error(&e))?;
    let pack = Pack::open(&outcome.path).map_err(|e| pack_error(&e))?;
    describe(py, &pack)
}

/// Removes the installed pack `id` from `dir` (by default the user pack directory).
/// Raises `FileNotFoundError` when it is not installed.
#[pyfunction]
#[pyo3(signature = (id, dir=None))]
pub fn remove_pack(id: &str, dir: Option<PathBuf>) -> PyResult<()> {
    store(dir)?.remove(id).map_err(|e| pack_error(&e))?;
    Ok(())
}

/// The user pack directory: `ARBRUME_PACKS_DIR` when set, else the platform data
/// directory (`%APPDATA%\arbrume\packs`, `~/Library/Application Support/arbrume/packs`
/// or `~/.local/share/arbrume/packs`).
#[pyfunction]
pub fn packs_dir() -> PyResult<PathBuf> {
    arbrume_pack::packs_dir().map_err(|e| pack_error(&e))
}

//! The directory of installed packs.
//!
//! Each installed pack is a directory named by its id, holding the files of
//! `build_files` (thumbnails complete, `MANIFEST.sha256` listing the hashes, checked
//! every time the pack is opened). The directory is, in this order:
//!
//! 1. the value of the environment variable [`PACKS_DIR_ENV`] when set and not empty;
//! 2. on Windows `%APPDATA%\arbrume\packs`;
//! 3. on macOS `~/Library/Application Support/arbrume/packs`;
//! 4. elsewhere `$XDG_DATA_HOME/arbrume/packs`, or `~/.local/share/arbrume/packs`.
//!
//! Installing writes the new pack beside the old one under a hidden name, then swaps
//! the directories, so a pack is never seen half written.

use std::path::{Path, PathBuf};

use crate::PackError;
use crate::bundle::build_files;
use crate::pack::{Pack, PackSpecies, split_species_id, write_atomic};
use crate::paths::check_name;

/// Environment variable that sets the directory of installed packs.
pub const PACKS_DIR_ENV: &str = "ARBRUME_PACKS_DIR";

/// The directory of installed packs (see the module documentation).
///
/// # Errors
///
/// Returns [`PackError::NotFound`] when neither the variable nor a home directory is
/// set.
pub fn packs_dir() -> Result<PathBuf, PackError> {
    let var = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty());
    if let Some(dir) = var(PACKS_DIR_ENV) {
        return Ok(PathBuf::from(dir));
    }
    let base = if cfg!(windows) {
        var("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|home| PathBuf::from(home).join("Library/Application Support"))
    } else {
        let xdg = var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute());
        xdg.or_else(|| var("HOME").map(|home| PathBuf::from(home).join(".local/share")))
    };
    base.map(|base| base.join("arbrume").join("packs"))
        .ok_or_else(|| {
            PackError::NotFound(format!(
                "no directory for installed packs: set {PACKS_DIR_ENV} or a home directory"
            ))
        })
}

/// What [`PackStore::install`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallOutcome {
    /// Id of the pack.
    pub id: String,
    /// Version installed.
    pub version: String,
    /// Version it replaced, if the pack was installed already.
    pub replaced: Option<String>,
    /// Directory of the installed pack.
    pub path: PathBuf,
}

/// A directory of installed packs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackStore {
    dir: PathBuf,
}

impl PackStore {
    /// The store in `dir`, which need not exist yet.
    #[must_use]
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The store in [`packs_dir`].
    ///
    /// # Errors
    ///
    /// The error of [`packs_dir`].
    pub fn open_default() -> Result<Self, PackError> {
        packs_dir().map(Self::new)
    }

    /// The directory of the store.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Ids of the installed packs, sorted: the directories that are not hidden.
    fn ids(&self) -> Result<Vec<String>, PackError> {
        let entries = match std::fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(PackError::io(&self.dir, &error)),
        };
        let mut ids = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| PackError::io(&self.dir, &e))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with('.') && entry.path().is_dir() {
                ids.push(name);
            }
        }
        ids.sort();
        Ok(ids)
    }

    /// Every installed pack, sorted by id; a pack that fails to open gives its error.
    ///
    /// # Errors
    ///
    /// Returns [`PackError::Io`] when the directory cannot be listed.
    pub fn list(&self) -> Result<Vec<Result<Pack, PackError>>, PackError> {
        Ok(self
            .ids()?
            .into_iter()
            .map(|id| Pack::open(&self.dir.join(id)))
            .collect())
    }

    /// The installed pack `id`.
    ///
    /// # Errors
    ///
    /// Returns [`PackError::NotFound`] when no such pack is installed, and the errors
    /// of [`Pack::open`].
    pub fn find(&self, id: &str) -> Result<Pack, PackError> {
        check_name("pack id", id).map_err(PackError::Usage)?;
        let path = self.dir.join(id);
        if !path.is_dir() {
            let installed = self.ids().unwrap_or_default();
            let installed = if installed.is_empty() {
                "none".to_owned()
            } else {
                installed.join(", ")
            };
            return Err(PackError::NotFound(format!(
                "no pack `{id}` is installed in {} (installed: {installed}); \
                 install one with `arbrume pack install FILE`",
                self.dir.display()
            )));
        }
        Pack::open(&path)
    }

    /// The species `pack_id/name` of an installed pack.
    ///
    /// # Errors
    ///
    /// Returns [`PackError::Usage`] for an id that is not `pack_id/name`, and the
    /// errors of [`PackStore::find`] and [`Pack::species`].
    pub fn species(&self, id: &str) -> Result<PackSpecies, PackError> {
        let (pack, name) = split_species_id(id).ok_or_else(|| {
            PackError::Usage(format!("`{id}` is not a pack species id `pack_id/name`"))
        })?;
        self.find(pack)?.species(name)
    }

    /// Validates the pack at `source` (a directory or an archive) and installs it,
    /// replacing an installed pack of the same id.
    ///
    /// # Errors
    ///
    /// The errors of [`Pack::load`] and `build_files` (an invalid pack is not
    /// installed), and [`PackError::Io`] when the files cannot be written.
    pub fn install(&self, source: &Path) -> Result<InstallOutcome, PackError> {
        let pack = Pack::load(source)?;
        let files = build_files(&pack)?;
        let id = pack.id().to_owned();
        let target = self.dir.join(&id);
        let staging = self
            .dir
            .join(format!(".{id}.install-{}", std::process::id()));
        let io = |path: &Path| {
            let path = path.to_path_buf();
            move |reason: String| PackError::Io { path, reason }
        };
        if staging.exists() {
            std::fs::remove_dir_all(&staging).map_err(|e| PackError::io(&staging, &e))?;
        }
        for (path, bytes) in &files {
            let file = staging.join(path);
            write_atomic(&file, bytes).map_err(io(&file))?;
        }
        let replaced = Pack::load(&target)
            .ok()
            .map(|old| old.manifest().version.clone());
        let old = self.dir.join(format!(".{id}.old-{}", std::process::id()));
        if target.exists() {
            std::fs::rename(&target, &old).map_err(|e| PackError::io(&target, &e))?;
        }
        std::fs::rename(&staging, &target).map_err(|e| PackError::io(&target, &e))?;
        if old.exists() {
            std::fs::remove_dir_all(&old).map_err(|e| PackError::io(&old, &e))?;
        }
        Ok(InstallOutcome {
            id,
            version: pack.manifest().version.clone(),
            replaced,
            path: target,
        })
    }

    /// Removes the installed pack `id`; returns its directory.
    ///
    /// # Errors
    ///
    /// The errors of [`PackStore::find`] (the pack must be installed and readable to
    /// be removed by id), and [`PackError::Io`] when it cannot be deleted.
    pub fn remove(&self, id: &str) -> Result<PathBuf, PackError> {
        check_name("pack id", id).map_err(PackError::Usage)?;
        let path = self.dir.join(id);
        if !path.join(crate::PACK_FILE).is_file() {
            return Err(self.find(id).err().unwrap_or_else(|| {
                PackError::NotFound(format!("no pack `{id}` in {}", self.dir.display()))
            }));
        }
        std::fs::remove_dir_all(&path).map_err(|e| PackError::io(&path, &e))?;
        Ok(path)
    }
}

//! A pack in memory: its manifest and files, read from a directory or an archive.
//!
//! Every file of a pack is read into memory when it is opened (packs are limited to
//! `MAX_PACK_BYTES`), so validating, building and installing never read the source
//! twice. Generating a species with image textures needs them as files: a directory
//! pack is used in place, while the images and thumbnails of an archive are extracted
//! once into `<temp dir>/arbrume-packs/<id>-<hash>/`, named by a hash of the contents
//! so that two versions never share a directory.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use arbrume_core::species::Species;

use crate::hashes::{hash_manifest, sha256_hex, verify_hashes};
use crate::manifest::{PackManifest, SpeciesEntry, migrate_species};
use crate::paths::{FileKind, check_content, check_name, check_path, kind};
use crate::validate::Issue;
use crate::{HASHES_FILE, MAX_FILE_BYTES, MAX_PACK_BYTES, PACK_FILE, PackError};

/// Where a pack was read from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackSource {
    /// A pack directory.
    Directory(PathBuf),
    /// A `.arbpack` archive.
    Archive(PathBuf),
}

impl PackSource {
    /// The directory or archive.
    #[must_use]
    pub fn path(&self) -> &Path {
        match self {
            Self::Directory(path) | Self::Archive(path) => path,
        }
    }
}

/// A pack: its manifest and every file. See the module documentation.
#[derive(Debug)]
pub struct Pack {
    manifest: PackManifest,
    files: BTreeMap<String, Vec<u8>>,
    source: PackSource,
    extracted: OnceLock<PathBuf>,
}

/// One species of a pack, ready to generate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackSpecies {
    /// `pack_id/name`.
    pub id: String,
    /// Its entry in `pack.toml`.
    pub entry: SpeciesEntry,
    /// TOML text of the species file, migrated to the current schema.
    pub text: String,
    /// Directory its texture paths are relative to, when it names image files.
    pub asset_dir: Option<PathBuf>,
    /// Thumbnail file on disk, if the pack has one for the species.
    pub thumbnail: Option<PathBuf>,
}

impl PackSpecies {
    /// The parsed species, with its asset directory.
    ///
    /// # Errors
    ///
    /// Returns [`PackError::Invalid`] when the species file is not a valid species.
    pub fn species(&self) -> Result<Species, PackError> {
        let mut species = Species::from_toml_str(&self.text).map_err(|e| PackError::Invalid {
            path: PathBuf::from(&self.id),
            issues: vec![Issue::new(self.entry.file_path(), e.to_string())],
        })?;
        species.asset_dir.clone_from(&self.asset_dir);
        Ok(species)
    }
}

/// Splits a species id `pack_id/name` into its two names, when both are valid names.
#[must_use]
pub fn split_species_id(id: &str) -> Option<(&str, &str)> {
    let (pack, name) = id.split_once('/')?;
    let valid = check_name("pack id", pack).is_ok() && check_name("species", name).is_ok();
    valid.then_some((pack, name))
}

/// Reads every file under `dir`, skipping hidden files and directories (names starting
/// with `.`); symbolic links are refused.
fn read_dir_files(dir: &Path) -> Result<BTreeMap<String, Vec<u8>>, PackError> {
    let mut files = BTreeMap::new();
    let mut total = 0_u64;
    let mut pending = vec![(dir.to_path_buf(), String::new())];
    while let Some((path, prefix)) = pending.pop() {
        let entries = std::fs::read_dir(&path).map_err(|e| PackError::io(&path, &e))?;
        for entry in entries {
            let entry = entry.map_err(|e| PackError::io(&path, &e))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let full = entry.path();
            let meta = std::fs::symlink_metadata(&full).map_err(|e| PackError::io(&full, &e))?;
            let relative = format!("{prefix}{name}");
            if meta.file_type().is_symlink() {
                let reason = "symbolic links are not allowed in a pack".to_owned();
                return Err(PackError::Io { path: full, reason });
            }
            if meta.is_dir() {
                pending.push((full, format!("{relative}/")));
                continue;
            }
            total += meta.len();
            if meta.len() > MAX_FILE_BYTES || total > MAX_PACK_BYTES {
                let reason = format!("larger than the {MAX_FILE_BYTES} bytes a pack file may hold");
                return Err(PackError::Io { path: full, reason });
            }
            let bytes = std::fs::read(&full).map_err(|e| PackError::io(&full, &e))?;
            files.insert(relative, bytes);
        }
    }
    Ok(files)
}

impl Pack {
    /// Reads the pack at `path`, a directory or an archive, and parses `pack.toml`,
    /// without the checks of [`Pack::open`].
    ///
    /// # Errors
    ///
    /// Returns [`PackError::NotFound`] when nothing is at `path`, [`PackError::Io`] or
    /// [`PackError::Archive`] when the files cannot be read, and [`PackError::Invalid`]
    /// when `pack.toml` is missing or invalid.
    pub fn load(path: &Path) -> Result<Self, PackError> {
        let meta = std::fs::metadata(path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => PackError::NotFound(format!(
                "{}: no such pack directory or file",
                path.display()
            )),
            _ => PackError::io(path, &e),
        })?;
        if meta.is_dir() {
            let files = read_dir_files(path)?;
            return Self::from_files(files, PackSource::Directory(path.to_path_buf()));
        }
        if meta.len() > MAX_PACK_BYTES {
            let reason = format!("larger than {MAX_PACK_BYTES} bytes");
            return Err(PackError::Archive {
                path: path.to_path_buf(),
                reason,
            });
        }
        let bytes = std::fs::read(path).map_err(|e| PackError::io(path, &e))?;
        let files = crate::archive::read_archive(&bytes).map_err(|reason| PackError::Archive {
            path: path.to_path_buf(),
            reason,
        })?;
        Self::from_files(files, PackSource::Archive(path.to_path_buf()))
    }

    /// A pack of `files` read from `source`.
    ///
    /// # Errors
    ///
    /// Returns [`PackError::Invalid`] when `pack.toml` is missing or invalid.
    pub fn from_files(
        files: BTreeMap<String, Vec<u8>>,
        source: PackSource,
    ) -> Result<Self, PackError> {
        let invalid = |message: String| PackError::Invalid {
            path: source.path().to_path_buf(),
            issues: vec![Issue::new(PACK_FILE, message)],
        };
        let text = files
            .get(PACK_FILE)
            .ok_or_else(|| invalid("missing: a pack has a pack.toml at its root".to_owned()))?;
        let text = std::str::from_utf8(text).map_err(|_| invalid("not UTF-8".to_owned()))?;
        let manifest = PackManifest::from_toml_str(text).map_err(invalid)?;
        Ok(Self {
            manifest,
            files,
            source,
            extracted: OnceLock::new(),
        })
    }

    /// Reads the pack at `path` like [`Pack::load`] and checks its structure: allowed
    /// files, contents, hashes and the files `pack.toml` names ([`Pack::structure`]).
    ///
    /// # Errors
    ///
    /// The errors of [`Pack::load`], and [`PackError::Invalid`] listing every problem
    /// of structure.
    pub fn open(path: &Path) -> Result<Self, PackError> {
        let pack = Self::load(path)?;
        let issues = pack.structure();
        if issues.is_empty() {
            Ok(pack)
        } else {
            Err(PackError::Invalid {
                path: path.to_path_buf(),
                issues,
            })
        }
    }

    /// The parsed `pack.toml`.
    #[must_use]
    pub const fn manifest(&self) -> &PackManifest {
        &self.manifest
    }

    /// The identifier of the pack.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.manifest.id
    }

    /// Every file, path to bytes.
    #[must_use]
    pub const fn files(&self) -> &BTreeMap<String, Vec<u8>> {
        &self.files
    }

    /// Where the pack was read from.
    #[must_use]
    pub const fn source(&self) -> &PackSource {
        &self.source
    }

    /// Problems of structure: files a pack may not hold, executable content, hashes
    /// that do not match, files named by `pack.toml` that are missing, species files
    /// not listed.
    #[must_use]
    pub fn structure(&self) -> Vec<Issue> {
        let mut issues = Vec::new();
        let licence = &self.manifest.licence_file;
        for (path, bytes) in &self.files {
            if let Err(reason) = check_path(path) {
                issues.push(Issue::new(path, reason));
                continue;
            }
            match kind(path, licence) {
                None => issues.push(Issue::new(path, "a pack may not hold this file")),
                Some(kind) => {
                    if let Err(reason) = check_content(kind, bytes) {
                        issues.push(Issue::new(path, reason));
                    }
                }
            }
        }
        self.check_hashes(&mut issues);
        self.check_named_files(&mut issues);
        issues
    }

    /// Checks `MANIFEST.sha256`, which an archive must have.
    fn check_hashes(&self, issues: &mut Vec<Issue>) {
        match self.files.get(HASHES_FILE) {
            Some(bytes) => {
                let text = String::from_utf8_lossy(bytes);
                if let Err(problems) = verify_hashes(&self.files, &text) {
                    issues.extend(problems.into_iter().map(|p| Issue::new(HASHES_FILE, p)));
                }
            }
            None if matches!(self.source, PackSource::Archive(_)) => {
                issues.push(Issue::new(
                    HASHES_FILE,
                    "missing: an archive lists its hashes",
                ));
            }
            None => {}
        }
    }

    /// Checks the files `pack.toml` names and the species files it does not.
    fn check_named_files(&self, issues: &mut Vec<Issue>) {
        let licence = &self.manifest.licence_file;
        if self.files.get(licence).is_none_or(Vec::is_empty) {
            issues.push(Issue::new(licence, "the licence file is missing or empty"));
        }
        if self.manifest.species.is_empty() {
            issues.push(Issue::new(PACK_FILE, "a pack lists at least one species"));
        }
        for entry in &self.manifest.species {
            let mut named = vec![entry.file_path()];
            named.extend(entry.thumbnail.clone());
            named.extend(entry.preview.clone());
            for path in named {
                if !self.files.contains_key(&path) {
                    let message = format!("missing, named by species `{}`", entry.name);
                    issues.push(Issue::new(path, message));
                }
            }
        }
        for path in self.files.keys() {
            let listed = |e: &SpeciesEntry| e.file_path() == *path;
            if kind(path, licence) == Some(FileKind::Species)
                && !self.manifest.species.iter().any(listed)
            {
                issues.push(Issue::new(path, "species file not listed in pack.toml"));
            }
        }
    }

    /// The directory the image files of the species are relative to: the pack itself,
    /// or for an archive the directory its images and thumbnails are extracted to
    /// (see the module documentation). `None` when the pack has no such file.
    ///
    /// # Errors
    ///
    /// Returns [`PackError::Io`] when the files cannot be extracted.
    pub fn asset_dir(&self) -> Result<Option<PathBuf>, PackError> {
        let assets =
            |path: &&String| path.starts_with("textures/") || path.starts_with("thumbnails/");
        if !self.files.keys().any(|path| assets(&path)) {
            return Ok(None);
        }
        let archive = match &self.source {
            PackSource::Directory(dir) => return Ok(Some(dir.clone())),
            PackSource::Archive(path) => path,
        };
        if let Some(dir) = self.extracted.get() {
            return Ok(Some(dir.clone()));
        }
        let hash = sha256_hex(hash_manifest(&self.files).as_bytes());
        let name = format!("{}-{}", self.manifest.id, &hash[..16]);
        let dir = std::env::temp_dir().join("arbrume-packs").join(name);
        for (path, bytes) in self.files.iter().filter(|(path, _)| assets(path)) {
            let target = dir.join(path);
            if target.is_file() {
                continue;
            }
            write_atomic(&target, bytes).map_err(|reason| PackError::Archive {
                path: archive.clone(),
                reason,
            })?;
        }
        Ok(Some(self.extracted.get_or_init(|| dir).clone()))
    }

    /// The species `name` of the pack.
    ///
    /// # Errors
    ///
    /// Returns [`PackError::NotFound`] for a species the pack does not list,
    /// [`PackError::Invalid`] for a species file that cannot be read for this schema,
    /// and the errors of [`Pack::asset_dir`].
    pub fn species(&self, name: &str) -> Result<PackSpecies, PackError> {
        let entry = self.manifest.entry(name).ok_or_else(|| {
            let names: Vec<&str> = self
                .manifest
                .species
                .iter()
                .map(|e| e.name.as_str())
                .collect();
            PackError::NotFound(format!(
                "pack `{}` has no species `{name}` (it has: {})",
                self.manifest.id,
                names.join(", ")
            ))
        })?;
        let path = entry.file_path();
        let invalid = |reason: String| PackError::Invalid {
            path: self.source.path().to_path_buf(),
            issues: vec![Issue::new(&path, reason)],
        };
        let bytes = self
            .files
            .get(&path)
            .ok_or_else(|| invalid("missing".to_owned()))?;
        let text = std::str::from_utf8(bytes).map_err(|_| invalid("not UTF-8".to_owned()))?;
        let text = migrate_species(text).map_err(invalid)?;
        let asset_dir = self.asset_dir()?;
        let thumbnail = self
            .files
            .contains_key(&entry.thumbnail_path())
            .then(|| {
                asset_dir
                    .as_ref()
                    .map(|dir| dir.join(entry.thumbnail_path()))
            })
            .flatten();
        Ok(PackSpecies {
            id: format!("{}/{name}", self.manifest.id),
            entry: entry.clone(),
            text,
            asset_dir,
            thumbnail,
        })
    }
}

/// Writes `bytes` to `path` through a temporary file in the same directory, creating
/// the directory, so that a reader never sees a partial file.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("no parent directory")?;
    std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        name.unwrap_or_default(),
        std::process::id()
    ));
    std::fs::write(&temporary, bytes).map_err(|e| format!("{}: {e}", temporary.display()))?;
    std::fs::rename(&temporary, path).map_err(|e| format!("{}: {e}", path.display()))
}

//! Species packs: species definitions and their assets, authored, validated, packaged,
//! installed and used by name.
//!
//! A pack is a directory, or the same files in a `.arbpack` zip archive:
//!
//! | File | Content |
//! |---|---|
//! | `pack.toml` | [`PackManifest`]: id, name, version, authors, content licence, minimum species schema, the species with their display names, categories, thumbnails and previews |
//! | licence file (`LICENCE-CONTENT.txt`) | the terms of the content, named by `licence_file` |
//! | `species/<name>.toml` | one species file per species |
//! | `textures/` | optional PNG or JPEG images named by `bark.texture` or `foliage.texture` |
//! | `thumbnails/<name>.png` | thumbnails, rendered by [`write_thumbnails`] or when building |
//! | `previews/<name>.glb` | optional preview models |
//! | `MANIFEST.sha256` | SHA-256 of every other file ([`hash_manifest`]), required in an archive |
//!
//! Species of a pack are named `pack_id/name` everywhere a preset name is accepted.
//!
//! * [`Pack::open`] reads a directory or an archive and checks its structure: allowed
//!   files only, no executable content, safe paths, sizes, hashes, `pack.toml`.
//! * [`validate`] also parses every species, grows it at three ages of its range,
//!   decodes its images and thumbnails, and returns every problem found ([`Report`]).
//! * [`build_archive`] makes a deterministic archive: sorted entries, fixed timestamps,
//!   missing thumbnails rendered, the hash manifest added.
//! * [`PackStore`] is a directory of installed packs, by default [`packs_dir`]
//!   (overridden by the environment variable [`PACKS_DIR_ENV`]).
//! * [`new_pack`] and [`add_species`] start a pack and add species to it.
//!
//! The format is versioned ([`FORMAT_VERSION`]); older `pack.toml` files go through the
//! migrations of [`manifest`] before they are read. The guide for pack authors is
//! `docs/packs.md`.

#![forbid(unsafe_code)]

mod archive;
mod author;
mod bundle;
mod hashes;
pub mod manifest;
mod pack;
mod paths;
mod store;
mod thumbs;
mod validate;

pub use archive::{read_archive, write_archive};
pub use author::{add_species, new_pack};
pub use bundle::{build_archive, build_files};
pub use hashes::{hash_manifest, verify_hashes};
pub use manifest::{FORMAT_VERSION, PackManifest, SpeciesEntry};
pub use pack::{Pack, PackSource, PackSpecies, split_species_id};
pub use paths::check_path;
pub use store::{InstallOutcome, PACKS_DIR_ENV, PackStore, packs_dir};
pub use thumbs::{THUMBNAIL_SIZE, render_thumbnail, write_thumbnails};
pub use validate::{Issue, Report, validate, validate_path};

use std::path::PathBuf;
use thiserror::Error;

/// Name of the pack description file, at the root of a pack.
pub const PACK_FILE: &str = "pack.toml";
/// Name of the hash manifest, at the root of a pack.
pub const HASHES_FILE: &str = "MANIFEST.sha256";
/// Extension of pack archives.
pub const EXTENSION: &str = "arbpack";
/// Largest file of a pack, in bytes: the size of an uncompressed RGBA image at the
/// largest texture resolution.
pub const MAX_FILE_BYTES: u64 = 4 * (arbrume_core::species::MAX_TEXTURE_RESOLUTION as u64).pow(2);
/// Largest total size of the files of a pack, in bytes, which bounds what reading an
/// archive may allocate.
pub const MAX_PACK_BYTES: u64 = 1 << 30;

/// Error of the pack functions.
#[derive(Debug, Error)]
pub enum PackError {
    /// A file or directory cannot be read or written.
    #[error("{}: {reason}", path.display())]
    Io {
        /// The file or directory.
        path: PathBuf,
        /// The error of the operating system.
        reason: String,
    },
    /// The archive is not a valid pack archive.
    #[error("invalid pack archive {}: {reason}", path.display())]
    Archive {
        /// The archive.
        path: PathBuf,
        /// What is wrong.
        reason: String,
    },
    /// The pack breaks the rules of the format; every problem is listed.
    #[error("invalid pack {}:\n{}", path.display(), list(issues))]
    Invalid {
        /// The pack directory or archive.
        path: PathBuf,
        /// The problems found.
        issues: Vec<Issue>,
    },
    /// No such pack, or no such species in a pack.
    #[error("{0}")]
    NotFound(String),
    /// A request that cannot be carried out, such as adding a species twice.
    #[error("{0}")]
    Usage(String),
}

/// One issue per line, indented.
fn list(issues: &[Issue]) -> String {
    let lines: Vec<String> = issues.iter().map(|issue| format!("  {issue}")).collect();
    lines.join("\n")
}

impl PackError {
    /// An [`PackError::Io`] error for `path`.
    pub(crate) fn io(path: impl Into<PathBuf>, error: &std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            reason: error.to_string(),
        }
    }
}

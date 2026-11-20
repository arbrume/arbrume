//! Building the files of a pack for distribution: validated, thumbnails complete, hashes
//! listed. Both `arbrume pack build` and installing go through [`build_files`].

use std::collections::BTreeMap;

use crate::hashes::hash_manifest;
use crate::pack::Pack;
use crate::thumbs::{THUMBNAIL_SIZE, pack_thumbnail};
use crate::validate::validate;
use crate::{HASHES_FILE, PackError};

/// The files of `pack` as they are distributed: the pack must validate; each species
/// without a thumbnail gets one rendered at [`THUMBNAIL_SIZE`] pixels at its default
/// path, and `MANIFEST.sha256` lists the hashes of every other file.
///
/// # Errors
///
/// Returns [`PackError::Invalid`] with the errors of [`validate`], and the errors of
/// rendering a thumbnail.
pub fn build_files(pack: &Pack) -> Result<BTreeMap<String, Vec<u8>>, PackError> {
    let report = validate(pack);
    if !report.is_ok() {
        return Err(PackError::Invalid {
            path: pack.source().path().to_path_buf(),
            issues: report.errors,
        });
    }
    let mut files = pack.files().clone();
    for entry in &pack.manifest().species {
        let path = entry.thumbnail_path();
        if let std::collections::btree_map::Entry::Vacant(slot) = files.entry(path) {
            slot.insert(pack_thumbnail(pack, &entry.name, THUMBNAIL_SIZE)?);
        }
    }
    files.remove(HASHES_FILE);
    let hashes = hash_manifest(&files);
    files.insert(HASHES_FILE.to_owned(), hashes.into_bytes());
    Ok(files)
}

/// The `.arbpack` archive of `pack`: the files of [`build_files`] written by
/// `write_archive`, the same bytes for the same pack.
///
/// # Errors
///
/// The errors of [`build_files`], and [`PackError::Usage`] for files too many or too
/// large for an archive.
pub fn build_archive(pack: &Pack) -> Result<Vec<u8>, PackError> {
    let files = build_files(pack)?;
    crate::archive::write_archive(&files).map_err(PackError::Usage)
}

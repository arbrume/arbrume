//! Thumbnails of the species of a pack, rendered with `arbrume-raster`.
//!
//! A thumbnail is the plant of seed 1 at the oldest age of `variation.age`, meshed at
//! the quality of random plants (`arbrume_gen::RANDOM_QUALITY`) with its textures
//! (image textures of the pack included), rendered by `Renderer::thumbnail` with its
//! default view and light, [`THUMBNAIL_SIZE`] pixels square unless asked otherwise,
//! and encoded as PNG. Rendering is deterministic, so a built archive is too.

use std::path::Path;

use arbrume_core::species::Species;
use arbrume_gen::{GenParams, RANDOM_QUALITY, generate};
use arbrume_raster::{Renderer, ThumbnailOptions};
use arbrume_texture::textures_with_images;
use arbrume_texture::{TextureImages, TextureOptions, decode_image, textured_species};

use crate::pack::{Pack, write_atomic};
use crate::{PackError, PackSource};

/// Default side of a thumbnail, in pixels.
pub const THUMBNAIL_SIZE: u32 = 256;

/// Renders the thumbnail of `species`, `size` pixels square, as PNG bytes; `images`
/// are its decoded image textures.
///
/// # Errors
///
/// Returns why the plant cannot be generated or the image encoded.
pub fn render_thumbnail(
    species: &Species,
    images: &TextureImages,
    size: u32,
) -> Result<Vec<u8>, String> {
    let textures = textures_with_images(species, &TextureOptions::default(), images);
    let meshed = textured_species(species);
    let params = GenParams::new(1, species.variation.age[1]).with_quality(RANDOM_QUALITY);
    let mut plant = generate(&meshed, &params).map_err(|e| e.to_string())?;
    textures.apply(&mut plant.mesh, species);
    let options = ThumbnailOptions {
        width: size,
        height: size,
        ..ThumbnailOptions::default()
    };
    let image = Renderer::new().thumbnail(&plant.mesh, &options);
    image.to_png().map_err(|e| e.to_string())
}

/// The decoded image textures of species `name` of `pack`.
pub(crate) fn pack_images(pack: &Pack, species: &Species) -> Result<TextureImages, String> {
    let mut images = TextureImages::default();
    for (field, path) in species.texture_paths() {
        let bytes = pack
            .files()
            .get(path)
            .ok_or_else(|| format!("`{field}` names `{path}`, which is not in the pack"))?;
        let image = Some(decode_image(bytes).map_err(|e| format!("`{path}`: {e}"))?);
        match field {
            "bark.texture" => images.bark = image,
            "bark.normal_texture" => images.bark_normal = image,
            "foliage.texture" => images.leaf = image,
            _ => images.leaf_normal = image,
        }
    }
    Ok(images)
}

/// Renders the thumbnail of the species `name` of `pack`.
pub(crate) fn pack_thumbnail(pack: &Pack, name: &str, size: u32) -> Result<Vec<u8>, PackError> {
    let entry = pack.species(name)?;
    let species =
        Species::from_toml_str(&entry.text).map_err(|e| PackError::Usage(e.to_string()))?;
    let failed =
        |reason: String| PackError::Usage(format!("thumbnail of `{}`: {reason}", entry.id));
    let images = pack_images(pack, &species).map_err(failed)?;
    render_thumbnail(&species, &images, size).map_err(failed)
}

/// Renders the thumbnail of every species of the pack directory `dir` that has none
/// (every species with `force`), `size` pixels square (a power of two from 16 to 4096,
/// like image textures), and writes it to its thumbnail path. Returns the paths
/// written.
///
/// # Errors
///
/// Returns [`PackError::Usage`] when `dir` is not a pack directory, and the errors of
/// opening the pack, generating a plant or writing a file.
pub fn write_thumbnails(dir: &Path, size: u32, force: bool) -> Result<Vec<String>, PackError> {
    arbrume_texture::check_texture_size(size, size)
        .map_err(|reason| PackError::Usage(format!("thumbnail size: {reason}")))?;
    let pack = Pack::load(dir)?;
    if !matches!(pack.source(), PackSource::Directory(_)) {
        let message = format!("{} is not a pack directory", dir.display());
        return Err(PackError::Usage(message));
    }
    let mut written = Vec::new();
    for entry in &pack.manifest().species {
        let path = entry.thumbnail_path();
        if !force && pack.files().contains_key(&path) {
            continue;
        }
        let png = pack_thumbnail(&pack, &entry.name, size)?;
        let target = dir.join(&path);
        write_atomic(&target, &png).map_err(|reason| PackError::Io {
            path: target.clone(),
            reason,
        })?;
        written.push(path);
    }
    Ok(written)
}

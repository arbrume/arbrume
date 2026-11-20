//! Procedural textures of `arbrume gen` (`--textures`, see `docs/textures.md`).
//!
//! With `embed` or `files`, every plant carries the bark and leaf textures of its
//! species, synthesized once per species by a cache shared by the worker threads. With
//! `embed` the images are stored in each `.glb`; with `files` they are written once as
//! PNG files next to the `.glb` files, which reference them by relative URI, so that the
//! plants of a species share them. Each image is named after a hash of its bytes
//! (`arbrume_export::image_file_name`), written under a temporary name unique to the
//! thread and renamed, so concurrent writers of the same image never leave a partial
//! file.

use std::fs;
use std::path::Path;

use arbrume_core::mesh::TreeMesh;
use arbrume_export::{ImageStorage, image_file_name};
use arbrume_gen::TextureCache;
use clap::ValueEnum;
use serde::Deserialize;
use std::time::Duration;

/// What `--textures` asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextureChoice {
    /// No textures: flat colours, outline leaves.
    None,
    /// Textures inside each `.glb`.
    Embed,
    /// Textures as shared PNG files next to the `.glb` files.
    Files,
}

impl TextureChoice {
    /// The choice when neither the command line nor the config file makes one.
    pub const DEFAULT: Self = Self::Files;

    /// Whether plants carry textures.
    pub const fn textured(self) -> bool {
        !matches!(self, Self::None)
    }

    /// Where the writer puts the images.
    pub const fn storage(self) -> ImageStorage {
        match self {
            Self::Files => ImageStorage::External,
            Self::None | Self::Embed => ImageStorage::Embed,
        }
    }
}

/// Prints how many species were textured, the synthesis time and the size of their
/// images.
pub fn summary(cache: &TextureCache) {
    let entries = cache.entries();
    let time: Duration = entries.iter().map(|t| t.timings.total()).sum();
    let bytes: usize = entries.iter().map(|t| t.bytes()).sum();
    eprintln!(
        "textures: {} species, {:.0} ms of synthesis, {:.1} MiB of images",
        entries.len(),
        time.as_secs_f64() * 1e3,
        bytes as f64 / (1024.0 * 1024.0)
    );
}

/// Writes every texture image of `meshes` to `out`, under the name the `.glb` files
/// reference, unless a file of that name is already there. Returns the bytes written.
pub fn write_images<'a>(
    out: &Path,
    meshes: impl IntoIterator<Item = &'a TreeMesh>,
) -> Result<u64, String> {
    let mut written = 0;
    for mesh in meshes {
        for part in &mesh.parts {
            let material = &part.material;
            let images = [
                &material.base_color_texture,
                &material.normal_texture,
                &material.roughness_texture,
            ];
            for bytes in images.into_iter().flatten() {
                written += write_image(out, bytes)?;
            }
        }
    }
    Ok(written)
}

/// Writes one image unless it exists; returns the bytes written.
fn write_image(out: &Path, bytes: &[u8]) -> Result<u64, String> {
    let name = image_file_name(bytes);
    let path = out.join(&name);
    if path.exists() {
        return Ok(0);
    }
    let thread = format!("{:?}", std::thread::current().id());
    let tag: String = thread.chars().filter(char::is_ascii_digit).collect();
    let temp = out.join(format!("{name}.{}.{tag}.tmp", std::process::id()));
    fs::write(&temp, bytes).map_err(|e| format!("{}: {e}", temp.display()))?;
    fs::rename(&temp, &path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(bytes.len() as u64)
}

//! Procedural textures of a species: tileable bark maps and a leaf atlas, synthesized
//! on the CPU from the species parameters alone, with no image asset.
//!
//! * [`bark_maps`]: albedo, normal and roughness of the bark, tileable around and along
//!   the axis, from `species.bark.pattern` (module `bark`).
//! * [`leaf_maps`]: an alpha-masked albedo and a normal map holding a few variants of
//!   the leaf, drawn over the rectangle of the leaf template with veins
//!   (`foliage.venation`) and a margin (`foliage.margin`), and for a conifer twig the
//!   needle spray of its fins (modules `atlas`, `leaf`, `venation`, `open`, `spray`).
//! * [`synthesize`] makes both and encodes them as PNG ([`SpeciesTextures`]);
//!   [`SpeciesTextures::apply`] puts them on the materials of a mesh.
//! * [`TextureCache`] synthesizes the textures of each species once per batch and
//!   shares them between threads.
//! * [`textured_species`] is the species as it is meshed for textured leaves: outline
//!   blades become cards, whose rectangle the alpha of the texture cuts to the leaf.
//!
//! **Packing.** Bark: an sRGB RGB albedo, an RGB tangent-space normal map and a grey
//! roughness map, read as the green channel of the glTF metallic-roughness texture (the
//! metallic factor is 0). Leaves: an sRGB RGBA albedo whose alpha is the coverage, cut
//! at 0.5, and an RGB normal map. Normals follow the convention of
//! glTF: `+x` towards increasing `u`, `+y` towards decreasing `v` (up the image), as the
//! tangents of `arbrume-export` and the normal mapping of `arbrume-raster` read them.
//!
//! **Colour.** A texture holds the variation of the colour around its mean, scaled
//! into `[0, 1]` (see `bark` and `atlas`), and records that mean. [`SpeciesTextures::apply`]
//! sets the base colour factor of the material to its colour divided by the mean, so
//! the vertex colours (young bark, shade of each leaf) keep their meaning, a textured
//! plant has the mean colour of the untextured one, and individuals that differ from
//! their species in colour (`Species::varied`) share its textures.
//!
//! **Shared textures.** Individuals varied from a species also differ in leaf width and
//! twig length, which move the cells of their leaf atlas; `apply` maps the texture
//! coordinates of their leaves from their own atlas to the one of the textures, so a
//! batch synthesizes one texture set per species.
//!
//! **Determinism.** A texture is a pure function of the species, the seed of
//! [`TextureOptions`] and the resolution: every random number is a hash of integer
//! coordinates (see `noise`), and the parallel loops write disjoint rows or cells, so
//! the bytes do not depend on the thread count.
//!
//! ```
//! use arbrume_core::species::Species;
//! use arbrume_texture::{TextureOptions, synthesize};
//!
//! let mut species = Species::default();
//! species.bark.texture_resolution = 64;
//! species.foliage.texture_resolution = 64;
//! let textures = synthesize(&species, &TextureOptions::default());
//! assert!(textures.bark.is_some() && textures.leaf.is_some());
//! ```

#![forbid(unsafe_code)]

mod atlas;
mod bark;
mod image;
mod leaf;
mod noise;
mod open;
mod spray;
mod surface;
mod venation;

pub use atlas::{LeafMaps, leaf_maps};
pub use bark::{BarkMaps, bark_maps};
pub use image::Image;

use arbrume_core::mesh::{PartKind, TreeMesh};
use arbrume_core::rng::mix64;
use arbrume_core::species::{BladeShape, LeafAtlas, LeafGeometry, Species};
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

/// Alpha cutoff of textured leaves.
pub const LEAF_ALPHA_CUTOFF: f32 = 0.5;

/// Options of the synthesis, shared by a batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextureOptions {
    /// Seed of the textures, combined with the name of the species.
    pub seed: u64,
    /// Largest resolution in texels; a species asking for more is synthesized at this
    /// resolution. A power of two.
    pub max_resolution: u32,
}

impl Default for TextureOptions {
    /// Seed 0, no limit below the largest resolution a species may ask for.
    fn default() -> Self {
        Self {
            seed: 0,
            max_resolution: arbrume_core::species::MAX_TEXTURE_RESOLUTION,
        }
    }
}

/// The encoded textures of one material.
#[derive(Debug, Clone, PartialEq)]
pub struct MaterialTextures {
    /// PNG of the base colour (RGB for bark, RGBA with coverage for leaves).
    pub base_color: Vec<u8>,
    /// PNG of the tangent-space normals.
    pub normal: Vec<u8>,
    /// PNG of the roughness (grey), for bark.
    pub roughness: Option<Vec<u8>>,
    /// Width and height in texels.
    pub resolution: u32,
    /// Mean linear colour of the base colour image, which the base colour factor
    /// divides.
    pub mean: [f32; 3],
}

impl MaterialTextures {
    /// Total size of the encoded images, in bytes.
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.base_color.len() + self.normal.len() + self.roughness.as_ref().map_or(0, Vec::len)
    }
}

/// Wall-clock time of each step of [`synthesize`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SynthesisTimings {
    /// Synthesis of the bark maps.
    pub bark: Duration,
    /// Synthesis of the leaf atlas.
    pub leaf: Duration,
    /// PNG encoding of every map.
    pub encode: Duration,
}

impl SynthesisTimings {
    /// Sum of the steps.
    #[must_use]
    pub fn total(&self) -> Duration {
        self.bark + self.leaf + self.encode
    }
}

/// The textures of one species.
#[derive(Debug, Clone, PartialEq)]
pub struct SpeciesTextures {
    /// Bark maps.
    pub bark: Option<MaterialTextures>,
    /// Leaf atlas; `None` for a species without leaves.
    pub leaf: Option<MaterialTextures>,
    /// Layout of the leaf atlas.
    pub atlas: Option<LeafAtlas>,
    /// How long the synthesis took.
    pub timings: SynthesisTimings,
}

/// Resolution of a species texture asking for `wanted` texels under `options`.
fn resolution(wanted: u32, options: &TextureOptions) -> u32 {
    wanted.min(options.max_resolution.max(16)).max(16)
}

/// FNV-1a hash of `text`.
fn fnv(text: &str) -> u64 {
    text.bytes().fold(0xCBF2_9CE4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01B3)
    })
}

/// Synthesizes and encodes the bark maps and the leaf atlas of `species`.
#[must_use]
pub fn synthesize(species: &Species, options: &TextureOptions) -> SpeciesTextures {
    let seed = mix64(options.seed ^ fnv(&species.name));
    let mut timings = SynthesisTimings::default();
    let start = Instant::now();
    let bark_size = resolution(species.bark.texture_resolution, options);
    let maps = bark_maps(&species.bark, bark_size, seed);
    timings.bark = start.elapsed();
    let start = Instant::now();
    let bark = MaterialTextures {
        base_color: maps.albedo.to_png(),
        normal: maps.normal.to_png(),
        roughness: Some(maps.roughness.to_png()),
        resolution: bark_size,
        mean: maps.mean,
    };
    timings.encode = start.elapsed();
    let mut atlas = None;
    let leaf = species.foliage.enabled.then(|| {
        let start = Instant::now();
        let size = resolution(species.foliage.texture_resolution, options);
        let maps = leaf_maps(&species.foliage, &species.bark, size, mix64(seed));
        timings.leaf = start.elapsed();
        let start = Instant::now();
        atlas = Some(maps.atlas);
        let encoded = MaterialTextures {
            base_color: maps.albedo.to_png(),
            normal: maps.normal.to_png(),
            roughness: None,
            resolution: size,
            mean: maps.mean,
        };
        timings.encode += start.elapsed();
        encoded
    });
    SpeciesTextures {
        bark: Some(bark),
        leaf,
        atlas,
        timings,
    }
}

impl SpeciesTextures {
    /// Total size of the encoded images, in bytes.
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.bark.as_ref().map_or(0, MaterialTextures::bytes)
            + self.leaf.as_ref().map_or(0, MaterialTextures::bytes)
    }

    /// Puts the textures on the bark and leaf parts of `mesh`, meshed from
    /// [`textured_species`] of `species`: the species the textures were made for or an
    /// individual varied from it. The base colour factor of each material becomes its
    /// colour divided by the mean of its texture (at most 1), the roughness factor of the
    /// bark 1 (the roughness map carries it), leaves are alpha-masked at
    /// [`LEAF_ALPHA_CUTOFF`] and their texture coordinates are moved from the atlas of
    /// `species` to the one of the textures ([`SpeciesTextures::remap`]). Other parts
    /// are left as they are.
    pub fn apply(&self, mesh: &mut TreeMesh, species: &Species) {
        for part in &mut mesh.parts {
            let textures = match part.kind {
                PartKind::Bark => self.bark.as_ref(),
                PartKind::Leaf => self.leaf.as_ref(),
                PartKind::Billboard => None,
            };
            let Some(textures) = textures else {
                continue;
            };
            if part.kind == PartKind::Leaf {
                self.remap(species, &mut part.buffers.uvs);
            }
            let material = &mut part.material;
            for (factor, mean) in material.base_color.iter_mut().zip(textures.mean) {
                *factor = (*factor / mean.max(1e-6)).min(1.0);
            }
            material.base_color_texture = Some(textures.base_color.clone());
            material.normal_texture = Some(textures.normal.clone());
            material.roughness_texture.clone_from(&textures.roughness);
            if textures.roughness.is_some() {
                material.roughness = 1.0;
            }
            if part.kind == PartKind::Leaf {
                material.alpha_cutoff = Some(LEAF_ALPHA_CUTOFF);
            }
        }
    }

    /// Moves leaf texture coordinates `uvs` from the atlas of `species` to the atlas of
    /// the textures: each point keeps its place within its cell. Nothing changes when
    /// both atlases are the same.
    pub fn remap(&self, species: &Species, uvs: &mut [[f32; 2]]) {
        let (Some(to), from) = (self.atlas, species.foliage.atlas()) else {
            return;
        };
        if to == from {
            return;
        }
        let cells = from.leaf_cells + u32::from(from.spray);
        let rects: Vec<[f32; 4]> = (0..cells).map(|cell| from.cell_rect(cell)).collect();
        for uv in uvs {
            let inside = |r: &[f32; 4]| {
                let (pad_u, pad_v) = (0.5 * (r[2] - r[0]) + 1e-4, 0.5 * (r[3] - r[1]) + 1e-4);
                let centre = [f32::midpoint(r[0], r[2]), f32::midpoint(r[1], r[3])];
                (uv[0] - centre[0]).abs() <= pad_u && (uv[1] - centre[1]).abs() <= pad_v
            };
            let Some(cell) = rects.iter().position(inside) else {
                continue;
            };
            let r = rects[cell];
            let local = [
                (uv[0] - r[0]) / (r[2] - r[0]),
                (uv[1] - r[1]) / (r[3] - r[1]),
            ];
            let target = match from.spray_cell() {
                Some(spray) if spray == cell as u32 => to.spray_cell(),
                _ => Some(cell as u32 % to.leaf_cells),
            };
            if let Some(target) = target {
                *uv = to.map(target, local);
            }
        }
    }
}

/// The species as it is meshed for textured leaves: outline leaves (other than needles)
/// become cards, folded and bent like the blade (see `arbrume-mesh`), which the alpha
/// of the leaf texture cuts to the outline and margin. Needles keep their triangle,
/// which their opaque texture covers. Borrowed when nothing changes.
#[must_use]
pub fn textured_species(species: &Species) -> Cow<'_, Species> {
    let foliage = &species.foliage;
    if foliage.geometry == LeafGeometry::Outline && foliage.shape != BladeShape::Needle {
        let mut owned = species.clone();
        owned.foliage.geometry = LeafGeometry::Card;
        Cow::Owned(owned)
    } else {
        Cow::Borrowed(species)
    }
}

/// Textures of each species, synthesized once and shared between the threads of a
/// batch.
///
/// Species are told apart by their name and their bark and foliage parameters, so a
/// species changed by overrides gets its own textures. When several threads ask for
/// the same species at once, one synthesizes and the others wait for it.
#[derive(Debug, Default)]
pub struct TextureCache {
    options: TextureOptions,
    entries: Mutex<HashMap<String, Arc<OnceLock<Arc<SpeciesTextures>>>>>,
}

impl TextureCache {
    /// An empty cache synthesizing with `options`.
    #[must_use]
    pub fn new(options: TextureOptions) -> Self {
        Self {
            options,
            entries: Mutex::new(HashMap::new()),
        }
    }

    /// The options of the cache.
    #[must_use]
    pub const fn options(&self) -> &TextureOptions {
        &self.options
    }

    /// The textures of `species`, synthesized on first use.
    pub fn get(&self, species: &Species) -> Arc<SpeciesTextures> {
        let key = format!(
            "{}\u{0}{:?}\u{0}{:?}",
            species.name, species.bark, species.foliage
        );
        let slot = {
            let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
            Arc::clone(entries.entry(key).or_default())
        };
        Arc::clone(slot.get_or_init(|| Arc::new(synthesize(species, &self.options))))
    }

    /// Number of species asked for so far.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    /// Whether no species was asked for yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Every species synthesized so far, with its textures, in no particular order.
    pub fn entries(&self) -> Vec<Arc<SpeciesTextures>> {
        let entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        entries
            .values()
            .filter_map(|slot| slot.get().cloned())
            .collect()
    }
}

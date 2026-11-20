//! The variant library: a few trees of each species, grown and meshed once, that every
//! placement of the forest draws.
//!
//! Variant `k` of a species is the individual `species.varied(seed, k)` grown with the
//! seed `plant_seed(seed, k)`, then meshed as a chain of levels of detail
//! (`arbrume_gen::Generator::lod_chain`). With textures, every variant of a species shares
//! the texture set of the species. Variants are built in parallel on the `rayon` pool,
//! one generator each; the library does not depend on the number of threads.

use std::sync::Arc;
use std::time::{Duration, Instant};

use arbrume_core::graph::{NodeId, PlantGraph};
use arbrume_core::species::Species;
use arbrume_gen::{GenParams, Generator, LodMesh, LodSettings, TextureCache, TextureOptions};
use arbrume_gen::{Plant, plant_seed};
use rayon::prelude::*;

use crate::error::ForestError;

/// What the library builds for every variant.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LibrarySettings {
    /// The levels of detail.
    pub lod: LodSettings,
    /// Procedural or pack textures on every level.
    pub textures: bool,
    /// Mesh quality of level 0 (`GenParams::with_quality`).
    pub quality: f32,
}

/// The variants to build for one species.
#[derive(Debug, Clone, PartialEq)]
pub struct VariantSpec {
    /// Name of the species in the forest.
    pub name: String,
    /// The species.
    pub species: Species,
    /// Age of the variants, in growth cycles.
    pub age: u16,
    /// Number of variants.
    pub count: u16,
    /// Seed of the variants of the species.
    pub seed: u64,
}

/// One grown and meshed variant.
#[derive(Debug, Clone)]
pub struct Variant {
    /// Growth seed.
    pub seed: u64,
    /// The individual of the species the variant is grown from (`Species::varied`).
    pub individual: Species,
    /// The plant (graph and level-0 mesh) at scale 1.
    pub plant: Plant,
    /// Its levels of detail, the billboard last when asked.
    pub levels: Vec<LodMesh>,
    /// Half the mean horizontal extent of level 0, in meters.
    pub crown_radius: f32,
    /// Largest radius of a first internode of a root axis, in meters.
    pub trunk_radius: f32,
    /// Height of level 0, in meters.
    pub height: f32,
    /// Time spent growing and meshing it.
    pub time: Duration,
}

/// The variants of one species.
#[derive(Debug, Clone)]
pub struct SpeciesVariants {
    /// Name of the species in the forest.
    pub name: String,
    /// The species the variants are varied from.
    pub species: Species,
    /// The variants, by index.
    pub variants: Vec<Variant>,
}

impl SpeciesVariants {
    /// Mean crown radius of the variants.
    #[must_use]
    pub fn crown_radius(&self) -> f32 {
        mean(self.variants.iter().map(|v| v.crown_radius))
    }

    /// Mean trunk radius of the variants.
    #[must_use]
    pub fn trunk_radius(&self) -> f32 {
        mean(self.variants.iter().map(|v| v.trunk_radius))
    }
}

/// Mean of `values`, 0 when empty.
fn mean(values: impl Iterator<Item = f32>) -> f32 {
    let (sum, count) = values.fold((0.0, 0), |(s, n), v| (s + v, n + 1));
    if count == 0 { 0.0 } else { sum / count as f32 }
}

/// Builds the variants of every species of `specs`.
///
/// # Errors
/// The errors of the generator for any variant.
pub fn build_library(
    specs: &[VariantSpec],
    settings: &LibrarySettings,
) -> Result<Vec<SpeciesVariants>, ForestError> {
    let cache = settings
        .textures
        .then(|| Arc::new(TextureCache::new(TextureOptions::default())));
    let bases: Vec<Arc<Species>> = specs.iter().map(|s| Arc::new(s.species.clone())).collect();
    let jobs: Vec<(usize, u16)> = specs
        .iter()
        .enumerate()
        .flat_map(|(s, spec)| (0..spec.count).map(move |k| (s, k)))
        .collect();
    let built: Vec<Result<Variant, ForestError>> = jobs
        .par_iter()
        .map(|&(s, k)| {
            let mut generator = Generator::new();
            generator.set_textures(cache.clone());
            generator.set_texture_base(Some(Arc::clone(&bases[s])));
            build_variant(&mut generator, &specs[s], k, settings)
        })
        .collect();
    let mut library: Vec<SpeciesVariants> = specs
        .iter()
        .map(|spec| SpeciesVariants {
            name: spec.name.clone(),
            species: spec.species.clone(),
            variants: Vec::with_capacity(usize::from(spec.count)),
        })
        .collect();
    for ((s, _), variant) in jobs.into_iter().zip(built) {
        library[s].variants.push(variant?);
    }
    Ok(library)
}

/// Grows and meshes variant `k` of `spec`.
fn build_variant(
    generator: &mut Generator,
    spec: &VariantSpec,
    k: u16,
    settings: &LibrarySettings,
) -> Result<Variant, ForestError> {
    let start = Instant::now();
    let seed = plant_seed(spec.seed, u32::from(k));
    let individual = spec.species.varied(spec.seed, u32::from(k));
    let params = GenParams::new(seed, spec.age).with_quality(settings.quality);
    let plant = generator.generate(&individual, &params)?;
    let levels = generator.lod_chain(&plant, &individual, &settings.lod)?;
    let size = plant.stats.bounds.map_or([0.0; 3], |b| b.size());
    Ok(Variant {
        seed,
        crown_radius: 0.25 * (size[0] + size[2]),
        trunk_radius: trunk_radius(&plant.graph),
        height: plant.stats.height,
        levels,
        plant,
        individual,
        time: start.elapsed(),
    })
}

/// Largest radius of the first internode of a root axis.
fn trunk_radius(graph: &PlantGraph) -> f32 {
    graph
        .parents()
        .iter()
        .zip(graph.radii())
        .filter(|(parent, _)| **parent == NodeId::NONE)
        .map(|(_, radius)| *radius)
        .fold(0.0, f32::max)
}

//! The whole pipeline of a config: terrain, exclusions, library, placement and coupled
//! region.

use std::path::Path;
use std::time::{Duration, Instant};

use crate::config::ForestConfig;
use crate::coupled::{Coupled, CoupledSettings, grow_coupled};
use crate::error::ForestError;
use crate::exclude::Exclusions;
use crate::library::{SpeciesVariants, VariantSpec, build_library};
use crate::place::{Instance, PlaceReport, place};
use crate::rules::SpeciesRule;
use crate::terrain::Terrain;

/// A placed forest and its variants.
#[derive(Debug, Clone)]
pub struct Forest {
    /// Name of the forest.
    pub name: String,
    /// Seed of the forest.
    pub seed: u64,
    /// The ground, carved.
    pub terrain: Terrain,
    /// The exclusions.
    pub exclusions: Exclusions,
    /// The variant specs, one per species.
    pub specs: Vec<VariantSpec>,
    /// The placement rule of each species.
    pub rules: Vec<SpeciesRule>,
    /// The variants of each species; empty when not grown.
    pub library: Vec<SpeciesVariants>,
    /// Every tree.
    pub instances: Vec<Instance>,
    /// What the placement did.
    pub placement: PlaceReport,
    /// Trees of the coupled region, grown together.
    pub coupled: Option<Coupled>,
    /// Time spent building the library.
    pub library_time: Duration,
}

impl Forest {
    /// Places the forest of `config`, whose relative paths are relative to `base`. With
    /// `grow`, the variant library is built first (and the spacings default to the crowns
    /// of the variants), and the coupled region, if any, is grown.
    ///
    /// # Errors
    /// The errors of the config, the terrain, the generator and the placement.
    pub fn build(config: &ForestConfig, base: &Path, grow: bool) -> Result<Self, ForestError> {
        if config.species.is_empty() {
            return Err(ForestError::Config("no species".to_owned()));
        }
        let exclusions = config.exclusions(base)?;
        let terrain = config.terrain(base, &exclusions)?;
        let species = config.load_species(base)?;
        let specs = config.variant_specs(&species);
        let start = Instant::now();
        let library = if grow {
            build_library(&specs, &config.library_settings())?
        } else {
            Vec::new()
        };
        let library_time = start.elapsed();
        let rules = (0..species.len())
            .map(|s| {
                let radii = library
                    .get(s)
                    .map(|v: &SpeciesVariants| (v.crown_radius(), v.trunk_radius()));
                config.rule(s, &species[s].1, radii)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let (instances, placement) =
            place(&terrain, &exclusions, &rules, &config.place_settings())?;
        let coupled = match (&config.coupled, grow) {
            (Some(c), true) => {
                let settings = CoupledSettings {
                    region: c.region,
                    tile: c.tile,
                    halo: c.halo,
                    quality: c.quality,
                };
                Some(grow_coupled(&instances, &specs, &settings)?)
            }
            _ => None,
        };
        Ok(Self {
            name: config.name.clone(),
            seed: config.seed,
            terrain,
            exclusions,
            specs,
            rules,
            library,
            instances,
            placement,
            coupled,
            library_time,
        })
    }

    /// `true` for the instances grown in the coupled region, by instance index.
    #[must_use]
    pub fn coupled_mask(&self) -> Vec<bool> {
        let mut mask = vec![false; self.instances.len()];
        if let Some(coupled) = &self.coupled {
            for tree in &coupled.trees {
                mask[tree.instance] = true;
            }
        }
        mask
    }
}

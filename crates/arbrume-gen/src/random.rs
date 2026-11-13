//! Random plants: a varied individual of a random species for each index.
//!
//! A [`RandomSampler`] holds a base seed and a list of candidate species, by default
//! every built-in preset. Plant `index` is drawn in four steps, each with its own stream
//! of the counter-based generator of `arbrume-core`, keyed by `(seed, index)`:
//!
//! 1. a species, with a probability proportional to its `variation.weight`;
//! 2. an age, uniform over the integers of its `variation.age`, and a scale, uniform
//!    over its `variation.scale`;
//! 3. an individual of the species, `Species::varied`, whose jittered parameters stray
//!    from the preset within the ranges of its `[variation.jitter]` table;
//! 4. the plant seed [`plant_seed`]`(seed, index)`, which also seeds the mesh.
//!
//! A [`RandomPick`] is therefore a pure function of the sampler and the index: plants
//! can be generated in any order and on any number of threads.
//!
//! # Budget
//!
//! Random plants range from a herb to an old tree, and one mesh quality does not suit
//! both. The sampler carries a quality and a [`Budget`] of triangles and vertices;
//! [`RandomPick::generate`] meshes at that quality unless the mesh would exceed the
//! budget, in which case the quality is lowered until it fits
//! ([`Generator::generate_within`]). Small plants keep their detail and large ones stay
//! light. The quality used is recorded in the parameters of the plant.

use arbrume_core::rng::{Rng, Stream};
use arbrume_core::species::Species;

use crate::batch::{Budget, Generator};
use crate::params::{GenError, GenParams};
use crate::plant::{Plant, Timings};
use crate::presets::presets;
use crate::sample::plant_seed;

/// Stream of the choice of species.
const SPECIES_STREAM: Stream = Stream::custom(3);
/// Stream of the age and scale draws.
const AGE_STREAM: Stream = Stream::custom(4);

/// Default [`RandomSampler::quality`]. A starting point, not a measured value.
pub const RANDOM_QUALITY: f32 = 0.6;
/// Default [`RandomSampler::budget`]: 60 000 triangles and 55 000 vertices, which keeps
/// an exported `.glb` near 3 MB at most. A starting point, not a measured value.
pub const RANDOM_BUDGET: Budget = Budget {
    triangles: Some(60_000),
    vertices: Some(55_000),
};

/// Draws random plants: for each index a species chosen by its `variation.weight`, an
/// age and a scale within its `[variation]` ranges, a varied individual
/// (`Species::varied`) and a plant seed, all pure functions of `(seed, index)`. Plants
/// are meshed at `quality`, or lower when that would exceed `budget`.
#[derive(Debug, Clone, PartialEq)]
pub struct RandomSampler {
    /// Base seed.
    pub seed: u64,
    /// Mesh quality of plants that fit the triangle budget, as in
    /// `MeshOptions::with_quality`.
    pub quality: f32,
    /// Largest mesh of a plant.
    pub budget: Budget,
    species: Vec<(String, Species)>,
}

/// One plant drawn by a [`RandomSampler`].
#[derive(Debug, Clone, PartialEq)]
pub struct RandomPick {
    /// Name of the species the plant was drawn from.
    pub preset: String,
    /// The individual: the species with its jittered parameters.
    pub species: Species,
    /// Seed, age, scale and mesh options; the mesh quality is that of the sampler.
    pub params: GenParams,
    /// Budget of the sampler.
    pub budget: Budget,
}

impl RandomSampler {
    /// A sampler over every built-in preset, with [`RANDOM_QUALITY`] and
    /// [`RANDOM_BUDGET`].
    ///
    /// # Panics
    ///
    /// Panics if a built-in preset is invalid, which the tests of this crate rule out.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        let species = presets()
            .iter()
            .map(|(name, text)| {
                let species = Species::from_toml_str(text)
                    .unwrap_or_else(|error| panic!("built-in preset `{name}` is invalid: {error}"));
                ((*name).to_owned(), species)
            })
            .collect();
        Self {
            seed,
            quality: RANDOM_QUALITY,
            budget: RANDOM_BUDGET,
            species,
        }
    }

    /// The same sampler drawing from `species` (name and species) instead.
    #[must_use]
    pub fn with_species(mut self, species: Vec<(String, Species)>) -> Self {
        self.species = species;
        self
    }

    /// The candidate species, with their names.
    #[must_use]
    pub fn species(&self) -> &[(String, Species)] {
        &self.species
    }

    /// Checks the quality, the budget and the candidates.
    ///
    /// # Errors
    ///
    /// Returns [`GenError::InvalidParam`] for a quality that is not positive and finite,
    /// a budget of 0 or candidates whose weights are all 0, and [`GenError::Species`]
    /// for an invalid candidate.
    pub fn validate(&self) -> Result<(), GenError> {
        let invalid = |field, reason: String| GenError::InvalidParam { field, reason };
        if !(self.quality > 0.0 && self.quality.is_finite()) {
            let reason = format!("{} is not positive and finite", self.quality);
            return Err(invalid("quality", reason));
        }
        if self.budget.triangles == Some(0) || self.budget.vertices == Some(0) {
            return Err(invalid("budget", "0 leaves no plant".to_owned()));
        }
        for (_, species) in &self.species {
            species.validate()?;
        }
        if self.total_weight() <= 0.0 {
            let reason = "no species has a positive variation.weight".to_owned();
            return Err(invalid("species", reason));
        }
        Ok(())
    }

    fn total_weight(&self) -> f64 {
        let weights = self.species.iter();
        weights.map(|(_, s)| f64::from(s.variation.weight)).sum()
    }

    /// Plant `index`.
    ///
    /// # Errors
    ///
    /// The errors of [`RandomSampler::validate`].
    pub fn pick(&self, index: u32) -> Result<RandomPick, GenError> {
        self.validate()?;
        let mut rng = Rng::new(self.seed, index, SPECIES_STREAM);
        let mut target = f64::from(rng.f32()) * self.total_weight();
        let mut chosen = 0;
        for (position, (_, species)) in self.species.iter().enumerate() {
            let weight = f64::from(species.variation.weight);
            if weight <= 0.0 {
                continue;
            }
            chosen = position;
            if target < weight {
                break;
            }
            target -= weight;
        }
        let (name, species) = &self.species[chosen];
        let variation = &species.variation;
        let mut rng = Rng::new(self.seed, index, AGE_STREAM);
        let [young, old] = variation.age;
        let age = young + rng.below(u32::from(old - young) + 1) as u16;
        let [small, large] = variation.scale;
        let scale = rng.range(small, large);
        let seed = plant_seed(self.seed, index);
        let params = GenParams::new(seed, age)
            .with_scale(scale)
            .with_quality(self.quality);
        Ok(RandomPick {
            preset: name.clone(),
            species: species.varied(self.seed, index),
            params,
            budget: self.budget,
        })
    }
}

impl RandomPick {
    /// Generates the plant into `plant`, within the budget
    /// ([`Generator::generate_within`]).
    ///
    /// # Errors
    ///
    /// The same as [`crate::generate`].
    pub fn generate(
        &self,
        generator: &mut Generator,
        plant: &mut Plant,
    ) -> Result<Timings, GenError> {
        generator.generate_within(&self.species, &self.params, &self.budget, plant)
    }
}

#[cfg(test)]
mod tests {
    use super::{RANDOM_BUDGET, RandomSampler};
    use crate::{GenError, Generator, Plant, preset};
    use std::collections::BTreeSet;

    #[test]
    fn picks_are_a_pure_function_of_seed_and_index() {
        let sampler = RandomSampler::new(5);
        let again = RandomSampler::new(5);
        let other = RandomSampler::new(6);
        let mut differing = 0;
        for index in 0..40 {
            let pick = sampler.pick(index).unwrap();
            assert_eq!(pick, again.pick(index).unwrap());
            differing += usize::from(pick != other.pick(index).unwrap());
            pick.species.validate().unwrap();
            let [young, old] = pick.species.variation.age;
            assert!((young..=old).contains(&pick.params.age));
            let [small, large] = pick.species.variation.scale;
            assert!((small..=large).contains(&pick.params.scale));
        }
        assert_eq!(differing, 40);
    }

    #[test]
    fn every_weighted_preset_is_drawn_and_individuals_differ() {
        let sampler = RandomSampler::new(1);
        let mut seen = BTreeSet::new();
        let mut individuals = Vec::new();
        for index in 0..600 {
            let pick = sampler.pick(index).unwrap();
            seen.insert(pick.preset.clone());
            if pick.preset == "oak_like" {
                individuals.push(pick.species);
            }
        }
        let weighted = sampler.species().iter();
        let expected: BTreeSet<String> = weighted
            .filter(|(_, species)| species.variation.weight > 0.0)
            .map(|(name, _)| name.clone())
            .collect();
        assert_eq!(seen, expected);
        assert!(individuals.len() > 3);
        assert_ne!(individuals[0], individuals[1]);
        assert_ne!(individuals[0], preset("oak_like").unwrap());
    }

    #[test]
    fn zero_weight_species_are_never_drawn_and_no_weight_is_an_error() {
        let mut shrub = preset("shrub").unwrap();
        let mut fern = preset("fern_like").unwrap();
        fern.variation.weight = 0.0;
        let candidates = vec![
            ("fern".to_owned(), fern.clone()),
            ("shrub".to_owned(), shrub.clone()),
        ];
        let sampler = RandomSampler::new(3).with_species(candidates);
        for index in 0..50 {
            assert_eq!(sampler.pick(index).unwrap().preset, "shrub");
        }
        shrub.variation.weight = 0.0;
        let none = RandomSampler::new(3).with_species(vec![("shrub".to_owned(), shrub)]);
        let invalid = |result| matches!(result, Err(GenError::InvalidParam { .. }));
        assert!(invalid(none.pick(0).map(|_| ())));
        assert!(invalid(
            RandomSampler::new(3).with_species(Vec::new()).validate()
        ));
        let mut bad = RandomSampler::new(3);
        bad.quality = 0.0;
        assert!(invalid(bad.validate()));
        bad.quality = 1.0;
        bad.budget.triangles = Some(0);
        assert!(invalid(bad.validate()));
    }

    #[test]
    fn generated_plants_fit_the_budget_and_record_their_quality() {
        let sampler = RandomSampler::new(2);
        let mut generator = Generator::new();
        let mut plant = Plant::default();
        let mut lowered = 0;
        for index in 0..24 {
            let pick = sampler.pick(index).unwrap();
            pick.generate(&mut generator, &mut plant).unwrap();
            assert!(
                plant.stats.triangles <= RANDOM_BUDGET.triangles.unwrap(),
                "{index}"
            );
            assert!(
                plant.stats.vertices <= RANDOM_BUDGET.vertices.unwrap(),
                "{index}"
            );
            assert!(plant.stats.triangles > 0);
            assert!(plant.params.mesh.tolerance >= pick.params.mesh.tolerance);
            lowered += usize::from(plant.params.mesh != pick.params.mesh);
        }
        assert!(lowered > 0, "some plant should need a lower quality");
    }
}

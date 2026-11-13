//! Growth sequences: one plant meshed at several ages.
//!
//! A plant is grown once, with the leaf sites of every metamer kept
//! (`arbrume_grow::grow_timeline`). Each frame then takes the plant as it stood at the
//! end of one growth cycle: the metamers born by then, the branches shed later put
//! back, the radii of the radius model on that structure, and the leaves the species
//! still retained (`arbrume_grow::GrowthRecord::at_cycle`, then
//! `MeshOptions::max_cycle`).
//!
//! The axes keep the position they have in the grown plant, so they do not move from
//! one frame to the next. With `growth.weight_bending` they carry at every age the sag
//! of the final weight.

use std::time::{Duration, Instant};

use arbrume_core::species::Species;
use arbrume_grow::{GrowthRecord, Surroundings, grow_timeline};
use arbrume_mesh::MeshOptions;

use crate::batch::Generator;
use crate::params::{GenError, GenParams};
use crate::plant::{Plant, Stats};

/// A grown plant that can be meshed at any of its ages. Made by
/// [`Generator::timeline`].
#[derive(Debug, Clone, PartialEq)]
pub struct Timeline {
    /// Parameters the plant was grown with.
    pub params: GenParams,
    /// The grown plant, with the leaf sites of every metamer.
    pub record: GrowthRecord,
    /// Time spent growing.
    pub grow: Duration,
}

impl Timeline {
    /// The cycles of a sequence of `frames` frames spread evenly over the growth, the
    /// last being the last cycle of the plant. Fewer when the plant has fewer cycles;
    /// none for a plant of age 0.
    #[must_use]
    pub fn frame_cycles(&self, frames: u16) -> Vec<u16> {
        let age = u32::from(self.params.age);
        let frames = u32::from(frames).min(age);
        (1..=frames)
            .map(|frame| ((frame * age).div_ceil(frames) - 1) as u16)
            .collect()
    }
}

impl Generator {
    /// Grows the plant of `params` once, ready to be meshed at any age with
    /// [`Generator::frame`].
    ///
    /// # Errors
    ///
    /// The same as [`crate::generate`], meshing errors apart.
    pub fn timeline(
        &mut self,
        species: &Species,
        params: &GenParams,
    ) -> Result<Timeline, GenError> {
        species.validate()?;
        params.validate()?;
        let start = Instant::now();
        let open = Surroundings::none();
        let surroundings = params.surroundings.as_deref().unwrap_or(&open);
        let record = grow_timeline(species, &params.grow_params(), surroundings);
        Ok(Timeline {
            params: params.clone(),
            record,
            grow: start.elapsed(),
        })
    }

    /// Writes into `plant` the plant of `timeline` as it stood at the end of growth
    /// cycle `cycle`, meshed with the options of the timeline. `plant.params.mesh`
    /// records `max_cycle`. Returns the time taken.
    ///
    /// # Errors
    ///
    /// Returns [`GenError::Mesh`] when the mesh would be too large; `plant` then keeps
    /// its previous mesh.
    pub fn frame(
        &mut self,
        species: &Species,
        timeline: &Timeline,
        cycle: u16,
        plant: &mut Plant,
    ) -> Result<Duration, GenError> {
        let start = Instant::now();
        let scale = timeline.params.scale;
        let graph = timeline.record.at_cycle(species, scale, cycle);
        let options = MeshOptions {
            max_cycle: Some(cycle),
            ..timeline.params.mesh
        };
        self.mesh_graph(&graph, species, &options, plant)?;
        plant.graph = graph;
        plant.params = GenParams {
            mesh: options,
            ..timeline.params.clone()
        };
        plant.stats = Stats::of(&plant.graph, &plant.mesh);
        Ok(start.elapsed())
    }
}

#[cfg(test)]
mod tests {
    use crate::{GenParams, Generator, Plant, preset};

    #[test]
    fn frame_cycles_end_on_the_last_cycle() {
        let species = preset("shrub").unwrap();
        let mut generator = Generator::new();
        let timeline = generator
            .timeline(&species, &GenParams::new(1, 10))
            .unwrap();
        assert_eq!(timeline.frame_cycles(5), [1, 3, 5, 7, 9]);
        assert_eq!(timeline.frame_cycles(3), [3, 6, 9]);
        assert_eq!(timeline.frame_cycles(40).len(), 10);
        assert_eq!(timeline.frame_cycles(1), [9]);
        assert_eq!(timeline.frame_cycles(0), [0_u16; 0]);
    }

    #[test]
    fn frames_grow_and_the_last_matches_the_plant() {
        for name in ["broadleaf", "conifer", "shrub"] {
            let species = preset(name).unwrap();
            let params = GenParams::new(4, 9);
            let mut generator = Generator::new();
            let timeline = generator.timeline(&species, &params).unwrap();
            let full = generator.generate(&species, &params).unwrap();
            let mut plant = Plant::default();
            let mut heights = Vec::new();
            for cycle in timeline.frame_cycles(3) {
                generator
                    .frame(&species, &timeline, cycle, &mut plant)
                    .unwrap();
                assert_eq!(plant.params.mesh.max_cycle, Some(cycle));
                assert!(plant.stats.triangles > 0, "{name} {cycle}");
                heights.push(plant.stats.height);
            }
            assert!(heights.windows(2).all(|pair| pair[0] <= pair[1]), "{name}");
            let (last, whole) = (&plant.mesh.parts[0], &full.mesh.parts[0]);
            assert_eq!(last.buffers.positions.len(), whole.buffers.positions.len());
        }
    }
}

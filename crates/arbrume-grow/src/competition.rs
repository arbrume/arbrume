//! The environment of the self-organizing mode: light, space and obstacles.
//!
//! [`Competition`] gives every live bud its exposure `Q` and preferred direction `V`
//! from the `sensing` of the species (Palubicki et al. 2009, section 4.1):
//!
//! * **shadow**: every metamer casts its pyramid in a [`ShadowGrid`] when it appears
//!   and takes it back when it is shed. `Q = max(C - s + a, 0)`, with the shade of the
//!   occluders of the [`Surroundings`] added to `s`, and `V` is the direction opposite
//!   to the gradient of `s`, the shade of the occluders included, so buds turn away
//!   from them;
//! * **markers**: `Q` is 1 for a bud that owns markers and 0 otherwise, `V` points to
//!   its markers ([`Markers`]). The markers fill the envelope of the surroundings, or a
//!   default envelope drawn from the species when there is none
//!   ([`default_envelope`]);
//! * **both**: `Q` of the shadow model for the buds that own markers, 0 for the others,
//!   and `V` of the markers.
//!
//! `Q` is finally raised to the `light_exponent` `kappa` of the species and multiplied by
//! the gravimorphism factor `b = cos² tau + d sin² tau` of Longay et al. (2012, eq. 1),
//! `tau` the angle between the vertical and the internode bearing the bud and `d` the
//! `gravimorphism` of the species: `Q = b L^kappa` ([`gravimorphism_factor`]). The
//! default `d = 1` gives `b = 1` exactly. Blocking is not sensed
//! here: the fate stage cuts the shoots that would leave the envelope or enter an
//! obstacle ([`crate::ResourceFate`]), and the buds held back keep their exposure, so
//! the branches pressed against a clip volume are not shed for lack of light.
//!
//! Lengths of the species are scaled by the scale of the run; the surroundings are in
//! meters and are not.
//!
//! # Threads
//!
//! Each bud is sensed from read-only data and written to its own slot, and each marker
//! is given to the smallest of the claims of the buds on it, a minimum that does not
//! depend on the order of the claims, the sums being made afterwards in a fixed order
//! ([`Markers`]). Both passes can therefore run on the `rayon` pool without changing any
//! result; [`EngineOptions`] says which do, and `docs/growth.md` has the measurements
//! behind its defaults.

use crate::markers::{Markers, Perception};
use crate::plant::Plant;
use crate::shadow::{GridStorage, ShadowGrid, ShadowParams};
use crate::stage::{Context, Environment};
use crate::surroundings::{Shape, Surroundings};
use arbrume_core::graph::{BudState, PlantGraph};
use arbrume_core::species::{Sensing, Species};
use glam::Vec3;
use rayon::prelude::*;

/// Buds per task of the parallel pass.
const CHUNK: usize = 256;

/// Engine choices of the self-organizing mode that do not change the plant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineOptions {
    /// Storage of the shadow grid.
    pub storage: GridStorage,
    /// Read the shadow grid for each bud on the current `rayon` pool instead of the
    /// calling thread.
    pub parallel_buds: bool,
    /// Give the markers to the buds on the current `rayon` pool instead of the calling
    /// thread.
    pub parallel_markers: bool,
}

impl Default for EngineOptions {
    /// A dense grid, buds sensed on the calling thread, markers on the pool: the
    /// fastest choices in the measurements of `docs/growth.md`.
    fn default() -> Self {
        Self {
            storage: GridStorage::Dense,
            parallel_buds: false,
            parallel_markers: true,
        }
    }
}

/// The shadow coefficients of `species` for a run at `scale`.
#[must_use]
pub fn shadow_params(species: &Species, scale: f32) -> ShadowParams {
    let growth = &species.growth;
    let model = &growth.self_organization;
    ShadowParams {
        cell: model.shadow_cell * growth.internode_unit() * scale,
        strength: model.shadow_strength,
        decay: model.shadow_decay,
        depth: model.shadow_depth,
        full: model.full_exposure,
    }
}

/// The envelope markers are scattered in when the surroundings give none: an ellipsoid
/// standing on the ground, as tall as the trunk can get and as wide as the longest
/// branch of order 1 on each side.
#[must_use]
pub fn default_envelope(species: &Species, scale: f32) -> Shape {
    let growth = &species.growth;
    let unit = growth.internode_unit();
    let height = (growth.axis_length.order(0).bounds().1 + unit) * scale;
    let reach = (growth.axis_length.order(1).bounds().1 + growth.stem_spread + unit) * scale;
    Shape::Ellipsoid {
        center: Vec3::new(0.0, 0.5 * height, 0.0),
        radii: Vec3::new(reach, 0.5 * height, reach),
    }
}

/// What a bud senses without the markers.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Sensed {
    exposure: f32,
    preferred: Vec3,
}

/// Environment of the self-organizing mode. See the [module documentation](self).
#[derive(Debug, Clone)]
pub struct Competition<'a> {
    surroundings: &'a Surroundings,
    /// Position of the plant in the frame of the surroundings and of the grid.
    origin: Vec3,
    shadow: Option<ShadowGrid>,
    /// Whether an occluder casts shade, which then also steers the buds.
    shading: bool,
    markers: Option<Markers>,
    perception: Perception,
    exponent: f32,
    /// Gravimorphism `d` of the species.
    gravimorphism: f32,
    options: EngineOptions,
    /// Whether the shadow of each node is in the grid.
    cast: Vec<bool>,
    buds: Vec<u32>,
    positions: Vec<Vec3>,
    directions: Vec<Vec3>,
    /// Gravimorphism factor `b` of each live bud.
    factors: Vec<f32>,
    sensed: Vec<Sensed>,
}

impl<'a> Competition<'a> {
    /// Environment of one run in `surroundings`.
    #[must_use]
    pub fn new(cx: &Context, surroundings: &'a Surroundings, options: EngineOptions) -> Self {
        let growth = &cx.species.growth;
        let model = &growth.self_organization;
        let unit = growth.internode_unit() * cx.scale;
        let shadow = (model.sensing != Sensing::Markers)
            .then(|| ShadowGrid::new(shadow_params(cx.species, cx.scale), options.storage));
        let markers = (model.sensing != Sensing::Shadow).then(|| {
            let fallback;
            let envelope = if let Some(envelope) = &surroundings.envelope {
                envelope
            } else {
                fallback = default_envelope(cx.species, cx.scale);
                &fallback
            };
            let density = model.marker_density / (unit * unit * unit);
            Markers::scatter(envelope, cx.seed, density, model.marker_limit)
        });
        Self {
            surroundings,
            origin: Vec3::ZERO,
            shadow,
            shading: surroundings.occluders.iter().any(|o| o.shade > 0.0),
            markers,
            perception: Perception {
                occupancy: model.occupancy_radius * unit,
                reach: model.perception_distance * unit,
                angle: model.perception_angle_deg.to_radians(),
            },
            exponent: model.light_exponent,
            gravimorphism: model.gravimorphism,
            options,
            cast: Vec::new(),
            buds: Vec::new(),
            positions: Vec::new(),
            directions: Vec::new(),
            factors: Vec::new(),
            sensed: Vec::new(),
        }
    }

    /// The same environment for a plant standing at `origin` in the frame of the
    /// surroundings.
    #[must_use]
    pub const fn at(mut self, origin: Vec3) -> Self {
        self.origin = origin;
        self
    }

    /// The shadow grid, when the species senses light.
    #[must_use]
    pub const fn shadow(&self) -> Option<&ShadowGrid> {
        self.shadow.as_ref()
    }

    /// Mutable access to the shadow grid slot, to lend one grid to several plants: swap
    /// the shared grid in before a cycle of this plant and out after it.
    pub const fn shadow_mut(&mut self) -> &mut Option<ShadowGrid> {
        &mut self.shadow
    }

    /// The markers left, when the species senses space.
    #[must_use]
    pub fn markers(&self) -> Option<&Markers> {
        self.markers.as_ref()
    }

    /// Casts the shadow of the nodes added since the last call and takes back that of
    /// the nodes shed since.
    fn update_shadow(&mut self, graph: &PlantGraph) {
        let Some(grid) = &mut self.shadow else {
            return;
        };
        let (positions, flags) = (graph.positions(), graph.flags());
        for (node, cast) in self.cast.iter_mut().enumerate() {
            if *cast && flags[node].is_shed() {
                grid.cast(positions[node] + self.origin, -1.0);
                *cast = false;
            }
        }
        for node in self.cast.len()..positions.len() {
            let live = !flags[node].is_shed();
            if live {
                grid.cast(positions[node] + self.origin, 1.0);
            }
            self.cast.push(live);
        }
    }

    /// Exposure and preferred direction of a bud at `position`, markers left aside.
    fn sense_bud(&self, position: Vec3) -> Sensed {
        let Some(grid) = &self.shadow else {
            return Sensed {
                exposure: 1.0,
                preferred: Vec3::ZERO,
            };
        };
        let cell = grid.params().cell;
        let shade = self.surroundings.shade(position, cell, grid.weights());
        let mut gradient = grid.gradient(position);
        if self.shading {
            gradient += self.shade_gradient(position, grid);
        }
        Sensed {
            exposure: grid.exposure(position, shade),
            preferred: (-gradient).normalize_or_zero(),
        }
    }

    /// Gradient of the shade of the occluders at `position`: the central difference
    /// over one voxel along each axis, in the units of [`ShadowGrid::gradient`].
    fn shade_gradient(&self, position: Vec3, grid: &ShadowGrid) -> Vec3 {
        let cell = grid.params().cell;
        let shade = |offset: Vec3| {
            self.surroundings
                .shade(position + offset * cell, cell, grid.weights())
        };
        let slope = |axis: Vec3| 0.5 * (shade(axis) - shade(-axis));
        Vec3::new(slope(Vec3::X), slope(Vec3::Y), slope(Vec3::Z))
    }

    /// Fills `positions`, `directions`, `factors` and `buds` with the live buds of
    /// `graph`.
    fn collect_buds(&mut self, graph: &PlantGraph) {
        self.buds.clear();
        self.positions.clear();
        self.directions.clear();
        self.factors.clear();
        let (nodes, positions) = (graph.bud_nodes(), graph.positions());
        let directions = graph.bud_directions();
        for (bud, state) in graph.bud_states().iter().enumerate() {
            if *state != BudState::Dead {
                let node = nodes[bud];
                let axis = positions[node.index()] - graph.proximal(node);
                self.buds.push(bud as u32);
                self.positions.push(positions[node.index()] + self.origin);
                self.directions.push(directions[bud]);
                let factor = gravimorphism_factor(axis, self.gravimorphism);
                self.factors.push(factor);
            }
        }
    }
}

/// Gravimorphism factor `b = cos² tau + d sin² tau` of Longay et al. (2012, eq. 1), `tau`
/// the angle between the vertical and `axis`, written `d + (1 - d) cos² tau` so that
/// `d = 1` gives exactly 1. A zero `axis` counts as vertical.
#[must_use]
pub fn gravimorphism_factor(axis: Vec3, d: f32) -> f32 {
    let cos = axis.try_normalize().map_or(1.0, |unit| unit.y);
    d + (1.0 - d) * cos * cos
}

impl Environment for Competition<'_> {
    fn sense(&mut self, plant: &mut Plant, _cycle: u16) {
        self.update_shadow(&plant.graph);
        self.collect_buds(&plant.graph);
        let mut sensed = std::mem::take(&mut self.sensed);
        sensed.clear();
        let one = |position: &Vec3| self.sense_bud(*position);
        if self.options.parallel_buds {
            self.positions
                .par_iter()
                .with_min_len(CHUNK)
                .map(one)
                .collect_into_vec(&mut sensed);
        } else {
            sensed.extend(self.positions.iter().map(one));
        }
        if let Some(markers) = &mut self.markers {
            let seen = markers.perceive(
                &self.positions,
                &self.directions,
                &self.perception,
                self.options.parallel_markers,
            );
            for (bud, towards) in sensed.iter_mut().zip(seen) {
                if *towards == Vec3::ZERO {
                    bud.exposure = 0.0;
                }
                bud.preferred = *towards;
            }
        }
        let buds = plant.graph.buds_mut();
        buds.exposure.fill(0.0);
        let factors = self.factors.iter();
        for ((&bud, sensed), factor) in self.buds.iter().zip(&sensed).zip(factors) {
            buds.exposure[bud as usize] = factor * sensed.exposure.powf(self.exponent);
            buds.preferred[bud as usize] = sensed.preferred;
        }
        self.sensed = sensed;
    }
}

#[cfg(test)]
mod tests {
    use super::{Competition, EngineOptions, gravimorphism_factor};
    use crate::plant::Plant;
    use crate::stage::{Context, Environment};
    use crate::surroundings::Surroundings;
    use arbrume_core::graph::{BudKind, Metamer};
    use arbrume_core::species::Species;
    use glam::Vec3;

    #[test]
    fn gravimorphism_is_d_on_horizontal_axes_and_one_on_vertical_ones() {
        for d in [0.0, 0.3, 0.75, 1.0] {
            assert!((gravimorphism_factor(Vec3::X * 2.0, d) - d).abs() < 1e-6);
            assert!((gravimorphism_factor(-Vec3::Z, d) - d).abs() < 1e-6);
            assert_eq!(gravimorphism_factor(Vec3::Y * 0.5, d), 1.0);
            assert_eq!(gravimorphism_factor(-Vec3::Y, d), 1.0);
            let slanted = gravimorphism_factor(Vec3::new(1.0, 1.0, 0.0), d);
            assert!((slanted - f32::midpoint(1.0, d)).abs() < 1e-6);
        }
        for axis in [Vec3::X, Vec3::new(0.3, -0.8, 0.1), Vec3::ZERO] {
            assert_eq!(gravimorphism_factor(axis, 1.0).to_bits(), 1.0_f32.to_bits());
        }
    }

    /// Exposure of the terminal buds of a vertical and of a horizontal stem standing far
    /// apart, under the shadow model with gravimorphism `d`.
    fn exposures(d: f32) -> [f32; 2] {
        let mut species = Species::default();
        species.growth.self_organization.gravimorphism = d;
        let mut plant = Plant::new();
        for (origin, axis) in [(Vec3::ZERO, Vec3::Y), (Vec3::X * 100.0, Vec3::X)] {
            let metamer = Metamer::new(origin + axis, 0.01, 0);
            let node = plant.graph.add_root_chain(origin, metamer);
            plant.graph.add_bud(node, BudKind::Terminal, axis);
        }
        let cx = Context {
            species: &species,
            seed: 0,
            age: 1,
            scale: 1.0,
        };
        let open = Surroundings::none();
        let mut competition = Competition::new(&cx, &open, EngineOptions::default());
        competition.sense(&mut plant, 0);
        let exposure = plant.graph.bud_exposures();
        [exposure[0], exposure[1]]
    }

    #[test]
    fn horizontal_buds_sense_d_times_the_light_of_vertical_ones() {
        let [vertical, horizontal] = exposures(1.0);
        assert_eq!(vertical, horizontal);
        let [vertical_d, horizontal_d] = exposures(0.4);
        assert_eq!(vertical_d, vertical);
        assert!((horizontal_d - 0.4 * horizontal).abs() < 1e-6);
    }
}

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
//! `Q` is finally raised to the `light_exponent` of the species. Blocking is not sensed
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
    options: EngineOptions,
    /// Whether the shadow of each node is in the grid.
    cast: Vec<bool>,
    buds: Vec<u32>,
    positions: Vec<Vec3>,
    directions: Vec<Vec3>,
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
            options,
            cast: Vec::new(),
            buds: Vec::new(),
            positions: Vec::new(),
            directions: Vec::new(),
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

    /// Fills `positions`, `directions` and `buds` with the live buds of `graph`.
    fn collect_buds(&mut self, graph: &PlantGraph) {
        self.buds.clear();
        self.positions.clear();
        self.directions.clear();
        let (nodes, positions) = (graph.bud_nodes(), graph.positions());
        let directions = graph.bud_directions();
        for (bud, state) in graph.bud_states().iter().enumerate() {
            if *state != BudState::Dead {
                self.buds.push(bud as u32);
                self.positions
                    .push(positions[nodes[bud].index()] + self.origin);
                self.directions.push(directions[bud]);
            }
        }
    }
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
        for (&bud, sensed) in self.buds.iter().zip(&sensed) {
            buds.exposure[bud as usize] = sensed.exposure.powf(self.exponent);
            buds.preferred[bud as usize] = sensed.preferred;
        }
        self.sensed = sensed;
    }
}

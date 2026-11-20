//! What a root tip senses: the resources of the soil.
//!
//! [`SoilSensing`] is the environment stage of the root system. It gives every live root
//! bud an uptake `Q` and a preferred direction `V`, in the spirit of the root model of
//! Li et al. (2023), where the growth of root branches follows the water and nutrients
//! of the soil around their tips and the direction of extension points to the highest
//! nutrient amount.
//!
//! * **Self-organizing mode.** The resource is a set of points scattered in the soil,
//!   denser where the soil is richer ([`scatter`]). Each cycle the points within the
//!   occupancy radius of a root tip are taken up and removed, and every other point in
//!   the perception cone of one or more tips goes to the closest ([`Markers`]); a tip
//!   that owns points has `Q` equal to the richness of the soil where it stands and
//!   prefers the direction of its points, a tip that owns none has `Q = 0`. Tips
//!   therefore compete for a resource they deplete, and the allocation of the cycle
//!   ([`crate::ResourceFate`]) feeds the roots in rich soil at the expense of the others.
//! * **Parametric mode.** Nothing is taken up: `Q = 1`, and `V` is the direction in
//!   which the richness of the soil rises, zero in homogeneous soil.
//!
//! The richness comes from the [`Soil`](super::Soil): 1 in homogeneous soil, more in
//! its resource patches.

use super::frame::SoilView;
use crate::markers::{Markers, Perception};
use crate::plant::Plant;
use crate::stage::{Context, Environment};
use arbrume_core::graph::BudState;
use arbrume_core::rng::{Rng, Stream, mix64};
use glam::Vec3;

/// The resource points of the soil seen by a root system, in the turned frame: about
/// `density` per cubic meter of homogeneous soil in the box of half-width `reach` and
/// depth `reach` below the collar, then for each resource patch `richness` times as many
/// again where the patch is at full strength, fewer where it fades; at most `limit` in
/// all. Points in the air or in an obstacle are left out. Candidate `i` of a pass is
/// drawn from the seed, the pass and `i`, so the points depend on the seed, the soil and
/// the surroundings only.
pub(crate) fn scatter(view: &SoilView, seed: u64, density: f32, limit: u32, reach: f32) -> Markers {
    let top = view.mirror.top;
    let min = Vec3::new(-reach, 0.0, -reach);
    let size = Vec3::new(2.0 * reach, top + reach, 2.0 * reach);
    let patches: Vec<(Vec3, Vec3, f32)> = view
        .soil
        .resources
        .iter()
        .map(|patch| {
            let bounds = patch.shape.bounds();
            let grown = Vec3::splat(patch.reach);
            let (low, high) = (bounds.min - grown, bounds.max + grown);
            (
                Vec3::new(low.x, top - high.y, low.z),
                high - low,
                patch.richness,
            )
        })
        .collect();
    let volume = |size: Vec3| f64::from(size.x) * f64::from(size.y) * f64::from(size.z);
    let wanted = volume(size)
        + patches
            .iter()
            .map(|p| volume(p.1) * f64::from(p.2))
            .sum::<f64>();
    let share = (f64::from(limit) / (wanted * f64::from(density))).min(1.0) as f32;
    let mut points = Vec::new();
    sample(
        view,
        seed,
        (min, size, density * share),
        &|_| 1.0,
        limit,
        &mut points,
    );
    for (index, (patch, &(low, extent, richness))) in
        view.soil.resources.iter().zip(&patches).enumerate()
    {
        let pass = mix64(seed ^ (index as u64 + 1));
        let strength = |turned: Vec3| patch.at(view.mirror.point(turned)) / patch.richness;
        let region = (low, extent, density * share * richness);
        sample(view, pass, region, &strength, limit, &mut points);
    }
    Markers::from_points(points)
}

/// Adds to `points` the points of one pass of [`scatter`]: candidates drawn uniformly
/// in the box `(min, size)` at `density` per cubic meter, each kept with the probability
/// `keep` gives at it when it lies in the soil, until `points` holds `limit`.
fn sample(
    view: &SoilView,
    seed: u64,
    (min, size, density): (Vec3, Vec3, f32),
    keep: &dyn Fn(Vec3) -> f32,
    limit: u32,
    points: &mut Vec<Vec3>,
) {
    let volume = f64::from(size.x) * f64::from(size.y) * f64::from(size.z);
    let candidates = (volume * f64::from(density)).min(1e9) as u32;
    for index in 0..candidates {
        if points.len() >= limit as usize {
            break;
        }
        let mut rng = Rng::new(seed, index, Stream::ENVIRONMENT);
        let point = min + Vec3::new(rng.f32(), rng.f32(), rng.f32()) * size;
        if view.clearance(point) > 0.0 && rng.f32() < keep(point) {
            points.push(point);
        }
    }
}

/// Environment stage of the root system. See the [module documentation](self).
#[derive(Debug, Clone)]
pub(crate) struct SoilSensing<'a> {
    view: &'a SoilView<'a>,
    markers: Option<Markers>,
    perception: Perception,
    /// Step of the central differences of the resource gradient, in meters.
    step: f32,
    parallel: bool,
    buds: Vec<u32>,
    positions: Vec<Vec3>,
    directions: Vec<Vec3>,
}

impl<'a> SoilSensing<'a> {
    /// The environment of the root system of the run `cx` (of the root species) in
    /// `view`; `markers` are the resource points of the self-organizing mode, `None` in
    /// the parametric mode.
    pub(crate) fn new(
        cx: &Context,
        view: &'a SoilView<'a>,
        markers: Option<Markers>,
        parallel: bool,
    ) -> Self {
        let growth = &cx.species.growth;
        let model = &growth.self_organization;
        let unit = growth.internode_unit() * cx.scale;
        Self {
            view,
            markers,
            perception: Perception {
                occupancy: model.occupancy_radius * unit,
                reach: model.perception_distance * unit,
                angle: model.perception_angle_deg.to_radians(),
            },
            step: unit,
            parallel,
            buds: Vec::new(),
            positions: Vec::new(),
            directions: Vec::new(),
        }
    }
}

impl Environment for SoilSensing<'_> {
    fn sense(&mut self, plant: &mut Plant, _cycle: u16) {
        let graph = &plant.graph;
        self.buds.clear();
        self.positions.clear();
        self.directions.clear();
        let (nodes, positions) = (graph.bud_nodes(), graph.positions());
        for (bud, state) in graph.bud_states().iter().enumerate() {
            if *state != BudState::Dead {
                self.buds.push(bud as u32);
                self.positions.push(positions[nodes[bud].index()]);
                self.directions.push(graph.bud_directions()[bud]);
            }
        }
        let buds = plant.graph.buds_mut();
        buds.exposure.fill(0.0);
        if let Some(markers) = &mut self.markers {
            let seen = markers.perceive(
                &self.positions,
                &self.directions,
                &self.perception,
                self.parallel,
            );
            for ((&bud, towards), position) in self.buds.iter().zip(seen).zip(&self.positions) {
                let fed = *towards != Vec3::ZERO;
                buds.exposure[bud as usize] = if fed {
                    self.view.richness(*position)
                } else {
                    0.0
                };
                buds.preferred[bud as usize] = *towards;
            }
        } else {
            for (&bud, position) in self.buds.iter().zip(&self.positions) {
                buds.exposure[bud as usize] = 1.0;
                buds.preferred[bud as usize] = self.view.resource_direction(*position, self.step);
            }
        }
    }
}

//! The water surface: canopies, floating leaves and pads.
//!
//! * [`canopy`] flags the nodes at the surface: within their radius and
//!   [`SURFACE_BAND`] below it, or above it;
//! * [`relay`] lays the plant out again from its base, every internode with its length
//!   and direction, except that an internode that would end above the surface (less its
//!   radius), or that belongs to the canopy, ends on it, keeping its length and its
//!   azimuth, turned downstream in a current by the `leaf_flex` of the species: stems
//!   never cross the surface, canopies lie on it and trail with the current, and a frond
//!   that leans in a current reaches the surface further along its length and floats
//!   less of it;
//! * [`add_pads`] ends every axis that reaches the surface in one leaf lying flat on
//!   it, for the floating leaves of a water lily, and drops the other leaves;
//! * [`orient_leaves`] turns the leaves of nodes at the surface flat on it, trailing
//!   downstream, turns the submerged leaves towards the force of the water on them
//!   (lift and drag), and tilts a leaf whose tip would cross the surface until its tip
//!   is on it.

use super::SURFACE_BAND;
use super::forces::leaf_force_per_area;
use crate::math::{UP, reject, upward_normal};
use crate::water::Water;
use arbrume_core::graph::{NodeId, OrganKind, OrganSite, PlantGraph};
use arbrume_core::species::{Aquatic, Species};
use glam::Vec3;

/// Height of a pad above the surface, in meters, so that it sits on the water.
pub(crate) const PAD_LIFT: f32 = 0.004;

/// Whether a node at `height` with an internode of `radius` is at the surface.
fn at_surface(water: &Water, height: f32, radius: f32) -> bool {
    height >= water.level() - radius - SURFACE_BAND
}

/// Flags the nodes of `graph` at the surface of `water`.
pub(super) fn canopy(graph: &PlantGraph, water: &Water) -> Vec<bool> {
    graph
        .positions()
        .iter()
        .zip(graph.radii())
        .map(|(position, &radius)| at_surface(water, position.y, radius))
        .collect()
}

/// Lays `graph` out again under the surface of `water`, the level parts turned towards
/// the flow by `trail` in `[0, 1]`. See the [module documentation](self).
pub(super) fn relay(graph: &mut PlantGraph, water: &Water, canopy: &[bool], trail: f32) {
    let downstream = water.flow().normalize_or_zero() * trail;
    let old = graph.positions().to_vec();
    let mut new = old.clone();
    for node in graph.node_ids() {
        let index = node.index();
        let (old_base, new_base) = graph.parent(node).map_or_else(
            || {
                let origin = graph.chain_origin(graph.chain_of(node));
                (origin, origin)
            },
            |parent| (old[parent.index()], new[parent.index()]),
        );
        let step = old[index] - old_base;
        let limit = water.level() - graph.radii()[index];
        let end = new_base + step;
        if end.y <= limit && !canopy[index] {
            new[index] = end;
            continue;
        }
        let length = step.length();
        let rise = (limit - new_base.y).clamp(-length, length);
        let level = level_or(step, level_or(new_base - old_base, Vec3::X));
        let level = (level * (1.0 - trail) + downstream)
            .try_normalize()
            .unwrap_or(level);
        new[index] = new_base + level * (length * length - rise * rise).sqrt() + UP * rise;
    }
    graph.positions_mut().copy_from_slice(&new);
}

/// The last live node of every chain whose tip is at the surface.
fn surface_tips(graph: &PlantGraph, water: &Water) -> Vec<NodeId> {
    graph
        .chain_ids()
        .filter_map(|chain| {
            let tip = graph
                .chain_nodes_rev(chain)
                .find(|&node| !graph.is_shed(node))?;
            let height = graph.position(tip).y;
            at_surface(water, height, graph.radii()[tip.index()]).then_some(tip)
        })
        .collect()
}

/// A horizontal unit vector along `direction`, or `fallback` when it is vertical.
pub(super) fn level_or(direction: Vec3, fallback: Vec3) -> Vec3 {
    reject(direction, UP).try_normalize().unwrap_or(fallback)
}

/// Ends every axis that reaches the surface in one leaf lying flat on it, and drops
/// every other leaf. Run before the forces of the water, which the pads do not load
/// but the dropped leaves would; [`lift_pads`] then puts the pads on the surface.
pub(super) fn add_pads(graph: &mut PlantGraph, water: &Water) {
    let tips = surface_tips(graph, water);
    graph.clear_organs();
    for tip in tips {
        let along = graph.position(tip) - graph.proximal(tip);
        graph.add_organ(OrganSite {
            node: tip,
            kind: OrganKind::Leaf,
            direction: level_or(along, Vec3::X),
            normal: UP,
            scale: 1.0,
            birth: graph.births()[tip.index()],
        });
    }
}

/// Moves the node of every pad just above the surface.
pub(super) fn lift_pads(graph: &mut PlantGraph, water: &Water) {
    let nodes: Vec<NodeId> = graph.organs().iter().map(|site| site.node).collect();
    for node in nodes {
        graph.positions_mut()[node.index()].y = water.level() + PAD_LIFT;
    }
}

/// The attitude of a leaf of length `length` on a node at `height`: see the
/// [module documentation](self).
struct Attitude<'a> {
    aquatic: &'a Aquatic,
    water: &'a Water,
    /// Unit direction of the force per area on a submerged blade, zero for none.
    pull: Vec3,
    /// Unit direction of the flow, zero in still water.
    downstream: Vec3,
    /// Sine of the largest random turn the mesher gives a leaf, which may lift its tip.
    wobble: f32,
}

impl Attitude<'_> {
    /// Direction and normal of a leaf at the surface: level, upper side up, turned
    /// downstream by the flexibility of the leaf.
    fn floating(&self, site: &OrganSite) -> (Vec3, Vec3) {
        let flat = level_or(site.direction, level_or(site.normal, Vec3::X));
        let trail = self.aquatic.leaf_flex;
        let target = (flat * (1.0 - trail) + self.downstream * trail)
            .try_normalize()
            .unwrap_or(flat);
        let weight = self.aquatic.floating_leaves;
        let direction = (site.direction * (1.0 - weight) + target * weight)
            .try_normalize()
            .unwrap_or(target);
        let normal = site.normal * (1.0 - weight) + UP * weight;
        (direction, normal)
    }

    /// Direction of a submerged leaf, turned towards the force on it.
    fn submerged(&self, site: &OrganSite) -> Vec3 {
        let flex = self.aquatic.leaf_flex;
        (site.direction * (1.0 - flex) + self.pull * flex)
            .try_normalize()
            .unwrap_or(site.direction)
    }

    /// `direction` tilted so that a leaf of `length` from `height` keeps its tip under
    /// the surface, whatever random turn the mesher gives it.
    fn under_surface(&self, direction: Vec3, height: f32, length: f32) -> Vec3 {
        let surface = self.water.level() - self.wobble * length;
        if height + direction.y * length <= surface || length <= 0.0 {
            return direction;
        }
        let rise = ((surface - height) / length).clamp(-1.0, 1.0);
        let level = level_or(
            direction,
            self.downstream.try_normalize().unwrap_or(Vec3::X),
        );
        level * (1.0 - rise * rise).sqrt() + UP * rise
    }
}

/// Turns the leaves of `graph` in `water`: see the [module documentation](self).
pub(super) fn orient_leaves(
    graph: &mut PlantGraph,
    species: &Species,
    aquatic: &Aquatic,
    water: &Water,
) {
    let attitude = Attitude {
        aquatic,
        water,
        pull: leaf_force_per_area(species, water).normalize_or_zero(),
        downstream: water.flow().normalize_or_zero(),
        wobble: species.foliage.orientation_variation_deg.to_radians().sin(),
    };
    let foliage = &species.foliage;
    let reach = foliage.length * (1.0 + foliage.petiole_length);
    let positions = graph.positions().to_vec();
    let radii = graph.radii().to_vec();
    for site in graph.organs_mut() {
        if site.kind != OrganKind::Leaf {
            continue;
        }
        let height = positions[site.node.index()].y;
        let (direction, normal) = if at_surface(water, height, radii[site.node.index()]) {
            attitude.floating(site)
        } else {
            let direction = attitude.submerged(site);
            let length = reach * site.scale;
            (
                attitude.under_surface(direction, height, length),
                site.normal,
            )
        };
        site.direction = direction;
        site.normal = reject(normal, direction)
            .try_normalize()
            .unwrap_or_else(|| upward_normal(direction, direction.any_orthonormal_vector()));
    }
}

#[cfg(test)]
mod tests {
    use super::{PAD_LIFT, add_pads, canopy, lift_pads, relay};
    use crate::water::Water;
    use arbrume_core::graph::{Metamer, PlantGraph};
    use glam::Vec3;

    #[test]
    fn nodes_end_under_the_surface_and_pads_float_on_it() {
        let mut graph = PlantGraph::new();
        let first = graph.add_root_chain(Vec3::ZERO, Metamer::new(Vec3::Y * 0.8, 0.01, 0));
        let chain = graph.chain_of(first);
        graph.extend_chain(chain, Metamer::new(Vec3::new(0.1, 1.6, 0.0), 0.01, 1));
        let water = Water::still(1.5);
        let flags = canopy(&graph, &water);
        assert_eq!(flags, [false, true]);
        relay(&mut graph, &water, &flags, 0.5);
        assert!((graph.positions()[1].y - 1.49).abs() < 1e-6);
        let second = graph.positions()[1] - graph.positions()[0];
        assert!((second.length() - Vec3::new(0.1, 0.8, 0.0).length()).abs() < 1e-5);
        assert!((graph.positions()[0].y - 0.8).abs() < 1e-6);
        add_pads(&mut graph, &water);
        lift_pads(&mut graph, &water);
        assert_eq!(graph.organs().len(), 1);
        let pad = graph.organs()[0];
        assert!((pad.normal - Vec3::Y).length() < 1e-6 && pad.direction.y.abs() < 1e-6);
        assert!((graph.positions()[1].y - 1.5 - PAD_LIFT).abs() < 1e-6);
    }
}

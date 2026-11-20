//! Bending of the stems by the forces of the water.
//!
//! Every internode carries two forces, applied at its middle:
//!
//! * the net buoyancy of its tissue, `s rho g V` upwards, with `s` the `stem_buoyancy`
//!   of the species (`1 - rho_tissue / rho_water`, negative for a stem that sinks), `V`
//!   the volume of the internode as a cylinder and `rho` the density of the water;
//! * the drag of the current across it, `0.5 rho Cd (2 r l) |u_n| u_n`, with `u_n` the
//!   part of the velocity of the current normal to the internode, `Cd` the
//!   `drag_coefficient`, `r` and `l` its radius and length (the quadratic drag of a
//!   cylinder in cross flow).
//!
//! A stem of the canopy trails along the flow on the surface and has no cross flow. The
//! leaves of a node add the drag of their blades lying in the flow,
//! `0.5 rho Cd_leaf A |u| u` with `A` their area and `Cd_leaf` the
//! `leaf_drag_coefficient`, and their lift, `leaf_lift A` plus the buoyancy of the float
//! at the base of each blade.
//!
//! As in the bending under weight (`docs/growth.md`), each internode is a cantilever in
//! the small-deflection approximation: with `F` the sum of the forces on everything it
//! carries and `L` the lever from its base to their point of application (the centroid
//! weighted by the magnitudes of the forces), it turns with all it carries by
//! `|L x F| l / (E pi r^4 / 4)` about `L x F`, `E` the `stiffness` of the species,
//! limited to the angle that aligns the lever with the force. A limp stem therefore
//! lines up with the force it carries, as a tethered float does, and leans downstream
//! by the ratio of drag to buoyancy. A canopy floating on the surface is held up by it:
//! only the horizontal part of its forces (the drag of the current) pulls on its frond,
//! where the frond reaches the surface; the canopy turns with its frond without turning
//! of its own, and the surface pass ([`super::surface::relay`]) lays it back on the
//! surface.
//! The pass runs once from base to tips on the unbent shape; it does not iterate to
//! equilibrium, and the drag is not recomputed on the bent stems.

use crate::math::UP;
use crate::water::{GRAVITY, Water};
use arbrume_core::graph::{NodeId, OrganKind, PlantGraph};
use arbrume_core::species::{Foliage, Species};
use glam::{Quat, Vec3};
use std::f32::consts::{FRAC_PI_4, PI};

/// Part of the bounding rectangle of a blade that the blade covers, a round figure for
/// the outlines of the library.
const BLADE_FILL: f32 = 0.65;

/// Ratio of the width of a blade float to its length.
pub(crate) const FLOAT_WIDTH: f32 = 0.6;

/// Area of the leaves of one organ site of scale 1, in m².
pub(crate) fn site_area(foliage: &Foliage) -> f32 {
    let leaves = foliage.twig.leaves.max(1) as f32;
    foliage.length * foliage.length * foliage.width_ratio * BLADE_FILL * leaves
}

/// Volume of the floats of one organ site of scale 1, in m³: an ellipsoid of length
/// `blade_float` leaf lengths at the base of every blade.
pub(crate) fn site_float_volume(foliage: &Foliage, blade_float: f32) -> f32 {
    let length = blade_float * foliage.length;
    let width = FLOAT_WIDTH * length;
    let leaves = foliage.twig.leaves.max(1) as f32;
    PI / 6.0 * length * width * width * leaves
}

/// Force per unit of blade area on a leaf, in N/m²: lift and float buoyancy upwards,
/// drag along the flow.
pub(crate) fn leaf_force_per_area(species: &Species, water: &Water) -> Vec3 {
    let Some(aquatic) = &species.aquatic else {
        return Vec3::ZERO;
    };
    let area = site_area(&species.foliage);
    let float = site_float_volume(&species.foliage, aquatic.blade_float);
    let float_lift = water.density * GRAVITY * float / area.max(f32::MIN_POSITIVE);
    let flow = water.flow();
    let drag = 0.5 * water.density * aquatic.leaf_drag_coefficient * flow.length() * flow;
    UP * (aquatic.leaf_lift + float_lift) + drag
}

/// Forces carried by an internode: their sum, the sum of their magnitudes and the sum
/// of magnitude times point of application.
#[derive(Debug, Clone, Copy, Default)]
struct Load {
    force: Vec3,
    weight: f32,
    moment: Vec3,
}

impl Load {
    fn at(force: Vec3, point: Vec3) -> Self {
        let weight = force.length();
        Self {
            force,
            weight,
            moment: point * weight,
        }
    }

    fn add(&mut self, other: &Self) {
        self.force += other.force;
        self.weight += other.weight;
        self.moment += other.moment;
    }
}

/// The forces on each internode and its leaves, at the middle of the internode. The
/// surface holds up the nodes of `canopy`: only the horizontal part of their forces
/// pulls on the frond, where it reaches the surface.
fn loads(graph: &PlantGraph, species: &Species, water: &Water, canopy: &[bool]) -> Vec<Load> {
    let Some(aquatic) = &species.aquatic else {
        return Vec::new();
    };
    let flow = water.flow();
    let rho = water.density;
    let mut loads: Vec<Load> = graph
        .node_ids()
        .map(|node| {
            let (base, tip) = (graph.proximal(node), graph.position(node));
            let radius = graph.radii()[node.index()];
            let length = tip.distance(base);
            let tangent = (tip - base).normalize_or_zero();
            // A floating stem trails along the flow, so no flow crosses it.
            let crossing = if canopy[node.index()] {
                Vec3::ZERO
            } else {
                flow
            };
            let normal_flow = crossing - tangent * crossing.dot(tangent);
            let lift = aquatic.stem_buoyancy * rho * GRAVITY * PI * radius * radius * length;
            let drag = 0.5 * rho * aquatic.drag_coefficient * 2.0 * radius * length;
            let force = UP * lift + normal_flow * (drag * normal_flow.length());
            Load::at(force, 0.5 * (base + tip))
        })
        .collect();
    let per_area = leaf_force_per_area(species, water);
    let area = site_area(&species.foliage);
    for site in graph.organs() {
        if site.kind != OrganKind::Leaf || graph.is_shed(site.node) {
            continue;
        }
        let point = graph.position(site.node);
        let force = per_area * (area * site.scale * site.scale);
        loads[site.node.index()].add(&Load::at(force, point));
    }
    let entries = entries(graph, canopy);
    for (load, entry) in loads.iter_mut().zip(entries) {
        if let Some(entry) = entry {
            let level = Vec3::new(load.force.x, 0.0, load.force.z);
            *load = Load::at(level, entry);
        }
    }
    loads
}

/// For each node of `canopy`, the position of the first node of the canopy on its path
/// from the base: where its frond reaches the surface. `None` for the other nodes.
fn entries(graph: &PlantGraph, canopy: &[bool]) -> Vec<Option<Vec3>> {
    let mut entries: Vec<Option<Vec3>> = vec![None; graph.node_count()];
    for node in graph.node_ids() {
        if !canopy[node.index()] {
            continue;
        }
        let inherited = graph
            .parent(node)
            .and_then(|parent| entries[parent.index()]);
        entries[node.index()] = Some(inherited.unwrap_or_else(|| graph.position(node)));
    }
    entries
}

/// Rotation of an internode of `section` (length, radius) carrying `force` applied at
/// `lever` from its base, for a stiffness `stiffness`.
fn turn(lever: Vec3, force: Vec3, (length, radius): (f32, f32), stiffness: f32) -> Quat {
    let moment = lever.cross(force);
    let Some(axis) = moment.try_normalize() else {
        return Quat::IDENTITY;
    };
    if radius <= 0.0 {
        return Quat::IDENTITY;
    }
    let rigidity = stiffness * FRAC_PI_4 * radius.powi(4);
    let angle = moment.length() * length / rigidity;
    let aligned = lever.angle_between(force);
    Quat::from_axis_angle(axis, angle.min(aligned))
}

/// Bends `graph` with the forces of `water` and turns its organ sites with their nodes.
/// Nodes flagged in `canopy` turn with their parent only.
pub(super) fn bend_in_water(
    graph: &mut PlantGraph,
    species: &Species,
    water: &Water,
    canopy: &[bool],
) {
    let Some(aquatic) = &species.aquatic else {
        return;
    };
    let mut loads = loads(graph, species, water, canopy);
    graph.accumulate_basipetal(&mut loads, Load::add);
    let unbent = graph.positions().to_vec();
    let bases: Vec<Vec3> = graph.node_ids().map(|node| graph.proximal(node)).collect();
    let mut rotations = vec![Quat::IDENTITY; unbent.len()];
    let mut bent = unbent.clone();
    for node in 0..unbent.len() {
        let (base, tip, load) = (bases[node], unbent[node], loads[node]);
        let (bent_base, carried) = match graph.parent(NodeId::new(node)) {
            Some(parent) => (bent[parent.index()], rotations[parent.index()]),
            None => (base, Quat::IDENTITY),
        };
        rotations[node] = carried;
        if load.weight > 0.0 && !canopy[node] {
            let lever = carried * (load.moment / load.weight - base);
            let section = (tip.distance(base), graph.radii()[node]);
            let local = turn(lever, load.force, section, aquatic.stiffness);
            rotations[node] = (local * carried).normalize();
        }
        bent[node] = bent_base + rotations[node] * (tip - base);
    }
    graph.positions_mut().copy_from_slice(&bent);
    for site in graph.organs_mut() {
        let rotation = rotations[site.node.index()];
        site.direction = (rotation * site.direction).normalize();
        site.normal = (rotation * site.normal).normalize();
    }
}

#[cfg(test)]
mod tests {
    use super::bend_in_water;
    use crate::water::Water;
    use arbrume_core::graph::{Metamer, PlantGraph};
    use arbrume_core::species::{Aquatic, Species};
    use glam::Vec3;

    /// A vertical stem of `count` internodes of 0.5 m and 5 mm radius.
    fn stem(count: u8) -> PlantGraph {
        let mut graph = PlantGraph::new();
        let first = graph.add_root_chain(Vec3::ZERO, Metamer::new(Vec3::Y * 0.5, 0.005, 0));
        let chain = graph.chain_of(first);
        for index in 1..count {
            let position = Vec3::Y * (0.5 * f32::from(index + 1));
            graph.extend_chain(chain, Metamer::new(position, 0.005, 0));
        }
        graph
    }

    fn species() -> Species {
        Species {
            aquatic: Some(Aquatic {
                stiffness: 1.0e6,
                ..Aquatic::default()
            }),
            ..Species::default()
        }
    }

    #[test]
    fn a_current_bends_a_stem_downstream_and_keeps_its_lengths() {
        let mut graph = stem(8);
        let water = Water {
            current: Vec3::new(0.4, 0.0, 0.0),
            ..Water::still(10.0)
        };
        bend_in_water(&mut graph, &species(), &water, &[false; 8]);
        let tip = *graph.positions().last().unwrap();
        assert!(tip.x > 0.3, "{tip}");
        assert!(tip.z.abs() < 1e-5 && tip.y > 0.0);
        for node in graph.node_ids() {
            assert!((graph.internode_length(node) - 0.5).abs() < 1e-4);
        }
        let mut still = stem(8);
        bend_in_water(&mut still, &species(), &Water::still(10.0), &[false; 8]);
        assert!(still.positions().last().unwrap().x.abs() < 1e-5);
    }

    #[test]
    fn a_stronger_current_bends_further() {
        let tip_x = |speed: f32| {
            let mut graph = stem(8);
            let water = Water {
                current: Vec3::new(speed, 0.0, 0.0),
                ..Water::still(10.0)
            };
            bend_in_water(&mut graph, &species(), &water, &[false; 8]);
            graph.positions().last().unwrap().x
        };
        assert!(tip_x(0.1) < tip_x(0.3) && tip_x(0.3) < tip_x(0.8));
    }
}

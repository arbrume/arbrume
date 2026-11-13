//! Bending of the axes under the weight of the wood they carry.
//!
//! Each internode is treated as a cantilever in the small-deflection approximation of
//! beam theory. For the internode of node `i`, of length `l` and radius `r`:
//!
//! * `V` is the volume of wood from the internode to the tips it carries (each internode
//!   counted as a cylinder), and `d` the horizontal distance from the base of the
//!   internode to the centre of mass of that wood;
//! * the bending moment at the base is `rho * g * V * d` and the second moment of area
//!   of the section is `I = pi * r^4 / 4`;
//! * the internode and everything it carries rotate downwards by the angle
//!   `c * V * d * l / I`, where `c = rho * g / E` is the `weight_bending` of the
//!   species, about the horizontal axis through the base that is orthogonal to the
//!   lever arm. The angle is limited to the one that brings the centre of mass straight
//!   below the base, where the moment vanishes.
//!
//! The pass is acropetal: the volume and centre of mass carried by each internode are
//! summed once on the unbent shape, and the lever arm of an internode is turned by the
//! rotation accumulated below it before its own angle is computed. Leaves add no load.
//! This is a first-order estimate with a physical parameter, not a simulation: it does
//! not iterate to equilibrium and ignores the reaction wood by which real axes resist
//! sagging.

use crate::math::{UP, reject};
use arbrume_core::graph::{NodeId, PlantGraph};
use glam::{Quat, Vec3};
use std::f32::consts::{FRAC_PI_4, PI};

/// Wood carried by an internode: its volume and the sum of volume times position.
#[derive(Debug, Clone, Copy)]
struct Load {
    volume: f32,
    moment: Vec3,
}

/// Rotation of one internode of length `length` and radius `radius` carrying `volume` of
/// wood whose centre of mass is at `lever` from its base.
fn sag(volume: f32, lever: Vec3, (length, radius): (f32, f32), compliance: f32) -> Quat {
    let horizontal = reject(lever, UP);
    let reach = horizontal.length();
    let Some(axis) = UP.cross(horizontal).try_normalize() else {
        return Quat::IDENTITY;
    };
    if radius <= 0.0 {
        return Quat::IDENTITY;
    }
    let stiffness = FRAC_PI_4 * radius.powi(4);
    let angle = compliance * volume * reach * length / stiffness;
    let hanging = reach.atan2(-lever.y);
    Quat::from_axis_angle(axis, angle.min(hanging))
}

/// Bends the plant under its own weight and turns its organ sites with their nodes.
/// `compliance` is `rho * g / E` in 1/m; nothing happens when it is not positive.
pub(crate) fn bend_under_weight(graph: &mut PlantGraph, compliance: f32) {
    if compliance <= 0.0 || graph.node_count() == 0 {
        return;
    }
    let unbent = graph.positions().to_vec();
    let bases: Vec<Vec3> = graph.node_ids().map(|node| graph.proximal(node)).collect();
    let mut loads: Vec<Load> = graph
        .node_ids()
        .map(|node| {
            let (base, tip) = (bases[node.index()], unbent[node.index()]);
            let radius = graph.radii()[node.index()];
            let volume = PI * radius * radius * tip.distance(base);
            Load {
                volume,
                moment: (base + tip) * (0.5 * volume),
            }
        })
        .collect();
    graph.accumulate_basipetal(&mut loads, |parent, child| {
        parent.volume += child.volume;
        parent.moment += child.moment;
    });
    let mut rotations = vec![Quat::IDENTITY; unbent.len()];
    let mut bent = unbent.clone();
    for node in 0..unbent.len() {
        let (base, tip, load) = (bases[node], unbent[node], loads[node]);
        let (bent_base, carried) = match graph.parent(NodeId::new(node)) {
            Some(parent) => (bent[parent.index()], rotations[parent.index()]),
            None => (base, Quat::IDENTITY),
        };
        if load.volume > 0.0 {
            let lever = carried * (load.moment / load.volume - base);
            let section = (tip.distance(base), graph.radii()[node]);
            let local = sag(load.volume, lever, section, compliance);
            rotations[node] = (local * carried).normalize();
        } else {
            rotations[node] = carried;
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
    use super::bend_under_weight;
    use arbrume_core::graph::{Metamer, PlantGraph};
    use glam::Vec3;

    /// Horizontal branch of `count` internodes of 0.5 m and 1 cm radius.
    fn beam(count: u8) -> PlantGraph {
        let mut graph = PlantGraph::new();
        let first = graph.add_root_chain(Vec3::ZERO, Metamer::new(Vec3::X * 0.5, 0.01, 0));
        let chain = graph.chain_of(first);
        for index in 1..count {
            let position = Vec3::X * (0.5 * f32::from(index + 1));
            graph.extend_chain(chain, Metamer::new(position, 0.01, 0));
        }
        graph
    }

    #[test]
    fn a_horizontal_branch_sags_and_keeps_its_length() {
        let mut graph = beam(6);
        bend_under_weight(&mut graph, 1e-6);
        graph.check_invariants().unwrap();
        let tip = *graph.positions().last().unwrap();
        assert!(tip.y < 0.0 && tip.z.abs() < 1e-5);
        let chain = graph.chain_of(arbrume_core::graph::NodeId::new(0));
        assert!((graph.chain_length(chain) - 3.0).abs() < 1e-4);
        let heights: Vec<f32> = graph.positions().iter().map(|p| p.y).collect();
        assert!(heights.windows(2).all(|pair| pair[1] < pair[0]));
    }

    #[test]
    fn the_first_internode_turns_by_the_cantilever_angle() {
        let mut graph = beam(2);
        bend_under_weight(&mut graph, 1e-6);
        let volume = std::f32::consts::PI * 1e-4;
        let stiffness = std::f32::consts::FRAC_PI_4 * 1e-8;
        let expected = 1e-6 * volume * 0.5 * 0.5 / stiffness;
        let first = graph.positions()[0];
        assert!(((-first.y).atan2(first.x) - expected).abs() < 1e-5);
    }

    #[test]
    fn sagging_stops_when_the_branch_hangs() {
        let mut graph = beam(6);
        bend_under_weight(&mut graph, 1.0);
        let positions = graph.positions();
        assert!(positions.iter().all(|p| p.is_finite() && p.x >= -1e-3));
        assert!(positions.last().unwrap().y < -2.0);
    }

    #[test]
    fn a_vertical_trunk_and_a_zero_compliance_do_not_move() {
        let mut graph = PlantGraph::new();
        let base = graph.add_root_chain(Vec3::ZERO, Metamer::new(Vec3::Y, 0.1, 0));
        graph.extend_chain(graph.chain_of(base), Metamer::new(Vec3::Y * 2.0, 0.1, 0));
        let before = graph.clone();
        bend_under_weight(&mut graph, 1e-4);
        assert_eq!(graph, before);
        let mut horizontal = beam(3);
        let unbent = horizontal.clone();
        bend_under_weight(&mut horizontal, 0.0);
        assert_eq!(horizontal, unbent);
    }
}

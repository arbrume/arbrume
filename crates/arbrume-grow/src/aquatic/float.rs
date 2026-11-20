//! A large float at the tip of each stem: the pneumatocyst of a bull kelp.
//!
//! The float continues every stem as a few more metamers whose radii follow an ellipse
//! from the radius of the stem to `radius` and back. On a stem that reaches the surface
//! the float lies on it, downstream of the tip (along the level part of the last
//! internode when the water is still), its axis `0.7 radius` under the surface so that
//! its top shows above the water; on a younger stem that has not reached it the float
//! continues the last internode. The leaves of the stem are replaced by `blades` leaves
//! at the far end of the float, fanned around its direction: in a half-turn about the
//! vertical on the surface, all around and outwards below it. The leaf pass of
//! [`super::dress`] then lays the floating blades on the surface and turns the others
//! with the water.

use super::surface::level_or;
use crate::math::{UP, perpendicular};
use crate::stage::Context;
use crate::water::Water;
use arbrume_core::graph::{ChainId, Metamer, NodeId, OrganKind, OrganSite, PlantGraph};
use arbrume_core::rng::{Rng, Stream};
use arbrume_core::species::TipFloat;
use glam::{Quat, Vec3};
use std::f32::consts::TAU;

/// Metamers of a float.
const FLOAT_METAMERS: u32 = 6;

/// Depth of the axis of a float under the surface, in float radii.
const FLOAT_DEPTH: f32 = 0.7;

/// Half-width of the fan of the blades of a floating float about the downstream
/// direction, in radians.
const FAN: f32 = 1.2;

/// Outward lean of the blades of a submerged float from its direction, as the tangent
/// of the angle.
const SPLAY: f32 = 0.5;

/// Stream of the random turns of the blades of a float.
const BLADES: Stream = Stream::custom(0x5A01);

/// A stem that bears a float: its chain, its last live node and whether that node is at
/// the surface.
#[derive(Debug, Clone, Copy)]
struct Stem {
    chain: ChainId,
    tip: NodeId,
    floating: bool,
}

/// The stems of `graph`.
fn stems(graph: &PlantGraph, water: &Water) -> Vec<Stem> {
    let mut stems = Vec::new();
    for chain in graph.chain_ids() {
        if !graph.chain(chain).attach.is_none() {
            continue;
        }
        let Some(tip) = graph.chain_nodes_rev(chain).find(|&n| !graph.is_shed(n)) else {
            continue;
        };
        let radius = graph.radii()[tip.index()];
        let floating = graph.position(tip).y >= water.level() - radius - super::SURFACE_BAND;
        stems.push(Stem {
            chain,
            tip,
            floating,
        });
    }
    stems
}

/// Adds the float of `stem` and returns its last node and direction.
fn add_float(
    graph: &mut PlantGraph,
    stem: Stem,
    (radius, length): (f32, f32),
    water: &Water,
) -> (NodeId, Vec3) {
    let last = stem.tip;
    let along = graph.position(last) - graph.proximal(last);
    let direction = if stem.floating {
        let downstream = water.flow().normalize_or_zero();
        level_or(along + downstream * along.length(), Vec3::X)
    } else {
        along.try_normalize().unwrap_or(UP)
    };
    let start = graph.position(last);
    let stem_radius = graph.radii()[last.index()];
    let birth = graph.births()[last.index()];
    let mut end = last;
    for step in 1..=FLOAT_METAMERS {
        let s = step as f32 / FLOAT_METAMERS as f32;
        let mut point = start + direction * (length * s);
        if stem.floating {
            point.y = water.level() - FLOAT_DEPTH * radius;
        }
        let bulge = (1.0 - (2.0 * s - 1.0).powi(2)).max(0.0).sqrt();
        let r = (radius * bulge).max(stem_radius.min(radius));
        end = graph.extend_chain(stem.chain, Metamer::new(point, r, birth));
    }
    (end, direction)
}

/// Adds a float to every stem of `graph`. See the [module documentation](self).
pub(super) fn add_tip_floats(graph: &mut PlantGraph, cx: &Context, tip: &TipFloat, water: &Water) {
    let stems = stems(graph, water);
    if stems.is_empty() {
        return;
    }
    let size = (tip.radius * cx.scale, tip.length * cx.scale);
    let chains: Vec<ChainId> = stems.iter().map(|stem| stem.chain).collect();
    let mut sites: Vec<OrganSite> = graph
        .organs()
        .iter()
        .filter(|site| !chains.contains(&graph.chain_of(site.node)))
        .copied()
        .collect();
    for stem in stems {
        let (end, direction) = add_float(graph, stem, size, water);
        let mut rng = Rng::new(cx.seed, stem.chain.index() as u32, BLADES);
        for blade in 0..tip.blades {
            let (direction, normal) = if stem.floating {
                fanned(direction, blade, tip.blades, &mut rng)
            } else {
                splayed(direction, blade, tip.blades, &mut rng)
            };
            sites.push(OrganSite {
                node: end,
                kind: OrganKind::Leaf,
                direction,
                normal,
                scale: 1.0 + 0.2 * rng.signed(),
                birth: graph.births()[end.index()],
            });
        }
    }
    graph.clear_organs();
    for site in sites {
        graph.add_organ(site);
    }
}

/// Direction and normal of blade `blade` of `count` on a floating float: level, fanned
/// about `direction`.
fn fanned(direction: Vec3, blade: u32, count: u32, rng: &mut Rng) -> (Vec3, Vec3) {
    let spread = if count > 1 {
        blade as f32 / (count - 1) as f32 * 2.0 - 1.0
    } else {
        0.0
    };
    let yaw = FAN * spread + 0.15 * rng.signed();
    (Quat::from_axis_angle(UP, yaw) * direction, UP)
}

/// Direction and normal of blade `blade` of `count` on a submerged float: leaning out
/// from `direction` all around it.
fn splayed(direction: Vec3, blade: u32, count: u32, rng: &mut Rng) -> (Vec3, Vec3) {
    let side = perpendicular(direction, Vec3::X);
    let azimuth = TAU * (blade as f32 + 0.3 * rng.signed()) / count.max(1) as f32;
    let outward = Quat::from_axis_angle(direction, azimuth) * side;
    let leaning = (direction + outward * SPLAY).normalize();
    (leaning, perpendicular(leaning, outward))
}

//! Meadows: a rhizome on the bottom bearing ramets, clonal shoots, at its nodes.
//!
//! Seagrasses and many freshwater plants spread by a rhizome or runner that creeps on
//! or just under the bottom and gives off a shoot at its nodes; a meadow is many such
//! ramets of one clone, joined by their rhizome. Here the rhizome is laid out first, on
//! a flat bottom:
//!
//! * `axes` rhizomes leave the base of the plant, evenly spread around it;
//! * every growth cycle each rhizome adds `extension` nodes, `spacing` meters apart,
//!   turning at each node by a random angle of at most `wander_deg`;
//! * a new node starts a lateral rhizome with `branch_probability`, at
//!   `branch_angle_deg` to the left or the right, up to `max_order`; it grows from the
//!   next cycle;
//! * growth stops at `max_ramets` nodes, the base included.
//!
//! Then every node bears a ramet: the species without its rhizome, grown with its own
//! seed for the cycles left after its node appeared, as a single aquatic plant
//! ([`super::dress`] included), and moved to its node. Ramets near the tips of the
//! rhizome are therefore younger, as at the front of a spreading clone. Ramets are
//! grown one after the other on the calling thread (a meadow is one job of a batch or
//! of a forest library, which are the parallel units), in node order; they do not shade
//! each other and do not see the obstacles of the surroundings.
//!
//! In the joined graph the rhizomes are axes of the plant (a rhizome leaving the base
//! is a stem, a lateral rhizome a lateral of its parent), sunk into the bottom by
//! `burial` of their radius, and every stem of a ramet is a relay of the rhizome node
//! that bears it, so its order is that of the rhizome. The births of a ramet are counted
//! from the birth of its node, so growth frames show the meadow spreading.

use super::grow_single;
use crate::GrowParams;
use crate::competition::EngineOptions;
use crate::math::UP;
use crate::surroundings::Surroundings;
use arbrume_core::graph::{ChainKind, Metamer, NodeId, OrganSite, PlantGraph};
use arbrume_core::rng::{Rng, Stream, mix64};
use arbrume_core::species::{Rhizome, Species};
use glam::{Quat, Vec3};
use std::f32::consts::TAU;

/// Distance in rhizome radii from the base of the plant at which a rhizome leaving it
/// starts, so that the closed feet of several rhizomes do not meet.
const FOOT_OFFSET: f32 = 2.5;

/// Stream of the layout of the rhizome.
const LAYOUT: Stream = Stream::custom(0x5A03);

/// One node of the rhizome.
#[derive(Debug, Clone, Copy)]
struct RhizomeNode {
    position: Vec3,
    birth: u16,
}

/// One rhizome axis.
#[derive(Debug, Clone)]
struct RhizomeAxis {
    /// Order of the axis, 0 for a rhizome leaving the base.
    order: u32,
    /// Rhizome node it leaves, `None` for a rhizome leaving the base.
    attach: Option<usize>,
    /// Current heading, horizontal.
    heading: Vec3,
    /// Its nodes, base to tip.
    nodes: Vec<usize>,
    /// First cycle in which it grows.
    start: u16,
}

/// The rhizome of a meadow.
#[derive(Debug, Clone, Default)]
struct Layout {
    nodes: Vec<RhizomeNode>,
    axes: Vec<RhizomeAxis>,
}

impl Layout {
    /// Lays out the rhizome of `rhizome` grown for `age` cycles at `scale`.
    fn new(rhizome: &Rhizome, seed: u64, age: u16, scale: f32) -> Self {
        let mut layout = Self::default();
        let mut rng = Rng::new(seed, 0, LAYOUT);
        let height = -rhizome.burial * rhizome.radius * scale;
        for index in 0..rhizome.axes {
            let azimuth = TAU * (index as f32 + 0.3 * rng.signed()) / rhizome.axes as f32;
            layout.axes.push(RhizomeAxis {
                order: 0,
                attach: None,
                heading: Vec3::new(azimuth.cos(), 0.0, azimuth.sin()),
                nodes: Vec::new(),
                start: 0,
            });
        }
        let base = Vec3::new(0.0, height, 0.0);
        let room = |layout: &Self| {
            let most = rhizome.max_ramets as usize;
            most.saturating_sub(layout.nodes.len() + 1)
        };
        for cycle in 0..age {
            let growing: Vec<usize> = (0..layout.axes.len())
                .filter(|&axis| layout.axes[axis].start <= cycle)
                .collect();
            for axis in growing {
                let steps = room(&layout).min(rhizome.extension as usize);
                for _ in 0..steps {
                    layout.extend(axis, rhizome, (base, scale, cycle), seed);
                }
            }
        }
        layout
    }

    /// Adds one node at the tip of rhizome `axis`, and maybe a lateral rhizome there.
    fn extend(
        &mut self,
        axis: usize,
        rhizome: &Rhizome,
        (base, scale, cycle): (Vec3, f32, u16),
        seed: u64,
    ) {
        let index = self.axes[axis].nodes.len() as u64;
        let mut rng = Rng::at(seed, axis as u32 + 1, LAYOUT, 4 * index);
        let turn = rhizome.wander_deg.to_radians() * rng.signed();
        let heading = Quat::from_axis_angle(UP, turn) * self.axes[axis].heading;
        let from = match (self.axes[axis].nodes.last(), self.axes[axis].attach) {
            (Some(&last), _) | (None, Some(last)) => self.nodes[last].position,
            (None, None) => base,
        };
        let node = self.nodes.len();
        self.nodes.push(RhizomeNode {
            position: from + heading * (rhizome.spacing * scale),
            birth: cycle,
        });
        let current = &mut self.axes[axis];
        current.heading = heading;
        current.nodes.push(node);
        let order = current.order;
        if order < rhizome.max_order && rng.chance(rhizome.branch_probability) {
            let side = if rng.chance(0.5) { 1.0 } else { -1.0 };
            let angle = side * rhizome.branch_angle_deg.to_radians();
            self.axes.push(RhizomeAxis {
                order: order + 1,
                attach: Some(node),
                heading: Quat::from_axis_angle(UP, angle) * heading,
                nodes: Vec::new(),
                start: cycle + 1,
            });
        }
    }
}

/// Copies `ramet` into `graph`, moved by `offset`, its births delayed by `delay`. Its
/// stems become relays of `bearer`, or stems of `graph` when `bearer` is `None`.
fn graft_ramet(
    graph: &mut PlantGraph,
    ramet: &PlantGraph,
    bearer: Option<NodeId>,
    (offset, delay): (Vec3, u16),
) {
    let mut copy = vec![NodeId::NONE; ramet.node_count()];
    for node in ramet.node_ids() {
        let chain = ramet.chain(ramet.chain_of(node));
        let metamer = Metamer {
            position: ramet.positions()[node.index()] + offset,
            radius: ramet.radii()[node.index()],
            birth: ramet.births()[node.index()] + delay,
            flags: ramet.flags()[node.index()],
        };
        let parent = ramet.parents()[node.index()];
        copy[node.index()] = if node != chain.first {
            graph.extend_chain(graph.chain_of(copy[parent.index()]), metamer)
        } else if chain.attach.is_none() {
            match bearer {
                Some(bearer) => graph.add_chain(bearer, ChainKind::Relay, metamer),
                None => graph.add_root_chain(chain.origin + offset, metamer),
            }
        } else {
            let same = ramet.orders()[chain.attach.index()] == chain.order;
            let kind = if same {
                ChainKind::Relay
            } else {
                ChainKind::Lateral
            };
            graph.add_chain(copy[chain.attach.index()], kind, metamer)
        };
    }
    for site in ramet.organs() {
        graph.add_organ(OrganSite {
            node: copy[site.node.index()],
            birth: site.birth + delay,
            ..*site
        });
    }
}

/// Adds the rhizome of `layout` to `graph`; returns the graph node of each rhizome node.
fn add_rhizome(graph: &mut PlantGraph, layout: &Layout, radius: f32) -> Vec<NodeId> {
    let mut ids = vec![NodeId::NONE; layout.nodes.len()];
    for axis in &layout.axes {
        let Some((&first, rest)) = axis.nodes.split_first() else {
            continue;
        };
        let start = &layout.nodes[first];
        let metamer = Metamer::new(start.position, radius, start.birth);
        ids[first] = if let Some(attach) = axis.attach {
            graph.add_chain(ids[attach], ChainKind::Lateral, metamer)
        } else {
            let out = Vec3::new(start.position.x, 0.0, start.position.z).normalize_or_zero();
            let base = out * (FOOT_OFFSET * radius) + Vec3::Y * start.position.y;
            graph.add_root_chain(base, metamer)
        };
        let chain = graph.chain_of(ids[first]);
        for &node in rest {
            let point = &layout.nodes[node];
            ids[node] =
                graph.extend_chain(chain, Metamer::new(point.position, radius, point.birth));
        }
    }
    ids
}

/// Grows the meadow of `species`, which must have an `[aquatic.rhizome]` layer. See
/// the [module documentation](self).
#[must_use]
pub fn grow_meadow(
    species: &Species,
    params: &GrowParams,
    surroundings: &Surroundings,
    options: EngineOptions,
) -> PlantGraph {
    let Some(rhizome) = species.aquatic.as_ref().and_then(|a| a.rhizome.clone()) else {
        return grow_single(species, params, surroundings, options);
    };
    let layout = Layout::new(&rhizome, params.seed, params.age, params.scale);
    let mut shoot = species.clone();
    if let Some(aquatic) = &mut shoot.aquatic {
        aquatic.rhizome = None;
    }
    let water_only = Surroundings {
        water: surroundings.water,
        ..Surroundings::none()
    };
    let births: Vec<u16> = std::iter::once(0)
        .chain(layout.nodes.iter().map(|node| node.birth))
        .collect();
    let ramets: Vec<PlantGraph> = births
        .iter()
        .enumerate()
        .map(|(index, &birth)| {
            let ramet = GrowParams {
                seed: mix64(params.seed ^ (index as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15)),
                age: params.age.saturating_sub(birth).max(1),
                scale: params.scale,
            };
            grow_single(&shoot, &ramet, &water_only, options)
        })
        .collect();
    let mut graph = PlantGraph::new();
    graft_ramet(&mut graph, &ramets[0], None, (Vec3::ZERO, 0));
    let ids = add_rhizome(&mut graph, &layout, rhizome.radius * params.scale);
    for (index, (node, ramet)) in layout.nodes.iter().zip(&ramets[1..]).enumerate() {
        let offset = Vec3::new(node.position.x, 0.0, node.position.z);
        graft_ramet(&mut graph, ramet, Some(ids[index]), (offset, node.birth));
    }
    graph.ensure_contiguous();
    graph
}

//! Joining a grown root system to its shoot.
//!
//! The root system is grown on its own graph, then copied into the graph of the shoot:
//! each primary root becomes a lateral axis of the first node of a stem, the **collar**,
//! and every root node carries [`NodeFlags::ROOT`]. The mesher then joins a primary root
//! to its stem as it joins any lateral to its parent, with the junctions of either tier.
//!
//! Passes follow on the joined graph.
//!
//! * **Pipe balance.** The root radii of each stem are multiplied by `k^b`, `b` the
//!   `pipe_balance` of the roots and `k` the factor that makes the pipe sum of the
//!   primaries equal that of the shoot at the collar, `r_shoot^n`, for the fine roots
//!   the graph leaves out.
//! * **Pipe model at the collar.** The first internode of a stem, which ends at the
//!   collar, carries the pipes of the shoot and of the roots: its radius becomes
//!   `(r_shoot^n + sum of r_primary^n)^(1/n)`, `n` the `pipe_exponent` of the roots.
//! * **Root flare.** A primary root is widened near the collar by
//!   `1 + flare (1 - s / flare_length)^2`, `s` the arc length from the collar.
//! * **Soil cover.** Every root node is moved down, where needed, so that the root is
//!   under the ground by its own radius. With `exposed > 0` the roots that run out
//!   near the surface are raised instead, close to the collar, so that the part
//!   `exposed` of their diameter shows above the ground; the raise fades with the arc
//!   length from the collar over `exposed_length`, and only acts on roots whose path
//!   from the collar is closer to horizontal than to vertical.
//! * **Seat.** The base of each stem is moved up to one and a half radii under its
//!   collar.

use super::soil::Soil;
use arbrume_core::graph::{ChainKind, Metamer, NodeFlags, NodeId, PlantGraph};
use arbrume_core::species::Roots;
use glam::Vec3;

/// The collar of each stem of `graph`: the first node of every chain that starts on the
/// ground.
#[must_use]
pub(crate) fn collars(graph: &PlantGraph) -> Vec<NodeId> {
    graph
        .chains()
        .iter()
        .filter(|chain| chain.attach.is_none() && !graph.flags()[chain.first.index()].is_root())
        .map(|chain| chain.first)
        .collect()
}

/// Copies `roots` into `graph`, each root chain attached to the collar of `collars`
/// nearest to its origin, every node flagged as a root.
///
/// A primary whose first internode leaves the collar so steeply that it would run out
/// through the base of the stem ([`Seat::runs_out`])
/// starts a chain of its own at the collar instead: a junction joins a lateral to the side
/// of its parent, and such a root continues the stem downwards.
pub(crate) fn graft(graph: &mut PlantGraph, roots: &PlantGraph, collars: &[NodeId], seat: &Seat) {
    let mut copy = vec![NodeId::NONE; roots.node_count()];
    for node in roots.node_ids() {
        let chain = roots.chain(roots.chain_of(node));
        let parent = roots.parents()[node.index()];
        let mut metamer = Metamer {
            position: roots.positions()[node.index()],
            radius: roots.radii()[node.index()],
            birth: roots.births()[node.index()],
            flags: roots.flags()[node.index()].with(NodeFlags::ROOT),
        };
        let new = if node != chain.first {
            graph.extend_chain(graph.chain_of(copy[parent.index()]), metamer)
        } else if chain.attach.is_none() {
            let collar = nearest(graph, collars, chain.origin);
            metamer.birth = metamer.birth.max(graph.births()[collar.index()]);
            let offset = metamer.position - chain.origin;
            let stem = seat.radius(collar);
            if seat.runs_out(offset, metamer.radius, stem) {
                // Starts a little along the root, so that its closed base shares no vertex
                // with the stem it starts in.
                let start = graph.positions()[collar.index()] + offset * 0.05;
                graph.add_root_chain(start, metamer)
            } else {
                graph.add_chain(collar, ChainKind::Lateral, metamer)
            }
        } else {
            let same = roots.orders()[chain.attach.index()] == chain.order;
            let kind = if same {
                ChainKind::Relay
            } else {
                ChainKind::Lateral
            };
            graph.add_chain(copy[chain.attach.index()], kind, metamer)
        };
        copy[node.index()] = new;
    }
}

/// The node of `collars` nearest to `point`.
fn nearest(graph: &PlantGraph, collars: &[NodeId], point: Vec3) -> NodeId {
    let distance = |node: &&NodeId| graph.positions()[node.index()].distance_squared(point);
    collars
        .iter()
        .min_by(|a, b| distance(a).total_cmp(&distance(b)))
        .copied()
        .unwrap_or(NodeId::NONE)
}

/// Pipe sum `sum of r^n` of the primary roots of each collar.
fn pipe_sums(
    graph: &PlantGraph,
    primaries: &[(NodeId, NodeId)],
    exponent: f32,
) -> Vec<(NodeId, f32)> {
    let mut sums: Vec<(NodeId, f32)> = Vec::new();
    for &(first, collar) in primaries {
        let pipes = graph.radii()[first.index()].powf(exponent);
        match sums.iter_mut().find(|(node, _)| *node == collar) {
            Some((_, sum)) => *sum += pipes,
            None => sums.push((collar, pipes)),
        }
    }
    sums
}

/// Flags every node of the root system `system` as a root.
pub(crate) fn mark(system: &mut PlantGraph) {
    for flags in system.flags_mut() {
        *flags = flags.with(NodeFlags::ROOT);
    }
}

/// The first node of each primary root of the root system `system`, with the collar of
/// `collars` (nodes of `shoot`) nearest to its origin.
fn primaries_of(
    system: &PlantGraph,
    shoot: &PlantGraph,
    collars: &[NodeId],
) -> Vec<(NodeId, NodeId)> {
    let primaries = system
        .chains()
        .iter()
        .filter(|chain| chain.attach.is_none());
    primaries
        .map(|chain| (chain.first, nearest(shoot, collars, chain.origin)))
        .collect()
}

/// Multiplies the radii of the root system `system` by `k^balance`, `k` computed for
/// each collar of `collars` (nodes of `shoot`). See the [module documentation](self).
pub(crate) fn balance(
    system: &mut PlantGraph,
    shoot: &PlantGraph,
    collars: &[NodeId],
    (exponent, balance): (f32, f32),
) {
    if balance <= 0.0 {
        return;
    }
    let primaries = primaries_of(system, shoot, collars);
    let factors: Vec<(NodeId, f32)> = pipe_sums(system, &primaries, exponent)
        .into_iter()
        .filter(|&(_, pipes)| pipes > 0.0)
        .map(|(collar, pipes)| {
            let stem = shoot.radii()[collar.index()].powf(exponent);
            (collar, (stem / pipes).powf(balance / exponent))
        })
        .collect();
    let mut owner = vec![NodeId::NONE; system.node_count()];
    for &(first, collar) in &primaries {
        owner[first.index()] = collar;
    }
    for node in system.node_ids() {
        let parent = system.parents()[node.index()];
        if owner[node.index()].is_none() && parent.is_some() {
            owner[node.index()] = owner[parent.index()];
        }
    }
    let radii = system.radii_mut();
    for (radius, collar) in radii.iter_mut().zip(&owner) {
        if let Some(&(_, factor)) = factors.iter().find(|(node, _)| node == collar) {
            *radius *= factor;
        }
    }
}

/// Pipe sum of the primary roots of the root system `system` for each collar of
/// `collars` (nodes of `shoot`).
pub(crate) fn collar_pipes(
    system: &PlantGraph,
    shoot: &PlantGraph,
    collars: &[NodeId],
    exponent: f32,
) -> Vec<(NodeId, f32)> {
    pipe_sums(system, &primaries_of(system, shoot, collars), exponent)
}

/// The radius of the first internode of each stem once [`couple_radii`] has set it:
/// `(r_shoot^n + sum)^(1/n)` for each collar and pipe sum of `sums`.
pub(crate) fn coupled(
    graph: &PlantGraph,
    sums: &[(NodeId, f32)],
    exponent: f32,
) -> Vec<(NodeId, f32)> {
    let coupled = |&(collar, pipes): &(NodeId, f32)| {
        let shoot = graph.radii()[collar.index()].powf(exponent);
        (collar, (shoot + pipes).powf(exponent.recip()))
    };
    sums.iter().map(coupled).collect()
}

/// Sets the radius of the first internode of each stem to the pipe sum of the shoot
/// and of the primary roots it bears, `sums` (see [`collar_pipes`]). See the [module
/// documentation](self).
pub(crate) fn couple_radii(graph: &mut PlantGraph, sums: &[(NodeId, f32)], exponent: f32) {
    let radii = graph.radii_mut();
    for &(collar, pipes) in sums {
        let shoot = radii[collar.index()].powf(exponent);
        radii[collar.index()] = (shoot + pipes).powf(exponent.recip());
    }
}

/// Where a root node of a graph starts from.
#[derive(Debug, Clone, Copy, PartialEq)]
struct CollarPath {
    /// Arc length from the collar along the roots.
    arc: f32,
    /// Position of the collar.
    at: Vec3,
    /// The collar node: the first node of a stem.
    collar: NodeId,
}

/// The [`CollarPath`] of every node of `graph`; zero for the nodes above the collar.
fn collar_paths(graph: &PlantGraph) -> Vec<CollarPath> {
    let none = CollarPath {
        arc: 0.0,
        at: Vec3::ZERO,
        collar: NodeId::NONE,
    };
    let mut paths = vec![none; graph.node_count()];
    let stems = collars(graph);
    let (flags, parents) = (graph.flags(), graph.parents());
    for node in graph
        .node_ids()
        .filter(|node| flags[node.index()].is_root())
    {
        let parent = parents[node.index()];
        let below = match parent {
            parent if parent.is_none() => CollarPath {
                at: graph.proximal(node),
                collar: nearest(graph, &stems, graph.proximal(node)),
                ..none
            },
            parent if flags[parent.index()].is_root() => paths[parent.index()],
            parent => CollarPath {
                at: graph.positions()[parent.index()],
                collar: parent,
                ..none
            },
        };
        paths[node.index()] = CollarPath {
            arc: below.arc + graph.internode_length(node),
            ..below
        };
    }
    paths
}

/// Widens each primary root of the root system `system` near the collar. See the
/// [module documentation](self).
pub(crate) fn flare(system: &mut PlantGraph, roots: &Roots, scale: f32) {
    let length = roots.flare_length * scale;
    if roots.flare <= 0.0 || length <= 0.0 {
        return;
    }
    let paths = collar_paths(system);
    let firsts: Vec<NodeId> = system
        .chains()
        .iter()
        .filter(|chain| chain.attach.is_none())
        .map(|chain| chain.first)
        .collect();
    for first in firsts {
        let chain = system.chain_of(first);
        let nodes: Vec<NodeId> = system.chain_nodes_rev(chain).collect();
        let radii = system.radii_mut();
        for node in nodes {
            let fade = (1.0 - paths[node.index()].arc / length).max(0.0);
            radii[node.index()] *= roots.flare.mul_add(fade * fade, 1.0);
        }
    }
}

/// Moves the root nodes under the ground, or raises the exposed ones. See the [module
/// documentation](self).
pub(crate) fn cover(graph: &mut PlantGraph, soil: &Soil, roots: &Roots, scale: f32) {
    let paths = collar_paths(graph);
    let reach = roots.exposed_length * scale;
    for node in graph.node_ids() {
        let index = node.index();
        if !graph.flags()[index].is_root() {
            continue;
        }
        let radius = graph.radii()[index];
        let point = graph.positions()[index];
        let ground = soil.ground.height(point.x, point.z);
        let buried = point.y.min(ground - radius);
        let weight = if roots.exposed > 0.0 {
            let path = paths[index];
            exposure(point - path.at, path.arc, reach)
        } else {
            0.0
        };
        let raised = ground + radius * 2.0f32.mul_add(roots.exposed, -1.0);
        graph.positions_mut()[index].y = buried + (raised - buried) * weight;
    }
}

/// Weight of the exposure of a root node `offset` from its collar and `arc` from it
/// along the root: 1 at the collar, fading as the square of `arc / reach`, and 0 for a
/// path from the collar closer to vertical than to horizontal.
fn exposure(offset: Vec3, arc: f32, reach: f32) -> f32 {
    if reach <= 0.0 || arc >= reach || arc <= 0.0 {
        return 0.0;
    }
    let horizontal = Vec3::new(offset.x, 0.0, offset.z).length() / arc;
    let flat = ((horizontal - 0.5) / 0.4).clamp(0.0, 1.0);
    let fade = 1.0 - arc / reach;
    fade * fade * flat * flat * 2.0f32.mul_add(-flat, 3.0)
}

/// Length of the base of a stem under its collar, in radii of that base.
pub(crate) const SEAT_DEPTH: f32 = 1.5;

/// Part of [`SEAT_DEPTH`] kept clear above the foot of a stem by the junctions of its
/// primary roots, in radii of the base.
pub(crate) const SEAT_MARGIN: f32 = 0.25;

/// The shape of the base of the stems of a plant, which decides how [`graft`] joins a
/// primary root.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Seat {
    /// Trunk flare of the species: the base of a stem widens linearly by `flare` from
    /// its foot up to its collar.
    pub(crate) flare: f32,
    /// Widening of a lateral at its junction, as a fraction of its radius.
    pub(crate) collar_flare: f32,
    /// Radius of the base of each stem, by collar.
    pub(crate) radii: Vec<(NodeId, f32)>,
}

impl Seat {
    /// Whether a primary root of radius `radius` whose first internode leaves the collar
    /// of a stem of base radius `stem` along `offset` comes out of the base of the stem
    /// less than [`SEAT_MARGIN`] radii above its foot, or through the foot: where the axis
    /// of the root crosses the widening base, plus the reach of its own collar along the
    /// stem.
    fn runs_out(&self, offset: Vec3, radius: f32, stem: f32) -> bool {
        let depth = -offset.y;
        if depth <= 0.0 {
            return false;
        }
        let run = Vec3::new(offset.x, 0.0, offset.z).length() / depth;
        if run <= 0.0 {
            return true;
        }
        let exit = stem * (1.0 + self.flare) / (run + self.flare / SEAT_DEPTH);
        let side = radius * (1.0 + self.collar_flare) * run.hypot(1.0) / run;
        exit + side > (SEAT_DEPTH - SEAT_MARGIN) * stem
    }

    /// Radius of the base of the stem of `collar`.
    fn radius(&self, collar: NodeId) -> f32 {
        self.radii
            .iter()
            .find(|(node, _)| *node == collar)
            .map_or(0.0, |&(_, radius)| radius)
    }
}

/// Moves the base of each stem up to [`SEAT_DEPTH`] collar radii under its collar, so
/// that the stem meets the roots at the ground instead of standing on its first
/// internode.
pub(crate) fn seat(graph: &mut PlantGraph, collars: &[NodeId]) {
    for &collar in collars {
        let depth = SEAT_DEPTH * graph.radii()[collar.index()];
        let origin = graph.positions()[collar.index()] - Vec3::Y * depth;
        let stem = graph.chain_of(collar);
        graph.set_root_origin(stem, origin);
    }
}

/// Sheds the root laterals that leave their parent inside a stem: within the flared
/// radius of the stem base (`flare` being the trunk flare of the species), plus the
/// radius of the parent, from the axis of the stem, above the base of the stem
/// ([`SEAT_DEPTH`]). Such a lateral would start inside
/// the wood of the stem. The laterals borne by the first node of a root are shed too: they
/// would leave it inside its own junction. Shed, they stay in the graph and in the pipe
/// sums.
pub(crate) fn clear_stems(graph: &mut PlantGraph, flare: f32) {
    let paths = collar_paths(graph);
    let flags = graph.flags();
    let mut shed = Vec::new();
    for chain in graph.chains() {
        let attach = chain.attach;
        if attach.is_none() || !flags[attach.index()].is_root() {
            continue;
        }
        let path = paths[attach.index()];
        if path.collar.is_none() {
            continue;
        }
        let on_first = graph.chain(graph.chain_of(attach)).first == attach;
        let stem = graph.radii()[path.collar.index()];
        let offset = graph.positions()[attach.index()] - path.at;
        let across = Vec3::new(offset.x, 0.0, offset.z).length();
        let margin = graph.radii()[attach.index()];
        let depth = -offset.y / (SEAT_DEPTH * stem);
        let surface = stem * flare.mul_add(1.0 - depth, 1.0);
        let inside = (0.0..1.0).contains(&depth) && across < surface + margin;
        if on_first || inside {
            shed.push(chain.first);
        }
    }
    graph.shed_subtrees(&shed, 0);
}

//! Geometric realism features of a plant skeleton: the feature set of `ICTree`.
//!
//! Polasek et al. (2021) learned a perceptual realism metric for tree models from the
//! judgements of about 4,000 people. Its feature-based predictor reads a set of
//! geometric features computed on the skeleton (their Table 2); a random forest fitted
//! on those features found the **sibling angles** and the **segment volume** the most
//! important, and chain deformation and chain segment count the least. This crate
//! computes those features from a [`PlantGraph`] so that each species can be tracked
//! over time. It does not use the trained `ICTree` predictor: the numbers are a
//! regression baseline to compare versions of a species against, not a realism score.
//!
//! # Definitions
//!
//! The skeleton is the live part of the graph (shed nodes are left out). Following
//! `ICTree`, a **segment** is one internode, and a **chain** is a maximal run of segments
//! from the root or a branching node (a node with two or more live children) to the
//! next branching node or a tip. `ICTree` chains are therefore shorter than the axes of
//! the graph: an axis that bears laterals is cut at each of them. Directions: `f` is the
//! first segment of a chain, `e` its last, `F` its chord (start to end), `up` is +Y.
//! Angles are in degrees, lengths in meters, volumes in cubic meters.
//!
//! * Segment: width `d` (diameter, twice the internode radius) and volume `V` (the
//!   frustum between the radius of the previous internode of the same axis, or its own
//!   radius for the first internode of an axis, and its own radius).
//! * Chain: segments `S`, depth `h` (1 for a chain starting at the root), length `l`,
//!   total length `L = |F|`, deformation `sigma` (sum of the angles between successive
//!   segments), straightness `tau = L / l`, slope `angle(f, up)`, smallest and largest
//!   width, tapering `d_min / d_max`, internodes `I = l / i_mu`, and tropisms
//!   `zeta_b = angle(f, e)`, `zeta_h` and `zeta_p = angle(e, F)`. The paper prints
//!   `zeta_h = angle(f, up)`, the same as the slope; it is read here as `angle(e, up)`.
//! * Sibling pairs (two chains starting at the same branching node): incidence angle
//!   `alpha = angle(f1, f2)` and total angle `beta = angle(e1, e2)`, and per branching
//!   node the smallest and largest of each (`A_m`, `A_M`, `B_m`, `B_M`).
//! * Parent-child pairs: length ratio `l_child / l_parent`, angle-sum difference
//!   `sigma_parent - sigma_child`, continuation `angle(e_parent, f_child)` and
//!   preservation `angle(F_parent, F_child)`, and per parent their smallest and largest.
//! * Branching asymmetry, per sibling pair of subtrees: `|a - b| / (a + b)` for the
//!   number of tips, the total length and the total volume. The paper multiplies the
//!   difference by an unspecified normalisation constant `chi`; the sum is used here.
//! * Global: `i_mu` and `i_var`, mean and variance of the chain length (the paper's
//!   estimate of the internode length), trunk length `l_t` (the longest chain starting at
//!   the root), tip count `c_l`, and age `l_t / i_mu + max(h) - 1`; plus the segment and
//!   chain counts, the total wood volume and the mean internode length, which are not in
//!   the paper.
//!
//! Each local feature is summarised by its count, mean, variance, minimum and maximum;
//! the histograms of the paper, the branching-model and ramification-model fitness
//! (`lambda`, `mu`) and the rendered volume imprint are not computed.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use arbrume_core::graph::{NodeId, PlantGraph};
use glam::Vec3;
use serde::Serialize;

/// Count, mean, variance, minimum and maximum of one local feature.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Summary {
    /// Number of values.
    pub count: usize,
    /// Mean; 0 when there is no value.
    pub mean: f64,
    /// Population variance; 0 when there is no value.
    pub var: f64,
    /// Smallest value; 0 when there is no value.
    pub min: f64,
    /// Largest value; 0 when there is no value.
    pub max: f64,
}

/// Running sums of one feature.
#[derive(Debug, Clone, Copy)]
struct Accumulator {
    count: usize,
    sum: f64,
    squares: f64,
    min: f64,
    max: f64,
}

impl Default for Accumulator {
    fn default() -> Self {
        Self {
            count: 0,
            sum: 0.0,
            squares: 0.0,
            min: f64::INFINITY,
            max: f64::NEG_INFINITY,
        }
    }
}

impl Accumulator {
    fn add(&mut self, value: f64) {
        if value.is_finite() {
            self.count += 1;
            self.sum += value;
            self.squares += value * value;
            self.min = self.min.min(value);
            self.max = self.max.max(value);
        }
    }

    fn summary(&self) -> Summary {
        if self.count == 0 {
            return Summary {
                count: 0,
                mean: 0.0,
                var: 0.0,
                min: 0.0,
                max: 0.0,
            };
        }
        let n = self.count as f64;
        let mean = self.sum / n;
        Summary {
            count: self.count,
            mean,
            var: (self.squares / n - mean * mean).max(0.0),
            min: self.min,
            max: self.max,
        }
    }
}

/// The features of one plant. Keys are listed in [`LOCAL_FEATURES`] and
/// [`GLOBAL_FEATURES`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Features {
    /// Summary of each local feature.
    pub local: BTreeMap<&'static str, Summary>,
    /// Value of each global feature.
    pub global: BTreeMap<&'static str, f64>,
}

/// Names of the local features, with their `ICTree` symbol in the documentation.
pub const LOCAL_FEATURES: [&str; 29] = [
    "segment_width",
    "segment_volume",
    "chain_segments",
    "chain_depth",
    "chain_length",
    "chain_total_length",
    "chain_deformation",
    "chain_straightness",
    "chain_slope",
    "chain_width_min",
    "chain_width_max",
    "chain_tapering",
    "chain_internodes",
    "chain_tropism_b",
    "chain_tropism_h",
    "chain_tropism_p",
    "sibling_angle",
    "sibling_total_angle",
    "sibling_angle_min",
    "sibling_angle_max",
    "sibling_total_angle_min",
    "sibling_total_angle_max",
    "parent_length_ratio",
    "parent_angle_sum",
    "parent_continuation",
    "parent_preservation",
    "asymmetry_tips",
    "asymmetry_length",
    "asymmetry_volume",
];

/// Names of the global features.
pub const GLOBAL_FEATURES: [&str; 9] = [
    "internode_mean",
    "internode_var",
    "trunk_length",
    "tip_count",
    "age",
    "segment_count",
    "chain_count",
    "total_volume",
    "segment_length_mean",
];

/// One `ICTree` chain and its measures.
#[derive(Debug, Clone)]
struct Chain {
    /// Segments (nodes of the graph), base to tip.
    segments: Vec<NodeId>,
    parent: Option<usize>,
    depth: u32,
    length: f32,
    first: Vec3,
    last: Vec3,
    chord: Vec3,
    deformation: f32,
    width_min: f32,
    width_max: f32,
    volume: f32,
    /// Tips, length and volume of the chain and everything above it.
    subtree: [f64; 3],
    children: Vec<usize>,
}

/// Angle between two vectors in degrees, `None` when one is zero.
fn angle(a: Vec3, b: Vec3) -> Option<f64> {
    let (a, b) = (a.try_normalize()?, b.try_normalize()?);
    Some(f64::from(a.dot(b).clamp(-1.0, 1.0).acos().to_degrees()))
}

/// Volume of the frustum of the internode of `node`.
fn segment_volume(graph: &PlantGraph, node: NodeId) -> f32 {
    let radius = graph.radii()[node.index()];
    let proximal = graph
        .parent(node)
        .filter(|parent| graph.chain_of(*parent) == graph.chain_of(node))
        .map_or(radius, |parent| graph.radii()[parent.index()]);
    let height = graph.internode_length(node);
    std::f32::consts::PI * height / 3.0
        * (proximal * proximal + proximal * radius + radius * radius)
}

/// Builds the `ICTree` chains of the live part of `graph`, parents before children.
fn chains(graph: &PlantGraph) -> Vec<Chain> {
    let children = graph.children();
    let live = |node: &&NodeId| !graph.is_shed(**node);
    let only_child = |node: NodeId| {
        let mut live_children = children.of(node).iter().filter(live);
        match (live_children.next(), live_children.next()) {
            (Some(&child), None) => Some(child),
            _ => None,
        }
    };
    let mut stack: Vec<(NodeId, Option<usize>)> = graph
        .node_ids()
        .filter(|&node| graph.parent(node).is_none() && !graph.is_shed(node))
        .map(|node| (node, None))
        .rev()
        .collect();
    let mut out: Vec<Chain> = Vec::new();
    while let Some((start, parent)) = stack.pop() {
        let mut segments = vec![start];
        while let Some(child) = only_child(segments[segments.len() - 1]) {
            segments.push(child);
        }
        let end = segments[segments.len() - 1];
        let index = out.len();
        let depth = parent.map_or(1, |p| out[p].depth + 1);
        if let Some(p) = parent {
            out[p].children.push(index);
        }
        out.push(measure(graph, segments, parent, depth));
        let next = children.of(end).iter().filter(live).rev();
        stack.extend(next.map(|&child| (child, Some(index))));
    }
    for index in (0..out.len()).rev() {
        let own = [
            if out[index].children.is_empty() {
                1.0
            } else {
                0.0
            },
            f64::from(out[index].length),
            f64::from(out[index].volume),
        ];
        let below = out[index].children.iter().fold(own, |sum, &child| {
            std::array::from_fn(|k| sum[k] + out[child].subtree[k])
        });
        out[index].subtree = below;
    }
    out
}

/// Measures of one chain made of `segments`.
fn measure(graph: &PlantGraph, segments: Vec<NodeId>, parent: Option<usize>, depth: u32) -> Chain {
    let direction = |node: NodeId| graph.position(node) - graph.proximal(node);
    let mut chain = Chain {
        parent,
        depth,
        length: 0.0,
        first: direction(segments[0]),
        last: direction(segments[segments.len() - 1]),
        chord: graph.position(segments[segments.len() - 1]) - graph.proximal(segments[0]),
        deformation: 0.0,
        width_min: f32::INFINITY,
        width_max: 0.0,
        volume: 0.0,
        subtree: [0.0; 3],
        children: Vec::new(),
        segments: Vec::new(),
    };
    let mut previous: Option<Vec3> = None;
    for &node in &segments {
        let step = direction(node);
        chain.length += step.length();
        if let Some(turn) = previous.and_then(|before| angle(before, step)) {
            chain.deformation += turn as f32;
        }
        previous = Some(step);
        let width = 2.0 * graph.radii()[node.index()];
        chain.width_min = chain.width_min.min(width);
        chain.width_max = chain.width_max.max(width);
        chain.volume += segment_volume(graph, node);
    }
    chain.segments = segments;
    chain
}

/// Index of each local feature in [`LOCAL_FEATURES`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Local {
    SegmentWidth,
    SegmentVolume,
    ChainSegments,
    ChainDepth,
    ChainLength,
    ChainTotalLength,
    ChainDeformation,
    ChainStraightness,
    ChainSlope,
    ChainWidthMin,
    ChainWidthMax,
    ChainTapering,
    ChainInternodes,
    ChainTropismB,
    ChainTropismH,
    ChainTropismP,
    SiblingAngle,
    SiblingTotalAngle,
    SiblingAngleMin,
    SiblingAngleMax,
    SiblingTotalAngleMin,
    SiblingTotalAngleMax,
    ParentLengthRatio,
    ParentAngleSum,
    ParentContinuation,
    ParentPreservation,
    AsymmetryTips,
    AsymmetryLength,
    AsymmetryVolume,
}

/// Accumulators of every local feature, indexed by [`Local`].
#[derive(Debug)]
struct Sums([Accumulator; LOCAL_FEATURES.len()]);

impl Sums {
    fn new() -> Self {
        Self([Accumulator::default(); LOCAL_FEATURES.len()])
    }

    fn add(&mut self, feature: Local, value: f64) {
        self.0[feature as usize].add(value);
    }

    fn add_angle(&mut self, feature: Local, value: Option<f64>) {
        if let Some(value) = value {
            self.add(feature, value);
        }
    }
}
/// Computes the features of the live part of `graph`.
#[must_use]
pub fn features(graph: &PlantGraph) -> Features {
    let chains = chains(graph);
    let mut sums = Sums::new();
    let mut lengths = Accumulator::default();
    for chain in &chains {
        lengths.add(f64::from(chain.length));
    }
    let internode = lengths.summary();
    for chain in &chains {
        chain_features(graph, chain, internode.mean, &mut sums);
        sibling_features(&chains, &chain.children, &mut sums);
        parent_features(&chains, chain, &mut sums);
    }
    let local = LOCAL_FEATURES
        .into_iter()
        .zip(&sums.0)
        .map(|(name, sum)| (name, sum.summary()))
        .collect();
    let global = global_features(&chains, internode);
    Features { local, global }
}

fn chain_features(graph: &PlantGraph, chain: &Chain, internode_mean: f64, sums: &mut Sums) {
    for &node in &chain.segments {
        sums.add(
            Local::SegmentWidth,
            f64::from(2.0 * graph.radii()[node.index()]),
        );
        sums.add(Local::SegmentVolume, f64::from(segment_volume(graph, node)));
    }
    let length = f64::from(chain.length);
    sums.add(Local::ChainSegments, chain.segments.len() as f64);
    sums.add(Local::ChainDepth, f64::from(chain.depth));
    sums.add(Local::ChainLength, length);
    sums.add(Local::ChainTotalLength, f64::from(chain.chord.length()));
    sums.add(Local::ChainDeformation, f64::from(chain.deformation));
    if length > 0.0 {
        sums.add(
            Local::ChainStraightness,
            f64::from(chain.chord.length()) / length,
        );
    }
    sums.add_angle(Local::ChainSlope, angle(chain.first, Vec3::Y));
    sums.add(Local::ChainWidthMin, f64::from(chain.width_min));
    sums.add(Local::ChainWidthMax, f64::from(chain.width_max));
    if chain.width_max > 0.0 {
        sums.add(
            Local::ChainTapering,
            f64::from(chain.width_min / chain.width_max),
        );
    }
    if internode_mean > 0.0 {
        sums.add(Local::ChainInternodes, length / internode_mean);
    }
    sums.add_angle(Local::ChainTropismB, angle(chain.first, chain.last));
    sums.add_angle(Local::ChainTropismH, angle(chain.last, Vec3::Y));
    sums.add_angle(Local::ChainTropismP, angle(chain.last, chain.chord));
}

/// Sibling and asymmetry features of the chains `siblings`, which start at one node.
fn sibling_features(chains: &[Chain], siblings: &[usize], sums: &mut Sums) {
    let mut alpha = Accumulator::default();
    let mut beta = Accumulator::default();
    for (k, &one) in siblings.iter().enumerate() {
        for &two in &siblings[k + 1..] {
            let (a, b) = (&chains[one], &chains[two]);
            if let Some(value) = angle(a.first, b.first) {
                sums.add(Local::SiblingAngle, value);
                alpha.add(value);
            }
            if let Some(value) = angle(a.last, b.last) {
                sums.add(Local::SiblingTotalAngle, value);
                beta.add(value);
            }
            let names = [
                Local::AsymmetryTips,
                Local::AsymmetryLength,
                Local::AsymmetryVolume,
            ];
            for (name, (x, y)) in names.into_iter().zip(a.subtree.into_iter().zip(b.subtree)) {
                if x + y > 0.0 {
                    sums.add(name, (x - y).abs() / (x + y));
                }
            }
        }
    }
    if alpha.count > 0 {
        sums.add(Local::SiblingAngleMin, alpha.min);
        sums.add(Local::SiblingAngleMax, alpha.max);
    }
    if beta.count > 0 {
        sums.add(Local::SiblingTotalAngleMin, beta.min);
        sums.add(Local::SiblingTotalAngleMax, beta.max);
    }
}

/// Parent-child features of `chain` and its parent.
fn parent_features(chains: &[Chain], chain: &Chain, sums: &mut Sums) {
    let Some(parent) = chain.parent.map(|p| &chains[p]) else {
        return;
    };
    if parent.length > 0.0 {
        sums.add(
            Local::ParentLengthRatio,
            f64::from(chain.length / parent.length),
        );
    }
    sums.add(
        Local::ParentAngleSum,
        f64::from(parent.deformation - chain.deformation),
    );
    sums.add_angle(Local::ParentContinuation, angle(parent.last, chain.first));
    sums.add_angle(Local::ParentPreservation, angle(parent.chord, chain.chord));
}

fn global_features(chains: &[Chain], internode: Summary) -> BTreeMap<&'static str, f64> {
    let trunk = chains
        .iter()
        .filter(|chain| chain.parent.is_none())
        .map(|chain| f64::from(chain.length))
        .fold(0.0, f64::max);
    let tips = chains.iter().filter(|c| c.children.is_empty()).count();
    let depth = chains.iter().map(|c| c.depth).max().unwrap_or(0);
    let segments: usize = chains.iter().map(|c| c.segments.len()).sum();
    let volume: f64 = chains.iter().map(|c| f64::from(c.volume)).sum();
    let wood: f64 = chains.iter().map(|c| f64::from(c.length)).sum();
    let age = if internode.mean > 0.0 {
        trunk / internode.mean + f64::from(depth) - 1.0
    } else {
        0.0
    };
    let values = [
        internode.mean,
        internode.var,
        trunk,
        tips as f64,
        age,
        segments as f64,
        chains.len() as f64,
        volume,
        if segments > 0 {
            wood / segments as f64
        } else {
            0.0
        },
    ];
    GLOBAL_FEATURES.into_iter().zip(values).collect()
}

#[cfg(test)]
mod tests {
    use super::{LOCAL_FEATURES, features};
    use arbrume_core::graph::{ChainKind, Metamer, PlantGraph};
    use glam::Vec3;

    /// A trunk of two unit internodes along +Y with two laterals at the top of the
    /// first one, along +X and -X.
    fn fork() -> PlantGraph {
        let mut graph = PlantGraph::new();
        let base = graph.add_root_chain(Vec3::ZERO, Metamer::new(Vec3::Y, 0.1, 0));
        graph.extend_chain(graph.chain_of(base), Metamer::new(Vec3::Y * 2.0, 0.1, 1));
        let side = |x: f32| Metamer::new(Vec3::new(x, 1.0, 0.0), 0.05, 1);
        graph.add_chain(base, ChainKind::Lateral, side(1.0));
        graph.add_chain(base, ChainKind::Lateral, side(-1.0));
        graph
    }

    #[test]
    fn a_fork_has_four_chains_and_known_angles() {
        let features = features(&fork());
        assert_eq!(features.global["chain_count"], 4.0);
        assert_eq!(features.global["tip_count"], 3.0);
        assert_eq!(features.global["trunk_length"], 1.0);
        let siblings = features.local["sibling_angle"];
        assert_eq!(siblings.count, 3);
        assert!((siblings.max - 180.0).abs() < 1e-3 && (siblings.min - 90.0).abs() < 1e-3);
        let slope = features.local["chain_slope"];
        assert!((slope.max - 90.0).abs() < 1e-3 && slope.min.abs() < 1e-3);
        let asymmetry = features.local["asymmetry_tips"];
        assert_eq!((asymmetry.count, asymmetry.max), (3, 0.0));
        for name in LOCAL_FEATURES {
            assert!(features.local.contains_key(name), "{name}");
        }
    }

    #[test]
    fn shed_branches_are_left_out() {
        let mut graph = fork();
        graph.shed_subtree(arbrume_core::graph::NodeId::new(2));
        let features = features(&graph);
        assert_eq!(features.global["tip_count"], 2.0);
        assert_eq!(features.local["sibling_angle"].count, 1);
    }

    #[test]
    fn segment_volume_is_a_frustum() {
        let features = features(&fork());
        let volume = features.global["total_volume"];
        let trunk = 2.0 * std::f64::consts::PI * 0.01;
        let laterals = 2.0 * std::f64::consts::PI * 0.0025;
        assert!((volume - trunk - laterals).abs() < 1e-5, "{volume}");
    }
}

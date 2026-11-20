//! A distance between two tree skeletons, after Stava et al. (2014).
//!
//! Stava et al. compare a generated tree to a target by a weighted sum of a shape, a
//! geometry and a structure term. This module keeps that split with terms that need no
//! age, no branching order and no internode structure from the input, so that a scanned
//! skeleton and an Arbrume plant go through the same steps.
//!
//! # Normalisation of a skeleton
//!
//! 1. **Pruning.** A node is kept when its radius and those of all its ancestors are at
//!    least `min_radius`, which removes the small branches a scan gets wrong.
//! 2. **Placement.** The skeleton is moved so that the base (the mean of the roots in X
//!    and Z, their lowest Y) is at the origin; the up axis is +Y.
//! 3. **Axes.** Each root starts an axis of order 0. At every node the axis continues into
//!    the child that carries the most wood (the largest total length of its subtree; ties
//!    go to the straightest continuation, then to the position); every other child
//!    starts an axis of the next order. Orders are thus derived from the geometry alone.
//! 4. **Resampling.** Every axis is resampled at a common arc-length step `step` (`n =
//!    max(1, round(L / step))` equal segments), each segment taking the radius of the
//!    input segment under its middle, and a lateral is attached to the nearest resampled
//!    node of its parent. Without `DistanceOptions::step` the step is the height of the
//!    tallest of the two trees over `steps_per_height`. Optionally `smooth` passes of a
//!    `[1/4, 1/2, 1/4]` filter move the inner points of every axis, against position noise.
//! 5. The result is rebuilt as a `PlantGraph` for the `ICTree` features of
//!    `arbrume-metrics`.
//!
//! # Terms
//!
//! Every term is in `[0, 1]` and symmetric in the two trees. With `H` and `R` the largest
//! height and horizontal reach of the two trees, and the distal points of the resampled
//! segments weighted by their length:
//!
//! * **envelope** (shape): per height bin (`height_bins` over `[0, H]`), the largest
//!   horizontal distance of wood from the vertical through the base, a radial profile;
//!   the term is one minus the intersection over union of the two profiles (the area of
//!   the silhouette swept around the vertical).
//! * **density** (shape): the length of wood in `height_bins` by `radial_bins` cells of
//!   `[0, H] x [0, R]`, normalised to sum 1; the term is the total variation distance.
//! * **geometry**: per order (0, 1, 2, 3 and more) the distributions over axes of the
//!   length, the base radius, the insertion angle (to the parent segment, or to +Y for a
//!   stem) and the inclination of the chord to +Y. Two distributions are compared by the
//!   1-Wasserstein distance, divided by the sum of the two means for lengths and radii
//!   and by 180 degrees for angles; a distribution present in one tree only counts 1.
//!   The term is the mean over pairs present in either tree.
//! * **counts** (structure): per order, `|a - b| / (a + b)` of the numbers of axes.
//! * **features** (structure): the mean over plants of every `ICTree` local feature but
//!   `chain_internodes` (whose mean is 1 by construction), and the global features
//!   `trunk_length`, `tip_count`, `chain_count` and `total_volume`: `|a - b| / 180` for
//!   angles bounded by 180 degrees, `|a - b| / (|a| + |b|)` for the others, averaged.
//!
//! The total is the weighted mean of the five terms. The default [`Weights`] give the
//! shape, geometry and structure groups equal weight, as Stava et al. did in all their
//! results; they are a choice, not a calibration.

use std::cmp::Ordering;

use arbrume_core::graph::{ChainKind, Metamer, NodeId, PlantGraph};
use arbrume_metrics::{Features, features};
use glam::Vec3;
use serde::{Deserialize, Serialize};

use crate::FitError;
use crate::skeleton::{TreeSkeleton, point, signed};

/// Orders compared separately; deeper orders are pooled with the last one.
pub const ORDER_GROUPS: usize = 4;

/// Axis properties compared by the geometry term.
pub const AXIS_PROPERTIES: [&str; 4] = ["length", "base_radius", "insertion_angle", "inclination"];

/// Local `ICTree` features whose values are angles in `[0, 180]` degrees.
const ANGLE_FEATURES: [&str; 12] = [
    "chain_slope",
    "chain_tropism_b",
    "chain_tropism_h",
    "chain_tropism_p",
    "sibling_angle",
    "sibling_total_angle",
    "sibling_angle_min",
    "sibling_angle_max",
    "sibling_total_angle_min",
    "sibling_total_angle_max",
    "parent_continuation",
    "parent_preservation",
];

/// Global `ICTree` features used by the features term.
const GLOBALS: [&str; 4] = ["trunk_length", "tip_count", "chain_count", "total_volume"];

/// Weights of the terms in the total.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Weights {
    /// Radial-profile envelope.
    pub envelope: f64,
    /// Wood density over height and radius.
    pub density: f64,
    /// Per-order distributions of axis properties.
    pub geometry: f64,
    /// Per-order axis counts.
    pub counts: f64,
    /// `ICTree` features.
    pub features: f64,
}

impl Default for Weights {
    /// Shape (envelope and density), geometry and structure (counts and features) with
    /// weight 1 each, split evenly inside a group.
    fn default() -> Self {
        Self {
            envelope: 0.5,
            density: 0.5,
            geometry: 1.0,
            counts: 0.5,
            features: 0.5,
        }
    }
}

/// Settings of the distance.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DistanceOptions {
    /// Nodes thinner than this, in meters, are removed with their subtree. 0 keeps all.
    pub min_radius: f32,
    /// Resampling step in meters; `None` derives it from the height.
    pub step: Option<f32>,
    /// Without `step`: the step is the height of the tallest tree over this.
    pub steps_per_height: f32,
    /// Smoothing passes over the resampled axes.
    pub smooth: u32,
    /// Height bins of the shape terms.
    pub height_bins: usize,
    /// Radial bins of the density term.
    pub radial_bins: usize,
    /// Weights of the terms.
    pub weights: Weights,
}

impl Default for DistanceOptions {
    /// No pruning, 100 steps per height, no smoothing, 12 by 6 bins, default weights.
    /// Settings, not calibrated values.
    fn default() -> Self {
        Self {
            min_radius: 0.0,
            step: None,
            steps_per_height: 100.0,
            smooth: 0,
            height_bins: 12,
            radial_bins: 6,
            weights: Weights::default(),
        }
    }
}

/// The terms of a distance and their weighted total, each in `[0, 1]`.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Terms {
    /// Radial-profile envelope.
    pub envelope: f64,
    /// Wood density over height and radius.
    pub density: f64,
    /// Per-order distributions of axis properties.
    pub geometry: f64,
    /// Per-order axis counts.
    pub counts: f64,
    /// `ICTree` features.
    pub features: f64,
    /// Weighted mean of the terms.
    pub total: f64,
}

impl Terms {
    /// Names of the terms, in the order of [`Terms::values`].
    pub const NAMES: [&str; 6] = [
        "envelope", "density", "geometry", "counts", "features", "total",
    ];

    /// The terms in the order of [`Terms::NAMES`].
    #[must_use]
    pub const fn values(&self) -> [f64; 6] {
        [
            self.envelope,
            self.density,
            self.geometry,
            self.counts,
            self.features,
            self.total,
        ]
    }

    /// Mean of `terms` term by term; zero when empty.
    #[must_use]
    pub fn mean(terms: &[Self]) -> Self {
        let n = terms.len().max(1) as f64;
        let sum = |f: fn(&Self) -> f64| terms.iter().map(f).sum::<f64>() / n;
        Self {
            envelope: sum(|t| t.envelope),
            density: sum(|t| t.density),
            geometry: sum(|t| t.geometry),
            counts: sum(|t| t.counts),
            features: sum(|t| t.features),
            total: sum(|t| t.total),
        }
    }
}

/// A skeleton normalised for comparison: everything the terms read.
#[derive(Debug, Clone, PartialEq)]
pub struct Descriptor {
    /// Resampling step used, meters.
    pub step: f32,
    /// Largest height of wood above the base, meters.
    pub height: f32,
    /// Largest horizontal distance of wood from the base, meters.
    pub reach: f32,
    /// Resampled nodes.
    pub nodes: usize,
    /// Axes per order group.
    pub counts: [usize; ORDER_GROUPS],
    /// Height, horizontal distance and length of every resampled segment.
    samples: Vec<[f32; 3]>,
    /// Sorted values of each axis property, per order group.
    properties: [[Vec<f64>; AXIS_PROPERTIES.len()]; ORDER_GROUPS],
    /// Selected `ICTree` features and whether each is an angle.
    features: Vec<(f64, bool)>,
}

/// One axis of the input skeleton: its nodes, base to tip, and where it starts.
#[derive(Debug, Clone)]
struct Axis {
    /// Input nodes, the first being the node it starts from.
    nodes: Vec<usize>,
    /// Arc length of every node from the start of the axis.
    arc: Vec<f32>,
    /// Parent axis and the index in its `nodes` of the start node.
    parent: Option<(usize, usize)>,
    order: u8,
    children: Vec<usize>,
}

/// Total order on positions, for deterministic tie breaks.
fn position_order(a: [f32; 3], b: [f32; 3]) -> Ordering {
    a[0].total_cmp(&b[0])
        .then(a[1].total_cmp(&b[1]))
        .then(a[2].total_cmp(&b[2]))
}

/// Angle between two vectors in degrees; 0 when one is zero.
fn angle(a: Vec3, b: Vec3) -> f64 {
    match (a.try_normalize(), b.try_normalize()) {
        (Some(a), Some(b)) => f64::from(a.dot(b).clamp(-1.0, 1.0).acos().to_degrees()),
        _ => 0.0,
    }
}

/// Nodes kept by pruning, re-indexed parents first, and moved to put the base at the
/// origin.
fn prune(skeleton: &TreeSkeleton, min_radius: f32) -> TreeSkeleton {
    let mut index = vec![-1_i32; skeleton.len()];
    let mut out = TreeSkeleton::default();
    for node in skeleton.topological_order() {
        let parent = skeleton.parent[node];
        let parent_kept = parent < 0 || index[parent as usize] >= 0;
        if parent_kept && skeleton.radius[node] >= min_radius {
            index[node] = signed(out.len());
            out.position.push(skeleton.position[node]);
            out.radius.push(skeleton.radius[node]);
            out.parent.push(if parent < 0 {
                -1
            } else {
                index[parent as usize]
            });
        }
    }
    let roots: Vec<Vec3> = (0..out.len())
        .filter(|&i| out.parent[i] < 0)
        .map(|i| point(&out, i))
        .collect();
    if let Some(low) = roots.iter().map(|p| p.y).min_by(f32::total_cmp) {
        let mean = roots.iter().copied().sum::<Vec3>() / roots.len() as f32;
        let base = Vec3::new(mean.x, low, mean.z);
        for p in &mut out.position {
            *p = (Vec3::from_array(*p) - base).to_array();
        }
    }
    out
}

/// Total length of the subtree of every node, its own segment included.
fn subtree_lengths(skeleton: &TreeSkeleton) -> Vec<f32> {
    let mut lengths: Vec<f32> = (0..skeleton.len())
        .map(|i| match usize::try_from(skeleton.parent[i]) {
            Ok(parent) => point(skeleton, i).distance(point(skeleton, parent)),
            Err(_) => 0.0,
        })
        .collect();
    for i in (0..skeleton.len()).rev() {
        if let Ok(parent) = usize::try_from(skeleton.parent[i]) {
            lengths[parent] += lengths[i];
        }
    }
    lengths
}

/// Where an axis starts: the node it grows from, its first node, its parent axis with
/// the index of the start node in that axis, and its order.
type Start = (usize, usize, Option<(usize, usize)>, u8);

/// Splits a pruned skeleton (parents before children) into axes, parents first.
fn axes(skeleton: &TreeSkeleton) -> Vec<Axis> {
    let children = skeleton.children();
    let wood = subtree_lengths(skeleton);
    let mut starts: Vec<Start> = (0..skeleton.len())
        .filter(|&root| skeleton.parent[root] < 0)
        .filter_map(|root| {
            let first = continuation(skeleton, &children[root], &wood, root, root)?;
            Some((root, first, None, 0))
        })
        .collect();
    let mut axes: Vec<Axis> = Vec::new();
    let mut next = 0;
    while next < starts.len() {
        let (start, first, parent, order) = starts[next];
        next += 1;
        let index = axes.len();
        if let Some((parent, _)) = parent {
            axes[parent].children.push(index);
        } else {
            let others = children[start].iter().filter(|&&c| c != first);
            starts.extend(others.map(|&c| (start, c, Some((index, 0)), order + 1)));
        }
        let mut nodes = vec![start, first];
        let mut arc = vec![0.0, point(skeleton, start).distance(point(skeleton, first))];
        loop {
            let at = nodes.len() - 1;
            let tip = nodes[at];
            let main = continuation(skeleton, &children[tip], &wood, nodes[at - 1], tip);
            let others = children[tip].iter().filter(|&&c| Some(c) != main);
            starts.extend(others.map(|&c| (tip, c, Some((index, at)), order + 1)));
            let Some(child) = main else {
                break;
            };
            arc.push(arc[at] + point(skeleton, tip).distance(point(skeleton, child)));
            nodes.push(child);
        }
        axes.push(Axis {
            nodes,
            arc,
            parent,
            order,
            children: Vec::new(),
        });
    }
    axes
}

/// The child of `node` that continues its axis: the most wood, then the straightest,
/// then the smallest position.
fn continuation(
    skeleton: &TreeSkeleton,
    children: &[usize],
    wood: &[f32],
    from: usize,
    node: usize,
) -> Option<usize> {
    let incoming = point(skeleton, node) - point(skeleton, from);
    let incoming = if from == node { Vec3::Y } else { incoming };
    let key = |&child: &usize| {
        let out = point(skeleton, child) - point(skeleton, node);
        let straight = -angle(incoming, out);
        (wood[child], straight, skeleton.position[child])
    };
    children.iter().copied().max_by(|a, b| {
        let (ka, kb) = (key(a), key(b));
        ka.0.total_cmp(&kb.0)
            .then(ka.1.total_cmp(&kb.1))
            .then(position_order(kb.2, ka.2))
    })
}

/// An axis resampled at the common step and added to the graph.
#[derive(Debug, Clone, Default)]
struct Resampled {
    /// Start point, then the distal end of every segment.
    points: Vec<Vec3>,
    /// Radius of every segment.
    radii: Vec<f32>,
    /// Graph node of every segment.
    nodes: Vec<NodeId>,
}

/// Axes in a deterministic order, parents before children: roots by position, children
/// by attachment arc, then by position.
fn canonical_order(axes: &[Axis], skeleton: &TreeSkeleton) -> Vec<usize> {
    let key = |&a: &usize| {
        let axis = &axes[a];
        let arc = axis.parent.map_or(0.0, |(p, at)| axes[p].arc[at]);
        (
            arc,
            skeleton.position[axis.nodes[0]],
            skeleton.position[axis.nodes[1]],
        )
    };
    let compare = |a: &usize, b: &usize| {
        let (ka, kb) = (key(a), key(b));
        ka.0.total_cmp(&kb.0)
            .then(position_order(ka.1, kb.1))
            .then(position_order(ka.2, kb.2))
    };
    let mut order: Vec<usize> = (0..axes.len())
        .filter(|&a| axes[a].parent.is_none())
        .collect();
    order.sort_by(compare);
    let mut next = 0;
    while next < order.len() {
        let mut children = axes[order[next]].children.clone();
        children.sort_by(compare);
        order.extend(children);
        next += 1;
    }
    order
}

/// Resamples the polyline of `axis` starting at `start` into equal segments of about
/// `step`, then applies `smooth` passes of the smoothing filter.
fn resample(
    axis: &Axis,
    skeleton: &TreeSkeleton,
    start: Vec3,
    step: f32,
    smooth: u32,
) -> Resampled {
    let mut polyline: Vec<Vec3> = axis.nodes.iter().map(|&n| point(skeleton, n)).collect();
    polyline[0] = start;
    let mut arc = vec![0.0_f32; polyline.len()];
    for j in 1..polyline.len() {
        arc[j] = arc[j - 1] + polyline[j].distance(polyline[j - 1]);
    }
    let length = arc[arc.len() - 1];
    let n = ((length / step).round() as usize).max(1);
    let segment = |s: f32| arc.partition_point(|&a| a < s).clamp(1, arc.len() - 1);
    let at = |s: f32| {
        let j = segment(s);
        let span = arc[j] - arc[j - 1];
        let t = if span > 0.0 {
            (s - arc[j - 1]) / span
        } else {
            1.0
        };
        polyline[j - 1].lerp(polyline[j], t.clamp(0.0, 1.0))
    };
    let mut points = vec![start];
    let mut radii = Vec::with_capacity(n);
    for k in 1..=n {
        let s = length * k as f32 / n as f32;
        points.push(if k == n {
            polyline[polyline.len() - 1]
        } else {
            at(s)
        });
        let middle = length * (k as f32 - 0.5) / n as f32;
        radii.push(skeleton.radius[axis.nodes[segment(middle)]]);
    }
    for _ in 0..smooth {
        let before = points.clone();
        for k in 1..n {
            points[k] = 0.25 * before[k - 1] + 0.5 * before[k] + 0.25 * before[k + 1];
        }
    }
    Resampled {
        points,
        radii,
        nodes: Vec::with_capacity(n),
    }
}

/// Index in the resampled parent of the node a child starting at arc `arc` attaches to.
fn snap(parent: &Resampled, length: f32, arc: f32) -> usize {
    let n = parent.radii.len();
    if length > 0.0 {
        ((arc / length * n as f32).round() as usize).clamp(1, n)
    } else {
        n
    }
}

/// Axis properties and samples gathered while the graph is built.
#[derive(Debug, Default)]
struct Gathered {
    samples: Vec<[f32; 3]>,
    properties: [[Vec<f64>; AXIS_PROPERTIES.len()]; ORDER_GROUPS],
    counts: [usize; ORDER_GROUPS],
}

impl Gathered {
    /// Records the resampled `axis` of `order`, whose first segment leaves `parent_dir`.
    fn add(&mut self, axis: &Resampled, order: u8, parent_dir: Vec3) {
        let group = usize::from(order).min(ORDER_GROUPS - 1);
        let mut length = 0.0_f32;
        for pair in axis.points.windows(2) {
            let piece = pair[0].distance(pair[1]);
            length += piece;
            let horizontal = Vec3::new(pair[1].x, 0.0, pair[1].z).length();
            self.samples.push([pair[1].y, horizontal, piece]);
        }
        let first = axis.points[1] - axis.points[0];
        let chord = axis.points[axis.points.len() - 1] - axis.points[0];
        let values = [
            f64::from(length),
            f64::from(axis.radii[0]),
            angle(parent_dir, first),
            angle(Vec3::Y, chord),
        ];
        for (list, value) in self.properties[group].iter_mut().zip(values) {
            list.push(value);
        }
        self.counts[group] += 1;
    }
}

/// Rebuilds the axes as a resampled plant graph and gathers what the terms read.
fn build(
    skeleton: &TreeSkeleton,
    axes: &[Axis],
    options: &DistanceOptions,
    step: f32,
) -> (PlantGraph, Gathered) {
    let mut graph = PlantGraph::new();
    let mut resampled: Vec<Resampled> = vec![Resampled::default(); axes.len()];
    let mut lengths = vec![0.0_f32; axes.len()];
    let mut gathered = Gathered::default();
    for a in canonical_order(axes, skeleton) {
        let axis = &axes[a];
        let (start, attach, parent_dir) = match axis.parent {
            None => (point(skeleton, axis.nodes[0]), None, Vec3::Y),
            Some((p, at)) => {
                let parent = &resampled[p];
                let k = snap(parent, lengths[p], axes[p].arc[at]);
                let dir = parent.points[k] - parent.points[k - 1];
                (parent.points[k], Some(parent.nodes[k - 1]), dir)
            }
        };
        let mut line = resample(axis, skeleton, start, step, options.smooth);
        let metamer = |k: usize| Metamer::new(line.points[k + 1], line.radii[k], 0);
        let first = match attach {
            None => graph.add_root_chain(start, metamer(0)),
            Some(node) => graph.add_chain(node, ChainKind::Lateral, metamer(0)),
        };
        let chain = graph.chain_of(first);
        let rest: Vec<NodeId> = (1..line.radii.len())
            .map(|k| graph.extend_chain(chain, metamer(k)))
            .collect();
        line.nodes.push(first);
        line.nodes.extend(rest);
        lengths[a] = line.points.windows(2).map(|w| w[0].distance(w[1])).sum();
        gathered.add(&line, axis.order, parent_dir);
        resampled[a] = line;
    }
    (graph, gathered)
}

/// The selected `ICTree` features, with whether each is an angle.
fn selected_features(features: &Features) -> Vec<(f64, bool)> {
    let local = features
        .local
        .iter()
        .filter(|(name, _)| **name != "chain_internodes")
        .map(|(name, summary)| (summary.mean, ANGLE_FEATURES.contains(name)));
    let global = GLOBALS
        .iter()
        .map(|name| (features.global.get(name).copied().unwrap_or(0.0), false));
    local.chain(global).collect()
}

/// Height of the skeleton after pruning, the scale of the automatic step.
#[must_use]
pub fn pruned_height(skeleton: &TreeSkeleton, min_radius: f32) -> f32 {
    let pruned = prune(skeleton, min_radius);
    pruned.position.iter().map(|p| p[1]).fold(0.0, f32::max)
}

/// The step of `options` for trees whose largest pruned height is `height`; 0.01 m when
/// that is not positive.
#[must_use]
pub fn step_for(height: f32, options: &DistanceOptions) -> f32 {
    let step = options
        .step
        .unwrap_or(height / options.steps_per_height.max(1.0));
    if step > 0.0 && step.is_finite() {
        step
    } else {
        0.01
    }
}

impl Descriptor {
    /// Normalises `skeleton` with `options` and resampling step `step` (meters).
    ///
    /// # Errors
    ///
    /// Returns [`FitError::Skeleton`] for an invalid skeleton or one that pruning leaves
    /// without a single segment.
    pub fn new(
        skeleton: &TreeSkeleton,
        step: f32,
        options: &DistanceOptions,
    ) -> Result<Self, FitError> {
        skeleton.validate()?;
        let pruned = prune(skeleton, options.min_radius);
        let axes = axes(&pruned);
        if axes.is_empty() {
            return Err(FitError::skeleton("no segment is left after pruning"));
        }
        let (graph, mut gathered) = build(&pruned, &axes, options, step);
        for group in &mut gathered.properties {
            for list in group {
                list.sort_by(f64::total_cmp);
            }
        }
        let height = gathered.samples.iter().map(|s| s[0]).fold(0.0, f32::max);
        let reach = gathered.samples.iter().map(|s| s[1]).fold(0.0, f32::max);
        Ok(Self {
            step,
            height,
            reach,
            nodes: graph.node_count(),
            counts: gathered.counts,
            samples: gathered.samples,
            properties: gathered.properties,
            features: selected_features(&features(&graph)),
        })
    }

    /// Compares two descriptors; `options` gives the bins and the weights.
    #[must_use]
    pub fn compare(&self, other: &Self, options: &DistanceOptions) -> Terms {
        let (envelope, density) = shape_terms(self, other, options);
        let geometry = geometry_term(self, other);
        let counts = mean_present(
            self.counts
                .iter()
                .zip(&other.counts)
                .map(|(&a, &b)| (a + b > 0).then(|| relative(a as f64, b as f64))),
        );
        let features = mean_present(self.features.iter().zip(&other.features).map(
            |(&(a, is_angle), &(b, _))| {
                Some(if is_angle {
                    ((a - b).abs() / 180.0).min(1.0)
                } else {
                    relative(a, b)
                })
            },
        ));
        let w = options.weights;
        let weighted = [
            (w.envelope, envelope),
            (w.density, density),
            (w.geometry, geometry),
            (w.counts, counts),
            (w.features, features),
        ];
        let weight: f64 = weighted.iter().map(|(w, _)| w).sum();
        let sum: f64 = weighted.iter().map(|(w, d)| w * d).sum();
        Terms {
            envelope,
            density,
            geometry,
            counts,
            features,
            total: if weight > 0.0 { sum / weight } else { 0.0 },
        }
    }
}

/// `|a - b| / (|a| + |b|)`, 0 when both are 0.
fn relative(a: f64, b: f64) -> f64 {
    let scale = a.abs() + b.abs();
    if scale > 0.0 {
        (a - b).abs() / scale
    } else {
        0.0
    }
}

/// Mean of the present values; 0 when none is.
fn mean_present(values: impl Iterator<Item = Option<f64>>) -> f64 {
    let (sum, count) = values
        .flatten()
        .fold((0.0, 0_usize), |(sum, count), v| (sum + v, count + 1));
    if count > 0 { sum / count as f64 } else { 0.0 }
}

/// The envelope and density terms.
fn shape_terms(a: &Descriptor, b: &Descriptor, options: &DistanceOptions) -> (f64, f64) {
    let height = a.height.max(b.height);
    let reach = a.reach.max(b.reach);
    let rows = options.height_bins.max(1);
    let columns = options.radial_bins.max(1);
    let bin = |value: f32, extent: f32, bins: usize| {
        if extent > 0.0 {
            ((value / extent * bins as f32).max(0.0) as usize).min(bins - 1)
        } else {
            0
        }
    };
    let profile = |d: &Descriptor| {
        let mut profile = vec![0.0_f64; rows];
        let mut cells = vec![0.0_f64; rows * columns];
        let mut total = 0.0;
        for &[y, r, length] in &d.samples {
            let row = bin(y, height, rows);
            profile[row] = profile[row].max(f64::from(r));
            cells[row * columns + bin(r, reach, columns)] += f64::from(length);
            total += f64::from(length);
        }
        if total > 0.0 {
            for cell in &mut cells {
                *cell /= total;
            }
        }
        (profile, cells)
    };
    let ((pa, ca), (pb, cb)) = (profile(a), profile(b));
    let union: f64 = pa.iter().zip(&pb).map(|(x, y)| x.max(*y)).sum();
    let intersection: f64 = pa.iter().zip(&pb).map(|(x, y)| x.min(*y)).sum();
    let envelope = if union > 0.0 {
        1.0 - intersection / union
    } else {
        0.0
    };
    let density = 0.5 * ca.iter().zip(&cb).map(|(x, y)| (x - y).abs()).sum::<f64>();
    (envelope, density.min(1.0))
}

/// The geometry term.
fn geometry_term(a: &Descriptor, b: &Descriptor) -> f64 {
    let pairs = a.properties.iter().zip(&b.properties).flat_map(|(ga, gb)| {
        ga.iter()
            .zip(gb)
            .enumerate()
            .map(|(property, (x, y))| match (x.is_empty(), y.is_empty()) {
                (true, true) => None,
                (true, false) | (false, true) => Some(1.0),
                (false, false) => {
                    let w = wasserstein(x, y);
                    Some(if property >= 2 {
                        (w / 180.0).min(1.0)
                    } else {
                        let scale = mean(x) + mean(y);
                        if scale > 0.0 {
                            (w / scale).min(1.0)
                        } else {
                            0.0
                        }
                    })
                }
            })
    });
    mean_present(pairs)
}

fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len().max(1) as f64
}

/// 1-Wasserstein distance between the empirical distributions of two sorted samples:
/// the integral of the absolute difference of their distribution functions.
#[must_use]
pub fn wasserstein(a: &[f64], b: &[f64]) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let (count_a, count_b) = (a.len() as f64, b.len() as f64);
    let (mut seen_a, mut seen_b) = (0, 0);
    let mut previous = a[0].min(b[0]);
    let mut total = 0.0;
    while seen_a < a.len() || seen_b < b.len() {
        let take_a = seen_b >= b.len() || (seen_a < a.len() && a[seen_a] <= b[seen_b]);
        let value = if take_a { a[seen_a] } else { b[seen_b] };
        let gap = seen_a as f64 / count_a - seen_b as f64 / count_b;
        total += gap.abs() * (value - previous);
        previous = value;
        if take_a {
            seen_a += 1;
        } else {
            seen_b += 1;
        }
    }
    total
}

/// The skeleton as the distance sees it: pruned, placed, split into axes and resampled
/// at `step`, as a plant graph (all births 0, no buds or organs). Useful to look at a
/// target, for instance by meshing its bark with a species.
///
/// # Errors
///
/// As [`Descriptor::new`].
pub fn normalized_graph(
    skeleton: &TreeSkeleton,
    step: f32,
    options: &DistanceOptions,
) -> Result<PlantGraph, FitError> {
    skeleton.validate()?;
    let pruned = prune(skeleton, options.min_radius);
    let axes = axes(&pruned);
    if axes.is_empty() {
        return Err(FitError::skeleton("no segment is left after pruning"));
    }
    Ok(build(&pruned, &axes, options, step).0)
}

/// The distance between two skeletons. Without `options.step`, the step is that of the
/// taller of the two after pruning, so the distance is symmetric.
///
/// # Errors
///
/// Returns [`FitError::Skeleton`] when a skeleton is invalid or empty after pruning.
pub fn distance(
    a: &TreeSkeleton,
    b: &TreeSkeleton,
    options: &DistanceOptions,
) -> Result<Terms, FitError> {
    let height = pruned_height(a, options.min_radius).max(pruned_height(b, options.min_radius));
    let step = step_for(height, options);
    let da = Descriptor::new(a, step, options)?;
    let db = Descriptor::new(b, step, options)?;
    Ok(da.compare(&db, options))
}

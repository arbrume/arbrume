//! Baked wind data: what a vertex shader needs to bend a plant in the wind, computed once
//! when the plant is meshed. Nothing is simulated here.
//!
//! The scheme is the hierarchical vertex displacement of Habel et al. (2009). Every
//! meshed axis is a branch, a tapered cantilever bending about its base. A branch has a
//! hierarchy level: 0 for a root axis, the level of its parent plus one for the others,
//! at most 3. A deeper axis gets level 3 and its own base as pivot, so the levels in
//! between move with the level-3 ancestor without bending of their own.
//!
//! **Per vertex** ([`WindVertex`]): `x0..x3`, the normalized position (0 at the base, 1
//! at the tip, by arc length along the axis polyline) along the branch of each level
//! that carries the vertex, the index of its own branch, and a flutter weight. Below its
//! own level, `x_k` is where the lineage of the vertex leaves the branch of level `k`;
//! at its own level it is the position of the vertex; above it is 0. The vertices of a
//! child where it leaves its parent therefore carry the same values as the parent
//! there, and the bending of the levels below moves them together. A leaf or twig takes
//! the values of the point of the axis it is attached to, and a flutter that grows from
//! 0 at the attachment to 1 at its farthest point.
//!
//! **Per branch** ([`WindBranch`]): parent, level, lineage, pivot, frame, length, root
//! and tip radius, taper ratio, mass and the deflection shape. The shape is the static
//! deflection `u(x)` of a cantilever of linearly tapering radius, `I(x)` proportional to
//! `((alpha - 1) x + 1)^4`, under a uniform load, from `(E I u'')'' = q` with a clamped
//! root and a free tip, fitted by least squares with `c2 x^2 + c4 x^4` as Habel et al.
//! do (their table 1: 0.3326 and 0.3989 at `alpha = 0.1`, which this integration
//! reproduces before normalization), then normalized so that `u(1) = 1`: the amplitude
//! is left to the shader. The mass is that of Habel et al. (eq. 16), the volume of the
//! frusta of the branch and of every branch it bears, with a unit density. The
//! frequency rule they cite has no stated units and is not baked.

use crate::axes::Mount;
use crate::bark::BarkPlan;
use arbrume_core::graph::{ChainId, PlantGraph};
use arbrume_core::mesh::{MeshBuffers, WindBranch, WindTable, WindVertex};
use glam::Vec3;
use std::f32::consts::PI;
use std::sync::OnceLock;

/// Taper ratios sampled by the deflection shape table, evenly over `[0, 1]`.
const SHAPE_SAMPLES: usize = 65;

/// Smallest taper ratio used for the deflection shape.
const MIN_TAPER: f32 = 0.02;

/// Integration steps of one deflection curve.
const SHAPE_STEPS: usize = 512;

/// Highest hierarchy level.
const MAX_LEVEL: u8 = 3;

/// Branch index of a chain that is not meshed.
pub(crate) const NO_BRANCH: u32 = u32::MAX;

/// Least-squares fit of `c2 x^2 + c4 x^4` to the deflection of a beam of taper `alpha`,
/// normalized to `c2 + c4 = 1`.
fn fit_shape(alpha: f64) -> [f32; 2] {
    let h = 1.0 / SHAPE_STEPS as f64;
    let (mut u, mut slope) = (0.0_f64, 0.0_f64);
    let (mut s44, mut s46, mut s48, mut b2, mut b4) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for step in 0..=SHAPE_STEPS {
        let x = step as f64 * h;
        let (x2, x4) = (x * x, x.powi(4));
        s44 += x4;
        s46 += x2 * x4;
        s48 += x4 * x4;
        b2 += x2 * u;
        b4 += x4 * u;
        let mid = x + 0.5 * h;
        let curvature = 0.5 * (1.0 - mid).powi(2) / ((alpha - 1.0) * mid + 1.0).powi(4);
        slope += curvature * h;
        u += slope * h;
    }
    let det = s44 * s48 - s46 * s46;
    let c2 = (b2 * s48 - b4 * s46) / det;
    let c4 = (s44 * b4 - s46 * b2) / det;
    let total = c2 + c4;
    [(c2 / total) as f32, (c4 / total) as f32]
}

/// The deflection shape of a branch of taper ratio `taper`, interpolated in a table.
pub(crate) fn deflection_shape(taper: f32) -> [f32; 2] {
    static TABLE: OnceLock<Vec<[f32; 2]>> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        (0..SHAPE_SAMPLES)
            .map(|i| {
                let alpha = i as f64 / (SHAPE_SAMPLES - 1) as f64;
                fit_shape(alpha.max(f64::from(MIN_TAPER)))
            })
            .collect()
    });
    let at = taper.clamp(0.0, 1.0) * (SHAPE_SAMPLES - 1) as f32;
    let low = (at.floor() as usize).min(SHAPE_SAMPLES - 2);
    let t = at - low as f32;
    let (a, b) = (table[low], table[low + 1]);
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

/// The branches of a mesh and the branch of every chain.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct WindPlan {
    /// The table written to the mesh.
    pub table: WindTable,
    /// Branch index of each chain, by chain id; [`NO_BRANCH`] when not meshed.
    pub branch_of: Vec<u32>,
    /// Length of each branch, the divisor of its positions.
    lengths: Vec<f32>,
    /// Wind data of each kept leaf in buffer order, flutter 0.
    pub leaves: Vec<WindVertex>,
}

/// A unit vector orthogonal to `tangent`, as horizontal as possible.
fn right_of(tangent: Vec3) -> Vec3 {
    Vec3::Y
        .cross(tangent)
        .try_normalize()
        .unwrap_or_else(|| tangent.any_orthonormal_vector())
}

impl WindPlan {
    /// Builds the branch table of the chains kept by `bark`.
    pub(crate) fn build(
        &mut self,
        graph: &PlantGraph,
        bark: &BarkPlan,
        arcs: &[f32],
        max_cycle: Option<u16>,
    ) {
        self.table.branches.clear();
        self.branch_of.clear();
        self.lengths.clear();
        let mut volumes = Vec::new();
        for chain in graph.chain_ids() {
            let tube = bark.tube(chain.index());
            let Some(view) = graph.chain_view(chain).filter(|_| tube.vertices > 0) else {
                self.branch_of.push(NO_BRANCH);
                continue;
            };
            let nodes = max_cycle
                .map_or(view.len(), |cycle| view.born_by(cycle))
                .max(1);
            let index = self.table.branches.len() as u32;
            self.branch_of.push(index);
            let length = arcs[view.first.index() + nodes - 1].max(1e-6);
            let (positions, radii) = (&view.positions[..nodes], &view.radii[..nodes]);
            let mut volume = 0.0;
            let mut previous = (view.origin, radii[0]);
            for (&point, &radius) in positions.iter().zip(radii) {
                let (a, b) = (previous.1, radius);
                volume += PI / 3.0 * point.distance(previous.0) * (a * a + a * b + b * b);
                previous = (point, radius);
            }
            volumes.push(volume);
            let tangent = (positions[0] - view.origin)
                .try_normalize()
                .unwrap_or(Vec3::Y);
            let (root, tip) = (radii[0], radii[nodes - 1]);
            let taper = if root > 0.0 { tip / root } else { 1.0 };
            let mut branch = WindBranch {
                chain: chain.0,
                parent: None,
                level: 0,
                lineage: [index; 4],
                lineage_x: [0.0; 4],
                pivot: view.origin.to_array(),
                tangent: tangent.to_array(),
                right: right_of(tangent).to_array(),
                length,
                root_radius: root,
                tip_radius: tip,
                taper,
                mass: 0.0,
                shape: deflection_shape(taper),
            };
            if !view.attach.is_none() {
                self.link(graph, view.attach, &mut branch, arcs);
            }
            self.table.branches.push(branch);
            self.lengths.push(length);
        }
        self.accumulate_mass(&volumes);
    }

    /// Sets the parent, level and lineage of `branch`, which grows from node `attach`.
    fn link(
        &self,
        graph: &PlantGraph,
        attach: arbrume_core::graph::NodeId,
        branch: &mut WindBranch,
        arcs: &[f32],
    ) {
        let parent_index = self.branch_of[graph.chain_of(attach).index()];
        let parent = &self.table.branches[parent_index as usize];
        let x = (arcs[attach.index()] / self.lengths[parent_index as usize]).clamp(0.0, 1.0);
        let own = branch.lineage[0];
        branch.parent = Some(parent_index);
        branch.lineage = parent.lineage;
        branch.lineage_x = parent.lineage_x;
        if parent.level < MAX_LEVEL {
            branch.level = parent.level + 1;
            branch.lineage_x[usize::from(parent.level)] = x;
        } else {
            branch.level = MAX_LEVEL;
        }
        let level = usize::from(branch.level);
        branch.lineage[level..].fill(own);
        branch.lineage_x[level..].fill(0.0);
    }

    /// Adds to each branch the volume of the branches it bears, children first.
    fn accumulate_mass(&mut self, volumes: &[f32]) {
        let branches = &mut self.table.branches;
        for (branch, &volume) in branches.iter_mut().zip(volumes) {
            branch.mass = volume;
        }
        for index in (0..branches.len()).rev() {
            if let Some(parent) = branches[index].parent {
                let mass = branches[index].mass;
                branches[parent as usize].mass += mass;
            }
        }
    }

    /// The wind data of a point at arc length `arc` along chain `chain`, flutter 0.
    fn at(&self, chain: usize, arc: f32) -> WindVertex {
        let index = self.branch_of[chain];
        if index == NO_BRANCH {
            return [0.0; 6];
        }
        let branch = &self.table.branches[index as usize];
        let x = (arc / self.lengths[index as usize]).clamp(0.0, 1.0);
        let mut vertex = [0.0; 6];
        vertex[..4].copy_from_slice(&branch.lineage_x);
        vertex[usize::from(branch.level)] = x;
        vertex[4] = index as f32;
        vertex
    }

    /// Writes the wind data of every bark vertex of `bark` into `buffers.wind`.
    pub(crate) fn fill_bark(&self, bark: &BarkPlan, buffers: &mut MeshBuffers) {
        buffers.wind.clear();
        buffers.wind.resize(buffers.positions.len(), [0.0; 6]);
        for (chain, &index) in self.branch_of.iter().enumerate() {
            if index == NO_BRANCH {
                continue;
            }
            let tube = bark.tube(chain);
            let out = &mut buffers.wind[tube.vertex_start..tube.vertex_start + tube.vertices];
            let mut slot = 0;
            for ring in tube.rings {
                let value = self.at(chain, ring.arc);
                let count = ring.sides as usize + 1;
                out[slot..slot + count].fill(value);
                slot += count;
            }
            let length = self.lengths[index as usize];
            out[slot] = self.at(chain, length);
            slot += 1;
            out[slot..].fill(self.at(chain, 0.0));
        }
    }

    /// Computes the wind data of the leaves `organs` of `graph`, attached at `mounts`
    /// (or at their nodes when `mounts` is empty).
    pub(crate) fn fill_leaves(
        &mut self,
        graph: &PlantGraph,
        organs: &[u32],
        mounts: &[Mount],
        arcs: &[f32],
    ) {
        self.leaves.clear();
        let sites = graph.organs();
        for (slot, &organ) in organs.iter().enumerate() {
            let value = mounts.get(slot).map_or_else(
                || {
                    let node = sites[organ as usize].node;
                    self.at(graph.chain_of(node).index(), arcs[node.index()])
                },
                |mount| self.at(ChainId(mount.chain).index(), mount.arc),
            );
            self.leaves.push(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{deflection_shape, fit_shape};

    #[test]
    fn deflection_fit_reproduces_the_published_coefficients() {
        let [c2, c4] = fit_shape(0.1);
        let published = [0.3326_f32, 0.398_924];
        let total = published[0] + published[1];
        assert!((c2 - published[0] / total).abs() < 2e-3, "{c2}");
        assert!((c4 - published[1] / total).abs() < 2e-3, "{c4}");
    }

    #[test]
    fn shape_is_normalized_and_stiffer_beams_bend_more_uniformly() {
        for taper in [0.0, 0.05, 0.3, 0.77, 1.0] {
            let [c2, c4] = deflection_shape(taper);
            assert!((c2 + c4 - 1.0).abs() < 1e-5);
        }
        assert!(deflection_shape(1.0)[0] > deflection_shape(0.1)[0]);
    }
}

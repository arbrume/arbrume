//! The bark part: every axis of the plant as a tube.
//!
//! The work is split in two passes over the chains of the graph.
//!
//! 1. **Plan** (sequential, cheap). Each kept chain is sampled into rings, appended to
//!    one buffer shared by the whole plant, and the number of vertices and indices of its
//!    tube is counted. A chain is left out when it has no node at the requested age, when
//!    its base radius is below `MeshOptions::min_radius`, or when its parent chain was
//!    left out. A running sum of the counts gives each chain its place in the output.
//! 2. **Fill** (parallel over chains when asked). The output buffers are sized once and
//!    cut into one disjoint slice per chain; each chain writes its vertices and triangles
//!    from its rings alone. The parallel fill halves the list of chains by vertex count
//!    until a piece is small enough for one task, splitting the slices along with it.
//!
//! The fill is a pure function of the rings of one chain, so the result is bit-identical
//! whatever the number of threads. Nothing is allocated per node or per chain: the rings,
//! the plans and the output buffers are each one allocation, reused from one mesh to the
//! next by a `Mesher`.
//!
//! **Junctions.** A lateral axis starts on the centerline of its parent, inside it, so
//! the two tubes intersect and no gap can open (as in Longay et al. 2012 and Raistrick et
//! al. 2023). Where it comes out it is widened into a collar that narrows back over a few
//! radii, which hides the seam and reads as the branch collar of a real tree. The surface
//! is not watertight. A relay axis, which continues its parent at the same order, gets
//! no collar.

use crate::ring::{Axis, BaseProfile, Ring, Sweep, sample_axis};
use crate::tube::{Circles, Style, TubeOut, tube_counts, write_tube};
use crate::{MeshError, MeshOptions};
use arbrume_core::graph::{ChainView, PlantGraph};
use arbrume_core::mesh::MeshBuffers;
use arbrume_core::rng::{Rng, Stream};
use arbrume_core::species::Species;
use std::f32::consts::TAU;
use std::ops::Range;

/// Runs of chains with at most this many vertices are filled by one parallel task.
const VERTICES_PER_TASK: usize = 16_384;

/// Smallest base radius used to scale texture coordinates, in meters.
const MIN_STYLE_RADIUS: f32 = 1e-5;

/// What pass 1 decided for one chain.
#[derive(Debug, Clone, PartialEq)]
struct ChainPlan {
    /// Rings of the chain in the shared ring buffer; empty when the chain is left out.
    rings: Range<usize>,
    /// Number of vertices of its tube.
    vertices: usize,
    /// Number of indices of its tube.
    indices: usize,
    /// Index of its first vertex in the bark buffers.
    vertex_start: usize,
    /// Index of its first index in the bark buffers.
    index_start: usize,
    /// Texture mapping and lobes.
    style: Style,
}

/// Result of pass 1. Kept between meshes so that its buffers are reused.
#[derive(Debug, Clone, Default)]
pub(crate) struct BarkPlan {
    rings: Vec<Ring>,
    chains: Vec<ChainPlan>,
    /// Total number of vertices.
    pub vertices: usize,
    /// Total number of indices.
    pub indices: usize,
    max_sides: u32,
}

/// Base profile of the chain `view`, sampled as `axis`.
fn base_profile(graph: &PlantGraph, view: &ChainView, axis: &Axis, sweep: &Sweep) -> BaseProfile {
    let radius = axis.radii[0];
    let Some(parent_radius) = view.parent_radius else {
        return BaseProfile::flare(sweep, axis.length(), radius);
    };
    if graph.orders()[view.attach.index()] == view.order {
        return BaseProfile::Plain;
    }
    let parent = (view.origin - graph.proximal(view.attach)).normalize_or_zero();
    let sine = axis
        .first_direction()
        .map_or(1.0, |direction| direction.cross(parent).length());
    BaseProfile::collar(sweep, radius, parent_radius, sine)
}

/// Texture mapping of a chain of base radius `radius`.
fn style(species: &Species, view: &ChainView, radius: f32, seed: u64) -> Style {
    let [around, along] = species.bark.texture_size;
    let circumference = TAU * radius;
    let u_repeats = (circumference / around).round().max(1.0);
    let v_offset = if view.attach.is_none() {
        0.0
    } else {
        Rng::new(seed, view.chain.0, Stream::BARK).f32()
    };
    Style {
        lobes: species.geometry.buttress_lobes,
        u_repeats,
        v_scale: u_repeats / circumference * around / along,
        v_offset,
    }
}

/// Pass 1: samples every kept chain into `plan` and counts its tube. The buffers of
/// `plan` are reused.
pub(crate) fn plan_bark(
    plan: &mut BarkPlan,
    graph: &PlantGraph,
    species: &Species,
    options: &MeshOptions,
) -> Result<(), MeshError> {
    let sweep = Sweep {
        geometry: &species.geometry,
        tolerance: options.tolerance,
    };
    plan.rings.clear();
    plan.chains.clear();
    (plan.vertices, plan.indices) = (0, 0);
    plan.max_sides = sweep.max_sides();
    for chain in graph.chain_ids() {
        let view = graph.chain_view(chain).ok_or(MeshError::NotContiguous)?;
        let nodes = options
            .max_cycle
            .map_or(view.len(), |cycle| view.born_by(cycle));
        let parent_kept = view.attach.is_none()
            || !plan.chains[graph.chain_of(view.attach).index()]
                .rings
                .is_empty();
        let start = plan.rings.len();
        let mut chain_style = Style {
            lobes: 0,
            u_repeats: 1.0,
            v_scale: 1.0,
            v_offset: 0.0,
        };
        if nodes > 0 && parent_kept && view.radii[0] >= options.min_radius {
            let mut axis = Axis {
                origin: view.origin,
                positions: &view.positions[..nodes],
                radii: &view.radii[..nodes],
                profile: BaseProfile::Plain,
            };
            axis.profile = base_profile(graph, &view, &axis, &sweep);
            sample_axis(&axis, &sweep, &mut plan.rings);
            let radius = axis.radii[0].max(MIN_STYLE_RADIUS);
            chain_style = style(species, &view, radius, options.seed);
        }
        let (vertices, indices) = tube_counts(&plan.rings[start..]);
        plan.chains.push(ChainPlan {
            rings: start..plan.rings.len(),
            vertices,
            indices,
            vertex_start: plan.vertices,
            index_start: plan.indices,
            style: chain_style,
        });
        plan.vertices += vertices;
        plan.indices += indices;
    }
    if u32::try_from(plan.vertices).is_err() {
        return Err(MeshError::TooLarge {
            vertices: plan.vertices,
        });
    }
    Ok(())
}

/// The slices of the bark buffers owned by a run of consecutive chains.
struct Run<'a> {
    positions: &'a mut [[f32; 3]],
    normals: &'a mut [[f32; 3]],
    uvs: &'a mut [[f32; 2]],
    indices: &'a mut [u32],
}

impl Run<'_> {
    /// Splits off the first `vertices` vertices and `indices` indices.
    fn split(self, vertices: usize, indices: usize) -> (Self, Self) {
        let (positions, positions_rest) = self.positions.split_at_mut(vertices);
        let (normals, normals_rest) = self.normals.split_at_mut(vertices);
        let (uvs, uvs_rest) = self.uvs.split_at_mut(vertices);
        let (indices, indices_rest) = self.indices.split_at_mut(indices);
        let head = Self {
            positions,
            normals,
            uvs,
            indices,
        };
        let rest = Self {
            positions: positions_rest,
            normals: normals_rest,
            uvs: uvs_rest,
            indices: indices_rest,
        };
        (head, rest)
    }
}

/// What every fill task reads.
#[derive(Clone, Copy)]
struct Filler<'a> {
    rings: &'a [Ring],
    circles: &'a Circles,
}

impl Filler<'_> {
    /// Fills the chains `chains`, whose slices are `run`, one after the other.
    fn fill(self, chains: &[ChainPlan], mut run: Run) {
        for chain in chains.iter().filter(|chain| chain.vertices > 0) {
            let (own, rest) = run.split(chain.vertices, chain.indices);
            run = rest;
            let mut out = TubeOut {
                positions: own.positions,
                normals: own.normals,
                uvs: own.uvs,
                indices: own.indices,
                base: chain.vertex_start as u32,
            };
            let rings = &self.rings[chain.rings.clone()];
            write_tube(rings, &chain.style, self.circles, &mut out);
        }
    }

    /// Fills `chains` on the `rayon` pool: the run is halved by vertex count until a
    /// piece is small enough for one task.
    fn fill_parallel(self, chains: &[ChainPlan], run: Run) {
        let (Some(first), Some(last)) = (chains.first(), chains.last()) else {
            return;
        };
        let vertices = last.vertex_start + last.vertices - first.vertex_start;
        if vertices <= VERTICES_PER_TASK || chains.len() < 2 {
            return self.fill(chains, run);
        }
        let middle = first.vertex_start + vertices / 2;
        let cut = chains
            .partition_point(|chain| chain.vertex_start < middle)
            .clamp(1, chains.len() - 1);
        let (left, right) = chains.split_at(cut);
        let (head, rest) = run.split(
            right[0].vertex_start - first.vertex_start,
            right[0].index_start - first.index_start,
        );
        rayon::join(
            || self.fill_parallel(left, head),
            || self.fill_parallel(right, rest),
        );
    }
}

/// Pass 2: sizes `buffers` for `plan` and fills them. Their allocations are reused.
pub(crate) fn fill_bark(
    plan: &BarkPlan,
    circles: &mut Circles,
    parallel: bool,
    buffers: &mut MeshBuffers,
) {
    buffers.positions.resize(plan.vertices, [0.0; 3]);
    buffers.normals.resize(plan.vertices, [0.0; 3]);
    buffers.uvs.resize(plan.vertices, [0.0; 2]);
    buffers.colors.clear();
    buffers.indices.resize(plan.indices, 0);
    circles.ensure(plan.max_sides);
    let filler = Filler {
        rings: &plan.rings,
        circles,
    };
    let run = Run {
        positions: &mut buffers.positions,
        normals: &mut buffers.normals,
        uvs: &mut buffers.uvs,
        indices: &mut buffers.indices,
    };
    if parallel {
        filler.fill_parallel(&plan.chains, run);
    } else {
        filler.fill(&plan.chains, run);
    }
}

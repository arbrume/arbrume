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
//! **Junctions.** With Tier A junctions (the default) a lateral axis starts on the
//! centerline of its parent, inside it, so the two tubes intersect and no gap can open
//! (as in Longay et al. 2012 and Raistrick et al. 2023). Where it comes out it is widened
//! into a collar that narrows back over a few radii, which hides the seam and reads as
//! the branch collar of a real tree. The surface is not watertight. A relay axis, which
//! continues its parent at the same order, gets no collar. With Tier B junctions the
//! child is stitched into a hole in its parent instead (`crate::junction`); the plan of a
//! chain then also places the holes of its children, before they are planned.

use crate::junction::{Foot, Junction};
use crate::ring::{Axis, BaseProfile, Ring, Sweep, frame_rings, sample_axis};
use crate::tube::{Circles, Joins, Style, Tint, TubeOut, tube_counts, write_tube};
use crate::{Junctions, MeshError, MeshOptions};
use arbrume_core::graph::{ChainView, PlantGraph};
use arbrume_core::mesh::MeshBuffers;
use arbrume_core::rng::{Rng, Stream};
use arbrume_core::species::Geometry;
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
    /// Tier B junctions of its children, in [`BarkPlan::junctions`].
    holes: Range<usize>,
    /// Its own Tier B junction with its parent.
    own: Option<usize>,
}

/// Result of pass 1. Kept between meshes so that its buffers are reused.
#[derive(Debug, Clone, Default)]
pub(crate) struct BarkPlan {
    pub(crate) rings: Vec<Ring>,
    chains: Vec<ChainPlan>,
    /// Total number of vertices.
    pub vertices: usize,
    /// Total number of indices.
    pub indices: usize,
    pub(crate) max_sides: u32,
    /// Whether the tubes carry vertex colours.
    colored: bool,
    /// Reach of each chain: an upper bound of the distance from its origin to any node
    /// of the chains it bears, itself included. Filled only when it decides a cut.
    reach: Vec<f32>,
    /// Tier B junctions, grouped by parent chain.
    pub(crate) junctions: Vec<Junction>,
    /// Footprint points of the junctions.
    pub(crate) feet: Vec<Foot>,
    /// Triangles of the stitches of the junctions.
    pub(crate) stitches: Vec<[u32; 3]>,
    /// Working memory of the junction planning.
    pub(crate) scratch: crate::junction::Scratch,
    /// Geometry of the species meshed.
    pub(crate) geometry: Geometry,
}

impl BarkPlan {
    /// First vertex and vertex count of the tube of each chain, indexed by chain id; the
    /// count is 0 for a chain left out.
    pub(crate) fn chain_vertices(&self) -> impl ExactSizeIterator<Item = (usize, usize)> + '_ {
        self.chains
            .iter()
            .map(|chain| (chain.vertex_start, chain.vertices))
    }
}

/// The tube of one chain as the wind data sees it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TubeLayout<'a> {
    /// Rings of the chain, empty when it is left out.
    pub rings: &'a [Ring],
    /// First vertex of the tube in the bark buffers.
    pub vertex_start: usize,
    /// Vertices of the tube.
    pub vertices: usize,
    /// Its own Tier B junction and footprint.
    pub own: Option<(&'a Junction, &'a [Foot])>,
}

impl BarkPlan {
    /// Fills `kept` with whether each chain, by chain id, is meshed.
    pub(crate) fn fill_kept(&self, kept: &mut Vec<bool>) {
        kept.clear();
        kept.extend(self.chains.iter().map(|chain| chain.vertices > 0));
    }

    /// The tube of chain `chain` (by chain id).
    pub(crate) fn tube(&self, chain: usize) -> TubeLayout<'_> {
        let plan = &self.chains[chain];
        TubeLayout {
            rings: &self.rings[plan.rings.clone()],
            vertex_start: plan.vertex_start,
            vertices: plan.vertices,
            own: plan.own.map(|index| {
                let junction = &self.junctions[index];
                (junction, &self.feet[junction.feet.clone()])
            }),
        }
    }

    /// What the fill of chain `plan` reads about its junctions.
    fn joins(&self, plan: &ChainPlan) -> Joins<'_> {
        let own = plan.own.map(|index| &self.junctions[index]);
        Joins {
            holes: &self.junctions[plan.holes.clone()],
            own,
            feet: own.map_or(&[], |junction| &self.feet[junction.feet.clone()]),
            stitches: &self.stitches,
            parent: own.map(|junction| {
                let parent = &self.chains[junction.hole.parent as usize];
                (&self.rings[parent.rings.clone()], &parent.style)
            }),
            geometry: &self.geometry,
        }
    }
}

/// Base profile of the chain `view`, sampled as `axis`.
pub(crate) fn base_profile(
    graph: &PlantGraph,
    view: &ChainView,
    axis: &Axis,
    sweep: &Sweep,
) -> BaseProfile {
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

/// Texture mapping and colours of a chain of base radius `radius`.
///
/// The vertex colours are factors on `Bark::material_color`: the young and old colours
/// of the species divided by it, both darkened by the shade of the chain, which is drawn
/// from the mesh seed and the chain id.
fn style(species: &Species, view: &ChainView, radius: f32, seed: u64) -> Style {
    let bark = &species.bark;
    let [around, along] = bark.texture_size;
    let circumference = TAU * radius;
    let u_repeats = (circumference / around).round().max(1.0);
    let mut rng = Rng::new(seed, view.chain.0, Stream::BARK);
    let offset = rng.f32();
    let v_offset = if view.attach.is_none() { 0.0 } else { offset };
    let tint = bark.has_vertex_colors().then(|| {
        let base = bark.material_color();
        let shade = bark.color_variation.mul_add(-rng.f32(), 1.0);
        let factor = |color: [f32; 3]| {
            std::array::from_fn(|channel| shade * color[channel] / base[channel].max(1e-6))
        };
        Tint {
            young: factor(bark.young_color.unwrap_or(bark.color)),
            old: factor(bark.color),
            young_radius: bark.young_radius,
        }
    });
    Style {
        lobes: species.geometry.buttress_lobes,
        u_repeats,
        v_scale: u_repeats / circumference * around / along,
        v_offset,
        closed_base: view.attach.is_none(),
        tint,
    }
}

/// Fills `reach` with the reach of every chain of `graph` (see [`BarkPlan`]), children
/// before parents.
fn fill_reach(graph: &PlantGraph, reach: &mut Vec<f32>) {
    reach.clear();
    reach.resize(graph.chain_count(), 0.0);
    for chain in graph.chain_ids().rev() {
        let Some(view) = graph.chain_view(chain) else {
            continue;
        };
        let own = view
            .positions
            .iter()
            .map(|p| p.distance(view.origin))
            .fold(0.0, f32::max);
        let index = chain.index();
        reach[index] = reach[index].max(own);
        if let Some(parent) = graph.parent_chain(chain) {
            let origin = graph.chain_origin(parent);
            let through = view.origin.distance(origin) + reach[index];
            reach[parent.index()] = reach[parent.index()].max(through);
        }
    }
}

/// Whether the chain `view`, cut to `nodes` nodes, is meshed by the rules of
/// [`plan_bark`], its parent being kept or not.
pub(crate) fn kept(
    view: &ChainView,
    nodes: usize,
    options: &MeshOptions,
    plan: &BarkPlan,
    parent_kept: bool,
) -> bool {
    let far = !plan.reach.is_empty() && plan.reach[view.chain.index()] > options.max_drop_reach;
    nodes > 0 && parent_kept && (view.radii[0] >= options.min_radius || far)
}

/// What the planning of every chain reads.
struct Inputs<'a> {
    graph: &'a PlantGraph,
    species: &'a Species,
    options: &'a MeshOptions,
    sweep: &'a Sweep<'a>,
}

/// Samples the kept chain `view`, cut to `nodes` nodes, into the rings of `plan`, with
/// its own Tier B junction `own` and, with Tier B junctions, the holes of its children;
/// returns its style.
fn sample_chain(
    plan: &mut BarkPlan,
    inputs: &Inputs,
    view: &ChainView,
    nodes: usize,
    own: Option<usize>,
) -> Style {
    let (start, holes_start) = (plan.rings.len(), plan.junctions.len());
    let tier_b = inputs.options.junctions == Junctions::TierB;
    let mut axis = Axis {
        origin: view.origin,
        positions: &view.positions[..nodes],
        radii: &view.radii[..nodes],
        profile: BaseProfile::Plain,
        stops: &[],
        extend: 0.0,
    };
    axis.profile = base_profile(inputs.graph, view, &axis, inputs.sweep);
    if tier_b {
        let context = crate::junction::Context {
            graph: inputs.graph,
            sweep: inputs.sweep,
            options: inputs.options,
        };
        crate::junction::plan_children(plan, &context, &axis, view.chain, own);
    }
    let stops = std::mem::take(&mut plan.scratch.stops);
    let sampled = Axis {
        stops: &stops,
        extend: plan.scratch.extend,
        ..axis
    };
    sample_axis(&sampled, inputs.sweep, &mut plan.rings);
    plan.scratch.stops = stops;
    if let Some(own) = own {
        crate::junction::collar_rings_into(plan, own, start);
    }
    frame_rings(&mut plan.rings[start..]);
    if tier_b {
        crate::junction::place_holes(plan, start, holes_start, own);
    }
    let radius = axis.radii[0].max(MIN_STYLE_RADIUS);
    style(inputs.species, view, radius, inputs.options.seed)
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
    plan.colored = species.bark.has_vertex_colors();
    plan.reach.clear();
    plan.junctions.clear();
    plan.feet.clear();
    plan.stitches.clear();
    plan.geometry.clone_from(&species.geometry);
    let tier_b = options.junctions == Junctions::TierB;
    if tier_b {
        plan.scratch.start(graph);
    }
    if options.min_radius > 0.0 && options.max_drop_reach.is_finite() {
        fill_reach(graph, &mut plan.reach);
    }
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
            closed_base: false,
            tint: None,
        };
        let own = if tier_b && !view.attach.is_none() {
            plan.scratch.own(chain.index())
        } else {
            None
        };
        let joined = !tier_b || view.attach.is_none() || own.is_some();
        let holes_start = plan.junctions.len();
        if kept(&view, nodes, options, plan, parent_kept) && joined {
            let inputs = Inputs {
                graph,
                species,
                options,
                sweep: &sweep,
            };
            chain_style = sample_chain(plan, &inputs, &view, nodes, own);
        }
        let mut chain_plan = ChainPlan {
            rings: start..plan.rings.len(),
            vertices: 0,
            indices: 0,
            vertex_start: plan.vertices,
            index_start: plan.indices,
            style: chain_style,
            holes: holes_start..plan.junctions.len(),
            own: own.filter(|_| plan.rings.len() > start),
        };
        let joins = plan.joins(&chain_plan);
        let (vertices, indices) =
            tube_counts(&plan.rings[start..], chain_style.closed_base, &joins);
        (chain_plan.vertices, chain_plan.indices) = (vertices, indices);
        plan.chains.push(chain_plan);
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
    /// Empty when the bark has no vertex colours.
    colors: &'a mut [[f32; 4]],
    indices: &'a mut [u32],
}

impl Run<'_> {
    /// Splits off the first `vertices` vertices and `indices` indices.
    fn split(self, vertices: usize, indices: usize) -> (Self, Self) {
        let (positions, positions_rest) = self.positions.split_at_mut(vertices);
        let (normals, normals_rest) = self.normals.split_at_mut(vertices);
        let (uvs, uvs_rest) = self.uvs.split_at_mut(vertices);
        let colored = vertices.min(self.colors.len());
        let (colors, colors_rest) = self.colors.split_at_mut(colored);
        let (indices, indices_rest) = self.indices.split_at_mut(indices);
        let head = Self {
            positions,
            normals,
            uvs,
            colors,
            indices,
        };
        let rest = Self {
            positions: positions_rest,
            normals: normals_rest,
            uvs: uvs_rest,
            colors: colors_rest,
            indices: indices_rest,
        };
        (head, rest)
    }
}

/// What every fill task reads.
#[derive(Clone, Copy)]
struct Filler<'a> {
    plan: &'a BarkPlan,
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
                colors: own.colors,
                indices: own.indices,
                base: chain.vertex_start as u32,
            };
            let rings = &self.plan.rings[chain.rings.clone()];
            let joins = self.plan.joins(chain);
            write_tube(rings, &chain.style, self.circles, &joins, &mut out);
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
    let colored = if plan.colored { plan.vertices } else { 0 };
    buffers.colors.resize(colored, [0.0; 4]);
    buffers.indices.resize(plan.indices, 0);
    circles.ensure(plan.max_sides);
    let filler = Filler { plan, circles };
    let run = Run {
        positions: &mut buffers.positions,
        normals: &mut buffers.normals,
        uvs: &mut buffers.uvs,
        colors: &mut buffers.colors,
        indices: &mut buffers.indices,
    };
    if parallel {
        filler.fill_parallel(&plan.chains, run);
    } else {
        filler.fill(&plan.chains, run);
    }
}

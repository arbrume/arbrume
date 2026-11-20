//! What branch-module instancing would save on a variant library, measured.
//!
//! Makowski et al. (2019) assemble every plant of a forest from a small set of reusable
//! branch modules, so that a module is stored once and drawn many times. Arbrume grows
//! each variant as one free graph, with no module structure, so this module asks how
//! far the branches of a library could be shared after the fact: every lateral axis of
//! a given order, with everything it bears, is a module; modules are compared by shape
//! and the library is reduced greedily to prototypes, a module joining the first
//! prototype it resembles within a tolerance.
//!
//! Shape: the live node positions of the module (at most [`SAMPLES`], evenly by index)
//! in a frame at its base: `y` along its first internode, `x` towards the centroid of
//! the module across `y`, all divided by the arc length of its axis. The error of
//! drawing module `a` with prototype `b` scaled to the length of `a` is the symmetric
//! mean nearest-point (chamfer) distance of their normalized points times the length of
//! `a`; a module joins a prototype when that error is at most `tolerance` times its
//! length and the two have the same number of axes. Vertex counts come from the mesh
//! provenance of level 0 (bark of the axes, leaves and twigs of their sites).
//!
//! The report gives the vertices of level 0 before and after replacing every module by
//! its prototype plus one 40-byte transform per module. It is a measurement: shared
//! modules would also need their junctions re-stitched to the bearing axis, and a
//! prototype changes the shape of the branches it replaces by the reported error.

use arbrume_core::graph::{ChainId, NodeId, PlantGraph};
use arbrume_core::mesh::{MeshProvenance, SemanticClass};
use glam::Vec3;

/// Most points kept per module for the comparison.
pub const SAMPLES: usize = 32;

/// One module: a lateral axis and everything it bears.
#[derive(Debug, Clone)]
struct Module {
    /// Normalized points in the frame of the module.
    points: Vec<Vec3>,
    /// Arc length of its axis, in meters.
    length: f32,
    /// Axes in the module.
    axes: usize,
    /// Vertices of level 0 in the module.
    vertices: usize,
}

/// The result for one tolerance.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ModuleReport {
    /// Order of the axes taken as modules.
    pub order: u8,
    /// Largest error over length accepted.
    pub tolerance: f32,
    /// Modules found.
    pub modules: usize,
    /// Prototypes kept.
    pub prototypes: usize,
    /// Vertices of level 0 of every plant.
    pub vertices_total: usize,
    /// Vertices of level 0 inside modules.
    pub vertices_in_modules: usize,
    /// Vertices stored after sharing: outside modules plus prototypes.
    pub vertices_shared: usize,
    /// Bytes of the module transforms, 40 per module.
    pub transform_bytes: usize,
    /// Mean error of the modules drawn by another prototype, in meters.
    pub mean_error: f32,
    /// Largest such error, in meters.
    pub max_error: f32,
}

/// Measures module sharing over `plants` (each a graph and the provenance of its
/// level-0 mesh), taking the axes of `order` as modules, for each tolerance.
#[must_use]
pub fn module_report(
    plants: &[(&PlantGraph, &MeshProvenance)],
    order: u8,
    tolerances: &[f32],
) -> Vec<ModuleReport> {
    let mut modules = Vec::new();
    let mut total = 0;
    for (graph, provenance) in plants {
        let per_chain = chain_vertices(graph, provenance);
        total += per_chain.iter().sum::<usize>();
        modules.extend(extract(graph, &per_chain, order));
    }
    modules.sort_by_key(|m| std::cmp::Reverse(m.vertices));
    tolerances
        .iter()
        .map(|&tolerance| share(&modules, order, tolerance, total))
        .collect()
}

/// Groups `modules` greedily into prototypes within `tolerance`.
fn share(modules: &[Module], order: u8, tolerance: f32, total: usize) -> ModuleReport {
    let mut prototypes: Vec<usize> = Vec::new();
    let (mut errors, mut count_errors, mut max_error) = (0.0, 0, 0.0f32);
    for (index, module) in modules.iter().enumerate() {
        let found = prototypes.iter().find_map(|&p| {
            let prototype = &modules[p];
            if prototype.axes != module.axes {
                return None;
            }
            let error = chamfer(&module.points, &prototype.points) * module.length;
            (error <= tolerance * module.length).then_some(error)
        });
        match found {
            Some(error) => {
                errors += error;
                count_errors += 1;
                max_error = max_error.max(error);
            }
            None => prototypes.push(index),
        }
    }
    let in_modules: usize = modules.iter().map(|m| m.vertices).sum();
    let kept: usize = prototypes.iter().map(|&p| modules[p].vertices).sum();
    ModuleReport {
        order,
        tolerance,
        modules: modules.len(),
        prototypes: prototypes.len(),
        vertices_total: total,
        vertices_in_modules: in_modules,
        vertices_shared: total - in_modules + kept,
        transform_bytes: 40 * modules.len(),
        mean_error: if count_errors == 0 {
            0.0
        } else {
            errors / count_errors as f32
        },
        max_error,
    }
}

/// Symmetric mean nearest-point distance of two point sets.
fn chamfer(a: &[Vec3], b: &[Vec3]) -> f32 {
    let one_way = |from: &[Vec3], to: &[Vec3]| {
        from.iter()
            .map(|p| {
                to.iter()
                    .map(|q| p.distance_squared(*q))
                    .fold(f32::INFINITY, f32::min)
                    .sqrt()
            })
            .sum::<f32>()
            / from.len().max(1) as f32
    };
    f32::midpoint(one_way(a, b), one_way(b, a))
}

/// Vertices of level 0 per chain: bark by chain, sites by the chain of their node.
fn chain_vertices(graph: &PlantGraph, provenance: &MeshProvenance) -> Vec<usize> {
    let mut counts = vec![0; graph.chains().len()];
    for (_, runs) in &provenance.parts {
        for run in runs {
            let chain = match run.class {
                SemanticClass::Twig | SemanticClass::Leaf => graph
                    .organs()
                    .get(run.instance as usize)
                    .map(|site| graph.chain_of(site.node).index()),
                _ => Some(run.instance as usize),
            };
            if let Some(slot) = chain.and_then(|c| counts.get_mut(c)) {
                *slot += run.len as usize;
            }
        }
    }
    counts
}

/// The modules of `graph`: every chain of `order` with the chains it bears.
fn extract(graph: &PlantGraph, per_chain: &[usize], order: u8) -> Vec<Module> {
    let chains = graph.chains();
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); chains.len()];
    for (index, chain) in chains.iter().enumerate() {
        if chain.attach != NodeId::NONE {
            children[graph.chain_of(chain.attach).index()].push(index);
        }
    }
    let mut modules = Vec::new();
    for (index, chain) in chains.iter().enumerate() {
        if chain.order != order || chain.attach == NodeId::NONE {
            continue;
        }
        let mut members = vec![index];
        let mut cursor = 0;
        while cursor < members.len() {
            members.extend(&children[members[cursor]]);
            cursor += 1;
        }
        if let Some(module) = module_of(graph, &members, per_chain) {
            modules.push(module);
        }
    }
    modules
}

/// The module made of the chains `members`, the first being its axis.
fn module_of(graph: &PlantGraph, members: &[usize], per_chain: &[usize]) -> Option<Module> {
    let axis = graph.chain_view(ChainId::new(members[0]))?;
    let first = *axis.positions.first()?;
    let length = axis.length();
    if length <= 0.0 {
        return None;
    }
    let origin = axis.origin;
    let mut points: Vec<Vec3> = members
        .iter()
        .filter_map(|&c| graph.chain_view(ChainId::new(c)))
        .flat_map(|view| view.positions.iter().copied())
        .collect();
    let stride = points.len().div_ceil(SAMPLES).max(1);
    points = points.into_iter().step_by(stride).collect();
    let y = (first - origin).normalize_or(Vec3::Y);
    let centroid = points.iter().copied().sum::<Vec3>() / points.len() as f32 - origin;
    let across = centroid - y * centroid.dot(y);
    let x = across
        .try_normalize()
        .unwrap_or_else(|| y.any_orthonormal_vector());
    let z = x.cross(y);
    let points = points
        .iter()
        .map(|p| {
            let d = *p - origin;
            Vec3::new(d.dot(x), d.dot(y), d.dot(z)) / length
        })
        .collect();
    Some(Module {
        points,
        length,
        axes: members.len(),
        vertices: members.iter().map(|&c| per_chain[c]).sum(),
    })
}

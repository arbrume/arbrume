//! A grown plant at an earlier age.
//!
//! Every metamer records the cycle of its birth and, when it was shed, the cycle of its
//! shedding, so one growth holds every earlier state of the plant. A [`GrowthRecord`]
//! is a plant grown with the leaf sites of all its metamers; [`GrowthRecord::at_cycle`]
//! gives the plant as it stood at the end of a cycle, to be meshed with
//! `MeshOptions::max_cycle` of `arbrume-mesh` set to that cycle, which leaves out what
//! was born later and the leaves older than the retention of the species:
//!
//! * the branches shed after the cycle are restored;
//! * the radii are those of the radius model on the structure of that cycle
//!   ([`radii_at_cycle`]): the pipe model from the tips the plant had then, or the
//!   tapered model with the lengths the axes had then;
//! * the leaves at the tip of an axis that had stopped by then are kept whatever their
//!   age, as they are on the grown plant.
//!
//! Positions are those of the grown plant, bending under its final weight included, so
//! an axis does not move from one age to the next. Whether an axis had stopped is read
//! from the grown plant: an axis that paused and grew again later counts as growing
//! throughout.

use arbrume_core::graph::{ChainId, PlantGraph};
use arbrume_core::species::{RadiusModel, Species};

/// A grown plant that can be shown at any earlier age. Made by
/// [`crate::grow_timeline`]. See the [module documentation](self).
#[derive(Debug, Clone, PartialEq)]
pub struct GrowthRecord {
    /// The grown graph, with the leaf sites of every metamer of the leaf-bearing
    /// orders: more than the grown plant shows.
    pub graph: PlantGraph,
    /// For each organ site of `graph`, the cycle from which it stays leafy for good;
    /// `u16::MAX` for a site that drops its leaves at the retention of the species.
    pub(crate) lasting: Vec<u16>,
}

impl GrowthRecord {
    /// The plant as it stood at the end of growth cycle `cycle`: a copy of the graph
    /// with the branches shed later restored and the radii of [`radii_at_cycle`]. Mesh
    /// it with `max_cycle = Some(cycle)`.
    ///
    /// `species` and `scale` are those the plant was grown with. The lasting leaf sites
    /// get `cycle` as their birth, so that the mesher keeps them.
    #[must_use]
    pub fn at_cycle(&self, species: &Species, scale: f32, cycle: u16) -> PlantGraph {
        let mut past = self.graph.clone();
        past.restore_shed_after(cycle);
        let mut radii = Vec::new();
        radii_at_cycle(&self.graph, species, scale, cycle, &mut radii);
        past.radii_mut().copy_from_slice(&radii);
        for (site, &lasting) in past.organs_mut().iter_mut().zip(&self.lasting) {
            if lasting <= cycle && site.birth <= cycle {
                site.birth = cycle;
            }
        }
        past
    }
}

/// Writes to `radii` the radius of every internode of `graph` as the plant was at the
/// end of growth cycle `cycle`; 0 for the metamers born later.
///
/// `graph` is the contiguous graph of a grown plant and `scale` the scale it was grown
/// with. The cost is one pass over the nodes. The pipe model counts the branches shed
/// before the cycle, like the radii of the grown plant.
pub fn radii_at_cycle(
    graph: &PlantGraph,
    species: &Species,
    scale: f32,
    cycle: u16,
    radii: &mut Vec<f32>,
) {
    radii.clear();
    radii.resize(graph.node_count(), 0.0);
    match species.geometry.radius_model {
        RadiusModel::Pipe => pipe(graph, species, scale, cycle, radii),
        RadiusModel::Tapered => tapered(graph, species, cycle, radii),
    }
}

/// Pipe model over the nodes born by `cycle`.
fn pipe(graph: &PlantGraph, species: &Species, scale: f32, cycle: u16, radii: &mut [f32]) {
    let exponent = species.geometry.pipe_exponent;
    let tip_area = (species.geometry.tip_radius * scale).powf(exponent);
    let (births, parents) = (graph.births(), graph.parents());
    for node in (0..radii.len()).rev() {
        if births[node] > cycle {
            radii[node] = 0.0;
            continue;
        }
        let area = if radii[node] > 0.0 {
            radii[node]
        } else {
            tip_area
        };
        radii[node] = area.powf(exponent.recip());
        if parents[node].is_some() {
            radii[parents[node].index()] += area;
        }
    }
}

/// Chain that `chain` relays, when it continues an axis of the same order.
fn relayed(graph: &PlantGraph, chain: ChainId) -> Option<ChainId> {
    let parent = graph.parent_chain(chain)?;
    (graph.chain(parent).order == graph.chain(chain).order).then_some(parent)
}

/// Tapered model with the lengths of the axes at `cycle`. Lengths are measured on the
/// graph, so they carry its scale.
fn tapered(graph: &PlantGraph, species: &Species, cycle: u16, radii: &mut [f32]) {
    let geometry = &species.geometry;
    let births = graph.births();
    let mut arcs = vec![0.0_f32; graph.node_count()];
    let mut lengths = vec![0.0_f32; graph.chain_count()];
    for id in graph.chain_ids() {
        let chain = graph.chain(id);
        let mut arc = relayed(graph, id).map_or(0.0, |_| arcs[chain.attach.index()]);
        for node in chain.first.index()..chain.first.index() + chain.len as usize {
            if births[node] > cycle {
                break;
            }
            arc += graph.internode_length(arbrume_core::graph::NodeId::new(node));
            arcs[node] = arc;
        }
        lengths[id.index()] = arc;
    }
    for id in graph.chain_ids().rev() {
        if let Some(parent) = relayed(graph, id) {
            lengths[parent.index()] = lengths[parent.index()].max(lengths[id.index()]);
        }
    }
    let mut bases = vec![0.0_f32; graph.chain_count()];
    for id in graph.chain_ids() {
        let chain = graph.chain(id);
        let length = lengths[id.index()];
        let base = match graph.parent_chain(id) {
            None => geometry.radius_ratio * length,
            Some(parent) if relayed(graph, id).is_some() => bases[parent.index()],
            Some(parent) => {
                let ratio = if lengths[parent.index()] > 0.0 {
                    length / lengths[parent.index()]
                } else {
                    0.0
                };
                let derived = bases[parent.index()] * ratio.powf(geometry.child_radius_exponent);
                derived.min(radii[chain.attach.index()])
            }
        };
        bases[id.index()] = base;
        for node in chain.first.index()..chain.first.index() + chain.len as usize {
            if births[node] > cycle {
                break;
            }
            let position = if length > 0.0 {
                (arcs[node] / length).min(1.0)
            } else {
                1.0
            };
            radii[node] = base * geometry.taper.eval(chain.order, position);
        }
    }
}

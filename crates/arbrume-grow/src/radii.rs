//! Radius pass: pipe model or tapered model, as the species asks.
//!
//! The pipe model (Shinozaki et al. 1964) is the basipetal pass of the core:
//! `r^n = sum of r_child^n` from `tip_radius` at the tips, with the exponent of the
//! species (2 is the rule of Leonardo, see Eloy 2011).
//!
//! The tapered model follows Weber and Penn (1995):
//!
//! * the base radius of a stem is `radius_ratio` times its length;
//! * the base radius of a lateral is that of its parent times
//!   `(length / parent length)^child_radius_exponent`, and never more than the radius of
//!   the parent where the lateral is attached;
//! * along an axis the radius is the base radius times `taper` at the relative position.
//!
//! Lengths are those of the apparent axes at the age of the plant: a relay continues the
//! arc length and keeps the base radius of the axis it relays, and the length of an axis
//! runs to the farthest tip of its relays. The flare of the trunk base is left to the
//! mesher.

use crate::plant::Plant;
use arbrume_core::graph::{ChainId, NodeId};
use arbrume_core::species::{RadiusModel, Species};

/// Sets every internode radius of a contiguous plant. `scale` is the factor already
/// applied to the lengths of the graph.
pub(crate) fn assign_radii(plant: &mut Plant, species: &Species, scale: f32) {
    let geometry = &species.geometry;
    match geometry.radius_model {
        RadiusModel::Pipe => plant
            .graph
            .pipe_radii(geometry.pipe_exponent, geometry.tip_radius * scale),
        RadiusModel::Tapered => {
            let radii = tapered_radii(plant, species, scale);
            plant.graph.radii_mut().copy_from_slice(&radii);
        }
    }
}

/// Chain that `chain` relays, when it is a relay.
fn relayed(plant: &Plant, chain: ChainId) -> Option<ChainId> {
    let graph = &plant.graph;
    let parent = graph.parent_chain(chain)?;
    (graph.chain(parent).order == graph.chain(chain).order).then_some(parent)
}

/// Unscaled length of the apparent axis of each chain, up to the farthest tip of its
/// relays.
fn apparent_lengths(plant: &Plant) -> Vec<f32> {
    let mut lengths: Vec<f32> = plant
        .chain_meristems
        .iter()
        .map(|&id| plant.meristem(id).length)
        .collect();
    for chain in plant.graph.chain_ids().rev() {
        if let Some(parent) = relayed(plant, chain) {
            lengths[parent.index()] = lengths[parent.index()].max(lengths[chain.index()]);
        }
    }
    lengths
}

/// Radii of the tapered model, one per node.
fn tapered_radii(plant: &Plant, species: &Species, scale: f32) -> Vec<f32> {
    let (graph, geometry) = (&plant.graph, &species.geometry);
    let lengths = apparent_lengths(plant);
    let mut bases = vec![0.0_f32; graph.chain_count()];
    let mut radii = vec![0.0_f32; graph.node_count()];
    for id in graph.chain_ids() {
        let chain = graph.chain(id);
        let length = lengths[id.index()];
        let base = match graph.parent_chain(id) {
            None => geometry.radius_ratio * length * scale,
            Some(parent) if relayed(plant, id).is_some() => bases[parent.index()],
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
        let mut arc = plant.meristem(plant.meristem_of(id)).start;
        let first = chain.first.index();
        let nodes = &mut radii[first..first + chain.len as usize];
        for (offset, radius) in nodes.iter_mut().enumerate() {
            arc += graph.internode_length(NodeId::new(first + offset)) / scale;
            let position = if length > 0.0 {
                (arc / length).min(1.0)
            } else {
                1.0
            };
            *radius = base * geometry.taper.eval(chain.order, position);
        }
    }
    radii
}

//! The root system of a generated plant.

use arbrume_core::graph::PlantGraph;
use arbrume_core::species::Species;
use arbrume_grow::{Soil, Surroundings, add_roots};

use crate::params::GenParams;

/// Homogeneous soil under flat ground, the soil of a plant whose parameters give none.
static HOMOGENEOUS: Soil = Soil::homogeneous();

/// `graph`, grown from `species` and `params` in `surroundings`, with the root system of
/// the species when it has a `[roots]` layer (`arbrume_grow::add_roots`), in the soil
/// of `params`. A species without roots gets `graph` back unchanged.
pub(crate) fn add(
    species: &Species,
    params: &GenParams,
    surroundings: &Surroundings,
    mut graph: PlantGraph,
) -> PlantGraph {
    if let Some(roots) = &species.roots {
        let soil = params.soil.as_deref().unwrap_or(&HOMOGENEOUS);
        add_roots(
            &mut graph,
            species,
            roots,
            &params.grow_params(),
            (surroundings, soil),
        );
    }
    graph
}

/// [`add`] for the frame of `cycle` of a growth sequence: the root system of the plant
/// grown for `cycle + 1` cycles, the age of the plant at the end of that cycle, under the
/// shoot `graph` of that frame. At the last cycle this is the plant of [`add`].
pub(crate) fn add_at_cycle(
    species: &Species,
    params: &GenParams,
    cycle: u16,
    graph: PlantGraph,
) -> PlantGraph {
    if species.roots.is_none() {
        return graph;
    }
    let open = Surroundings::none();
    let surroundings = params.surroundings.as_deref().unwrap_or(&open);
    let aged = GenParams {
        age: cycle.saturating_add(1).min(params.age),
        ..params.clone()
    };
    add(species, &aged, surroundings, graph)
}

//! Growth with the guides of the parameters, when they have strokes or freezes.

use arbrume_core::graph::PlantGraph;
use arbrume_core::species::Species;
use arbrume_grow::{
    GrowthRecord, GuideReport, Surroundings, grow_guided, grow_guided_timeline, grow_in,
    grow_timeline,
};

use crate::params::GenParams;

/// The graph of the plant of `params` in `surroundings`, and the report of its guides
/// when it has some.
pub(crate) fn grow(
    species: &Species,
    params: &GenParams,
    surroundings: &Surroundings,
) -> (PlantGraph, Option<GuideReport>) {
    let grow_params = params.grow_params();
    match params.active_guides() {
        Some(guides) => {
            let (graph, report) = grow_guided(species, &grow_params, surroundings, guides);
            (graph, Some(report))
        }
        None => (grow_in(species, &grow_params, surroundings), None),
    }
}

/// [`grow`] keeping every earlier state of the plant (`arbrume_grow::grow_timeline`).
pub(crate) fn grow_record(
    species: &Species,
    params: &GenParams,
    surroundings: &Surroundings,
) -> (GrowthRecord, Option<GuideReport>) {
    let grow_params = params.grow_params();
    match params.active_guides() {
        Some(guides) => {
            let (record, report) =
                grow_guided_timeline(species, &grow_params, surroundings, guides);
            (record, Some(report))
        }
        None => (grow_timeline(species, &grow_params, surroundings), None),
    }
}

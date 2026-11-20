//! Aquatic plants: growth under water, up to and along its surface.
//!
//! A species with an `[aquatic]` layer grows in the cycle of its growth mode with two
//! stages wrapped, then its grown graph is dressed by passes that depend on the water:
//!
//! * [`Submerged`] wraps the orientation stage: below the surface buoyancy pulls every
//!   shoot up, blended after the tropism of the species; at the surface an axis either
//!   spreads along it (a kelp canopy) or stops (a petiole under its pad);
//! * [`Attenuated`] wraps the environment stage: the exposure of each bud is multiplied
//!   by the fraction of light that reaches its depth, `exp(-Kd depth)` (Beer-Lambert),
//!   so in the self-organizing mode growth and shedding favour shoots near the surface;
//! * [`dress`] bends the stems with the forces of the water (buoyancy and the drag of a
//!   steady current) in one deterministic pass like the bending under weight, floats
//!   the canopy and the leaves on the surface, adds the floats at the tips, the pads
//!   and the haptera of a holdfast.
//!
//! A species with `[aquatic.rhizome]` grows a meadow instead ([`grow_meadow`]): a
//! rhizome laid on the bottom that bears a ramet, a shoot of the species, at each node.
//!
//! The water is that of the surroundings ([`crate::Water`]), or still water as deep as
//! the `water_depth` of the species. Everything runs on the calling thread, from keys
//! derived from the seed: the plant is the same for every thread count.

mod float;
mod forces;
mod holdfast;
mod light;
mod meadow;
mod orient;
mod surface;

pub use light::Attenuated;
pub use meadow::grow_meadow;
pub use orient::Submerged;

use crate::GrowParams;
use crate::competition::{Competition, EngineOptions};
use crate::confine::Confined;
use crate::cycle::run_cycles;
use crate::parametric::{GradientFate, TropismBlend};
use crate::plant::Plant;
use crate::stage::{Context, NoEnvironment, NoShedding, Stages};
use crate::surroundings::Surroundings;
use crate::water::Water;
use crate::{ExposureShedding, ResourceFate};
use arbrume_core::graph::PlantGraph;
use arbrume_core::species::{Aquatic, GrowthMode, Species};

/// Thickness of the band below the surface in which a node counts as being at the
/// surface, in meters, on top of the radius of its internode.
pub(crate) const SURFACE_BAND: f32 = 0.03;

/// The water a plant of `aquatic` grows in: that of `surroundings`, or still water;
/// its surface at `aquatic.water_depth` when the surroundings give none.
#[must_use]
pub fn water_of(aquatic: &Aquatic, surroundings: &Surroundings) -> Water {
    let water = surroundings
        .water
        .unwrap_or_else(|| Water::still(aquatic.water_depth));
    Water {
        surface: Some(water.surface.unwrap_or(aquatic.water_depth)),
        ..water
    }
}

/// The stages of an aquatic species in the self-organizing mode.
pub type AquaticStages<'a> = Stages<
    Attenuated<Competition<'a>>,
    ResourceFate<'a>,
    Confined<'a, Submerged<TropismBlend<'a>>>,
    ExposureShedding,
>;

/// Runs the cycles of an aquatic species in the mode it selects. The species must have
/// an `[aquatic]` layer.
pub(crate) fn grow_aquatic<'a>(
    cx: &Context<'a>,
    surroundings: &'a Surroundings,
    options: EngineOptions,
    aquatic: &Aquatic,
) -> Plant {
    let water = water_of(aquatic, surroundings);
    let submerged = Submerged::new(TropismBlend::new(*cx), cx, aquatic, water);
    match cx.species.growth.mode {
        GrowthMode::Parametric => {
            let mut stages = Stages {
                environment: Attenuated::new(NoEnvironment, water),
                fate: GradientFate::new(*cx),
                orientation: submerged,
                shedding: NoShedding,
            };
            run_cycles(cx, &mut stages)
        }
        GrowthMode::SelfOrganizing => {
            let mut stages: AquaticStages = Stages {
                environment: Attenuated::new(Competition::new(cx, surroundings, options), water),
                fate: ResourceFate::new(*cx, surroundings),
                orientation: Confined::new(submerged, cx, surroundings),
                shedding: ExposureShedding::new(cx),
            };
            run_cycles(cx, &mut stages)
        }
    }
}

/// Grows a plant of `species`, which must have an `[aquatic]` layer, in
/// `surroundings`: a meadow when the species has a rhizome ([`grow_meadow`]), a single
/// plant otherwise. The graph is finished, dressed and contiguous.
#[must_use]
pub fn grow_underwater(
    species: &Species,
    params: &GrowParams,
    surroundings: &Surroundings,
    options: EngineOptions,
) -> PlantGraph {
    let has_rhizome = species
        .aquatic
        .as_ref()
        .is_some_and(|aquatic| aquatic.rhizome.is_some());
    if has_rhizome {
        grow_meadow(species, params, surroundings, options)
    } else {
        grow_single(species, params, surroundings, options)
    }
}

/// One aquatic plant, without its rhizome: grown, finished and dressed. A species
/// without an `[aquatic]` layer grows as on land.
pub(crate) fn grow_single(
    species: &Species,
    params: &GrowParams,
    surroundings: &Surroundings,
    options: EngineOptions,
) -> PlantGraph {
    let cx = params.context(species);
    let Some(aquatic) = &species.aquatic else {
        return crate::grow_in_with(species, params, surroundings, options);
    };
    let plant = grow_aquatic(&cx, surroundings, options, aquatic);
    let mut graph = crate::finish(plant, &cx);
    dress(&mut graph, &cx, surroundings);
    graph
}

/// Dresses the grown, finished graph of an aquatic species in its water: bends it with
/// buoyancy and current, floats the canopy and the leaves on the surface, adds the tip
/// floats, the pads and the holdfast. Leaves the graph contiguous. Does nothing for a
/// species without an `[aquatic]` layer.
pub fn dress(graph: &mut PlantGraph, cx: &Context, surroundings: &Surroundings) {
    let Some(aquatic) = &cx.species.aquatic else {
        return;
    };
    if graph.node_count() == 0 {
        return;
    }
    let water = water_of(aquatic, surroundings);
    let canopy = surface::canopy(graph, &water);
    if aquatic.pads {
        surface::add_pads(graph, &water);
    }
    forces::bend_in_water(graph, cx.species, &water, &canopy);
    surface::relay(graph, &water, &canopy, aquatic.leaf_flex);
    if let Some(tip) = &aquatic.tip_float {
        float::add_tip_floats(graph, cx, tip, &water);
    }
    if aquatic.pads {
        surface::lift_pads(graph, &water);
    }
    surface::orient_leaves(graph, cx.species, aquatic, &water);
    if let Some(holdfast) = &aquatic.holdfast {
        holdfast::add_holdfast(graph, cx, holdfast);
    }
    graph.ensure_contiguous();
}

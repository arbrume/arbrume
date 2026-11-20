//! Root systems, grown in the same plant graph and by the same growth cycle as shoots.
//!
//! A species with a `[roots]` layer ([`Roots`]) grows a root system below its collar.
//! [`grow_rooted`] grows the shoot as [`crate::grow_in`] does, then [`add_roots`]:
//!
//! 1. **Collar.** The plant is lowered so that the first node of its stem, the collar,
//!    is on the ground (`y = 0`): the first internode of the stem goes under the ground
//!    and becomes the base of the stem, where shoot and roots meet.
//! 2. **Growth.** The root system is grown by the growth cycle of the shoots
//!    ([`crate::run_cycle`]), with the species built from `[roots]`, in the frame of the
//!    plant turned upside down about the ground ([`frame`]): there the roots rise into
//!    the soil as shoots rise into the air, and the stages of the shoots apply as they
//!    are. The primary roots leave the collar spread around it. In the parametric mode
//!    the stages are those of the shoots ([`crate::GradientFate`]) with an orientation
//!    that bends roots towards resources and around obstacles ([`orient`]); in the
//!    self-organizing mode root tips compete for the resources of the soil
//!    ([`sense`], [`crate::ResourceFate`]).
//! 3. **Graft.** The root graph is turned back and copied into the graph of the shoot,
//!    each primary root a lateral axis of the collar, each node flagged
//!    `NodeFlags::ROOT`; the trunk base takes the pipes of both systems, the primaries
//!    flare near the collar, and roots are kept under the ground or exposed
//!    ([`graft`]).
//!
//! The soil ([`Soil`]) gives the ground surface and the resource patches; every
//! blocking occluder of the [`Surroundings`] is an obstacle to the roots too.
//!
//! # What the model of Li et al. (2023) does and this one does not
//!
//! The root system follows the spirit of Rhizomorph (Li et al. 2023): root tips grow
//! towards the resources of the soil and compete for them, obstacles and the ground
//! surface shape the system, and the habits (tap, heart, plate, sinkers) are parameter
//! sets of one model. The coupling of shoot and root is reduced to the pipe model at the
//! collar. Not modelled:
//!
//! * the long-distance signals and the plant vigor `min(root flux, shoot flux)` split
//!   between shoot and root by their maintenance (their eqs. 1 to 17): shoot and root
//!   grow each with its own resource, the root system after the shoot;
//! * the soil as a field of water and nutrients that diffuse and are carried by
//!   gravity (their eqs. 25 to 31): here the resource is static, homogeneous with
//!   richer patches, depleted only where root tips take it up;
//! * the growth resistance of the soil, which shortens internodes (their eqs. 5 and
//!   22), and temperature;
//! * bud activation probabilities from signals (their eqs. 20 and 24): laterals form
//!   with the branching probability of the species;
//! * the switch of tropism at every node with a probability falling with the distance
//!   to the base (their eq. 23): a lateral root is drawn a sinker once;
//! * root shedding and turnover.
//!
//! In return roots never enter an obstacle or the air, which Li et al. state their
//! model does not prevent.
//!
//! # Determinism
//!
//! The root system depends on the species, the parameters, the surroundings, the soil
//! and the shoot. Every random draw is keyed by the meristem keys of the roots, derived
//! from the seed of the plant and a salt, so the roots never change the draws of the
//! shoot. The only parallel pass is the association of resource points to root tips,
//! which gives the same result on any number of threads ([`crate::Markers`]).

pub(crate) mod frame;
pub(crate) mod graft;
pub(crate) mod orient;
mod seed;
pub(crate) mod sense;
mod soil;
pub(crate) mod species;

pub use soil::{Ground, HeightField, Resource, Soil};

use crate::cycle::run_cycle;
use crate::parametric::GradientFate;
use crate::plant::{Plant, seed_key};
use crate::stage::{BudFate, Context, Environment, NoShedding, Shedding, ShootOrientation, Stages};
use crate::surroundings::Surroundings;
use crate::{GrowParams, ResourceFate, finish, grow_in};
use arbrume_core::graph::PlantGraph;
use arbrume_core::rng::mix64;
use arbrume_core::species::{GrowthMode, Roots, Species};
use frame::SoilView;
use glam::Vec3;
use orient::RootOrientation;
use sense::SoilSensing;

/// Salt of the seed of a root system, so that its draws differ from those of the shoot.
const ROOT_SALT: u64 = 0x524f_4f54_5359_5354;

/// Grows a plant of `species` in `surroundings` as [`crate::grow_in`] does and, when the
/// species has a `[roots]` layer, its root system in `soil` ([`add_roots`]). Without
/// that layer the result is exactly that of [`crate::grow_in`].
#[must_use]
pub fn grow_rooted(
    species: &Species,
    params: &GrowParams,
    surroundings: &Surroundings,
    soil: &Soil,
) -> PlantGraph {
    let mut graph = grow_in(species, params, surroundings);
    if let Some(roots) = &species.roots {
        add_roots(&mut graph, species, roots, params, (surroundings, soil));
    }
    graph
}

/// Grows the root system `roots` of the shoot `graph`, grown from `species` and
/// `params`, in `surroundings` and `soil`, and joins it to the shoot. See the [module
/// documentation](self). A graph without a stem is left as it is.
pub fn add_roots(
    graph: &mut PlantGraph,
    species: &Species,
    roots: &Roots,
    params: &GrowParams,
    (surroundings, soil): (&Surroundings, &Soil),
) {
    let collars = graft::collars(graph);
    let Some(lowest) = collars
        .iter()
        .map(|node| graph.positions()[node.index()].y)
        .reduce(f32::min)
    else {
        return;
    };
    graph.map_points(|point| point - Vec3::Y * lowest);
    let root_species = species::root_species(species, roots);
    let sinkers = species::sinker_species(&root_species);
    let cx = Context {
        species: &root_species,
        seed: mix64(params.seed ^ ROOT_SALT),
        age: params.age,
        scale: params.scale,
    };
    let unit = root_species.growth.internode_unit() * params.scale;
    let reach = root_reach(&root_species) * params.scale + unit;
    let view = SoilView::new(soil, surroundings, reach, unit);
    let starts: Vec<Vec3> = collars
        .iter()
        .map(|node| view.mirror.point(graph.positions()[node.index()]))
        .collect();
    let mut system = grow_system(
        &cx,
        &Context {
            species: &sinkers,
            ..cx
        },
        &view,
        (&starts, roots),
    );
    let mirror = view.mirror;
    system.map_points(|point| mirror.point(point));
    graft::mark(&mut system);
    let pipes = (roots.pipe_exponent, roots.pipe_balance);
    graft::balance(&mut system, graph, &collars, pipes);
    let sums = graft::collar_pipes(&system, graph, &collars, roots.pipe_exponent);
    graft::flare(&mut system, roots, params.scale);
    graft::cover(&mut system, soil, roots, params.scale);
    let seat = graft::Seat {
        flare: species.geometry.flare,
        collar_flare: species.geometry.collar_flare,
        radii: graft::coupled(graph, &sums, roots.pipe_exponent),
    };
    graft::graft(graph, &system, &collars, &seat);
    graft::couple_radii(graph, &sums, roots.pipe_exponent);
    graft::clear_stems(graph, species.geometry.flare);
    graft::seat(graph, &collars);
}

/// Farthest a root can reach from the collar at scale 1: the longest primary and the
/// longest lateral, end to end.
fn root_reach(species: &Species) -> f32 {
    let lengths = &species.growth.axis_length;
    let primary = lengths.order(0).bounds().1;
    let lateral = if species.architecture.max_order > 0 {
        lengths.order(1).bounds().1
    } else {
        0.0
    };
    primary + lateral
}

/// Grows the root system of the run `cx` in the turned frame of `view`, its primaries
/// leaving `starts`, and returns its graph, still turned.
fn grow_system(
    cx: &Context,
    sinkers: &Context,
    view: &SoilView,
    (starts, roots): (&[Vec3], &Roots),
) -> PlantGraph {
    let tap = roots.tap_root.then(|| seed_key(cx.seed, 0));
    let orientation = RootOrientation::new(*cx, *sinkers, view, (roots.sinker_probability, tap));
    match roots.mode {
        GrowthMode::Parametric => {
            let mut stages = Stages {
                environment: SoilSensing::new(cx, view, None, true),
                fate: GradientFate::new(*cx),
                orientation,
                shedding: NoShedding,
            };
            run_system(cx, &mut stages, starts, (roots.primaries, roots.tap_root))
        }
        GrowthMode::SelfOrganizing => {
            let growth = &cx.species.growth;
            let model = &growth.self_organization;
            let unit = growth.internode_unit() * cx.scale;
            let density = model.marker_density / (unit * unit * unit);
            let reach = root_reach(cx.species) * cx.scale + unit;
            let points = sense::scatter(view, cx.seed, density, model.marker_limit, reach);
            let mut stages = Stages {
                environment: SoilSensing::new(cx, view, Some(points), true),
                fate: ResourceFate::new(*cx, &view.turned),
                orientation,
                shedding: NoShedding,
            };
            run_system(cx, &mut stages, starts, (roots.primaries, roots.tap_root))
        }
    }
}

/// Seeds the primary roots at `starts` (their number, and whether the first is a tap root), runs every cycle of `cx` and finishes the
/// graph: contiguous chains and the pipe radii of the root species.
fn run_system<E, F, O, S>(
    cx: &Context,
    stages: &mut Stages<E, F, O, S>,
    starts: &[Vec3],
    primaries: (u8, bool),
) -> PlantGraph
where
    E: Environment,
    F: BudFate,
    O: ShootOrientation,
    S: Shedding,
{
    let mut plant = Plant::new();
    seed::seed_primaries(&mut plant, cx, &mut stages.fate, starts, primaries);
    for cycle in 0..cx.age {
        run_cycle(&mut plant, cx, stages, cycle);
    }
    finish(plant, cx)
}

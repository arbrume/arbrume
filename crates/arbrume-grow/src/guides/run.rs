//! Growing a plant with guides: the stages of the self-organizing mode wrapped by the
//! guided environment, the steered orientation and the frozen shedding, run in phases
//! that each add the erase strokes starting then to the blocking volumes.

use std::cell::RefCell;
use std::rc::Rc;

use crate::allocation::ResourceFate;
use crate::competition::{Competition, EngineOptions};
use crate::confine::Confined;
use crate::cycle::{run_cycle, seed_plant};
use crate::history::GrowthRecord;
use crate::parametric::TropismBlend;
use crate::plant::Plant;
use crate::shedding::ExposureShedding;
use crate::stage::{Context, Shedding, Stages};
use crate::surroundings::{Occluder, Shape, Surroundings};
use crate::{GrowParams, finish, finish_with, grow_in_with};
use arbrume_core::graph::PlantGraph;
use arbrume_core::grid::HashGrid;
use arbrume_core::species::{GrowthMode, Species};
use glam::Vec3;

use super::sense::Guided;
use super::steer::{Steered, Steering};
use super::stroke::stroke_balls;
use super::{GuideReport, Guides, StrokeKind, StrokeReport};

/// Grows a plant of `species` in `surroundings`, steered by `guides`. Without strokes
/// and freezes, or for a parametric species, the plant is that of [`crate::grow_in`]
/// and the report only gives the length of each limb stroke. The guides must be valid
/// ([`Guides::validate`]).
#[must_use]
pub fn grow_guided(
    species: &Species,
    params: &GrowParams,
    surroundings: &Surroundings,
    guides: &Guides,
) -> (PlantGraph, GuideReport) {
    grow_guided_with(
        species,
        params,
        surroundings,
        guides,
        EngineOptions::default(),
    )
}

/// Same as [`grow_guided`] with explicit engine options, which change the time and
/// memory taken but never the plant.
#[must_use]
pub fn grow_guided_with(
    species: &Species,
    params: &GrowParams,
    surroundings: &Surroundings,
    guides: &Guides,
    options: EngineOptions,
) -> (PlantGraph, GuideReport) {
    let cx = params.context(species);
    if !guided(species, guides) {
        let graph = grow_in_with(species, params, surroundings, options);
        return (graph, unguided_report(guides));
    }
    let (plant, report) = grow_plant(&cx, surroundings, guides, options);
    (finish(plant, &cx), report)
}

/// Same as [`grow_guided`], keeping the leaf sites of every metamer so that the plant can
/// be shown at any earlier age, as [`crate::grow_timeline`] does.
#[must_use]
pub fn grow_guided_timeline(
    species: &Species,
    params: &GrowParams,
    surroundings: &Surroundings,
    guides: &Guides,
) -> (GrowthRecord, GuideReport) {
    if !guided(species, guides) {
        let record = crate::grow_timeline(species, params, surroundings);
        return (record, unguided_report(guides));
    }
    let cx = params.context(species);
    let (plant, report) = grow_plant(&cx, surroundings, guides, EngineOptions::default());
    let mut lasting = Vec::new();
    let (graph, shown) = finish_with(plant, &cx, Some(&mut lasting));
    let record = GrowthRecord {
        graph,
        lasting,
        shown,
        age: params.age,
    };
    (record, report)
}

/// Whether `guides` change the plant of `species`.
fn guided(species: &Species, guides: &Guides) -> bool {
    !guides.is_empty() && species.growth.mode == GrowthMode::SelfOrganizing
}

/// The report of guides that did nothing.
fn unguided_report(guides: &Guides) -> GuideReport {
    let strokes = guides.strokes.iter().map(|stroke| StrokeReport {
        length: super::Polyline::new(&stroke.points).length(),
        ..StrokeReport::default()
    });
    GuideReport {
        strokes: strokes.collect(),
    }
}

/// Shedding by exposure until the first freeze, none from it on.
#[derive(Debug, Clone)]
struct Frozen<S> {
    inner: S,
    from: Option<u16>,
}

impl<S: Shedding> Shedding for Frozen<S> {
    fn shed(&mut self, plant: &mut Plant, cycle: u16) {
        if self.from.is_none_or(|from| cycle < from) {
            self.inner.shed(plant, cycle);
        }
    }
}

/// The surroundings of each phase of the run, with the cycle the phase starts: those
/// given, plus a blocking ball for each ball of every erase stroke acting by then.
fn phases(surroundings: &Surroundings, guides: &Guides) -> Vec<(u16, Surroundings)> {
    let mut starts: Vec<u16> = guides
        .strokes
        .iter()
        .filter(|stroke| stroke.kind == StrokeKind::Erase)
        .map(|stroke| stroke.cycle)
        .collect();
    starts.push(0);
    starts.sort_unstable();
    starts.dedup();
    starts
        .into_iter()
        .map(|start| {
            let mut around = surroundings.clone();
            for stroke in &guides.strokes {
                if stroke.kind != StrokeKind::Erase || stroke.cycle > start {
                    continue;
                }
                for center in stroke_balls(&stroke.points, stroke.radius) {
                    let radius = stroke.radius;
                    around.occluders.push(Occluder {
                        shape: Shape::Sphere { center, radius },
                        blocks: true,
                        shade: 0.0,
                    });
                }
            }
            (start, around)
        })
        .collect()
}

/// Runs the cycles of the run `cx` with guides and reports what each stroke did.
fn grow_plant(
    cx: &Context,
    surroundings: &Surroundings,
    guides: &Guides,
    options: EngineOptions,
) -> (Plant, GuideReport) {
    let phases = phases(surroundings, guides);
    let steering = Rc::new(RefCell::new(Steering::new(&guides.strokes)));
    let competition = Competition::new(cx, surroundings, options);
    let shared = Rc::clone(&steering);
    let mut environment = Guided::new(competition, *cx, guides, shared, options.parallel_markers);
    let mut shedding = Frozen {
        inner: ExposureShedding::new(cx),
        from: guides.freezes.first().copied(),
    };
    let mut plant = seed_plant(cx, &mut ResourceFate::new(*cx, &phases[0].1));
    for (index, (start, around)) in phases.iter().enumerate() {
        let end = phases
            .get(index + 1)
            .map_or(cx.age, |next| next.0.min(cx.age));
        if *start >= end {
            continue;
        }
        let steered = Steered::new(TropismBlend::new(*cx), Rc::clone(&steering), cx.scale);
        let mut stages = Stages {
            environment,
            fate: ResourceFate::new(*cx, around),
            orientation: Confined::new(steered, cx, around),
            shedding,
        };
        for cycle in *start..end {
            run_cycle(&mut plant, cx, &mut stages, cycle);
        }
        (environment, shedding) = (stages.environment, stages.shedding);
    }
    let report = report(&plant, guides, &environment, &steering.borrow());
    (plant, report)
}

/// What each stroke did to the grown `plant`.
fn report<E: crate::stage::Environment>(
    plant: &Plant,
    guides: &Guides,
    environment: &Guided<'_, E>,
    steering: &Steering,
) -> GuideReport {
    let graph = &plant.graph;
    let live: Vec<Vec3> = graph
        .positions()
        .iter()
        .zip(graph.flags())
        .filter(|(_, flags)| !flags.is_shed())
        .map(|(position, _)| *position)
        .collect();
    let occupancy = environment.occupancy();
    let grid = HashGrid::build(&live, occupancy);
    let mut strokes = unguided_report(guides).strokes;
    for (index, entry) in strokes.iter_mut().enumerate() {
        let markers = &environment.scattered[index];
        entry.markers = markers.len() as u32;
        let near = |marker: &&Vec3| grid.nearest_in_radius(**marker, occupancy).is_some();
        entry.reached = markers.iter().filter(near).count() as u32;
        entry.pruned = environment.pruned[index];
    }
    for limb in &steering.limbs {
        let entry = &mut strokes[limb.stroke];
        entry.claimed = !limb.followers.is_empty();
        entry.followed = (limb.progress - limb.start).max(0.0);
        let mut since = vec![None; graph.chain_count()];
        for &(id, cycle) in &limb.followers {
            let chain = plant.meristem(id).chain;
            if chain.is_some() {
                since[chain.index()] = Some(cycle);
            }
        }
        let (mut sum, mut count, mut largest) = (0.0, 0_u32, 0.0_f32);
        let (chains, births, flags) = (graph.node_chains(), graph.births(), graph.flags());
        for (node, position) in graph.positions().iter().enumerate() {
            let followed = since[chains[node].index()].is_some_and(|cycle| births[node] >= cycle);
            if followed && !flags[node].is_shed() {
                let distance = limb.line.distance(*position);
                (sum, count, largest) = (sum + distance, count + 1, largest.max(distance));
            }
        }
        entry.mean_distance = if count > 0 { sum / count as f32 } else { 0.0 };
        entry.max_distance = largest;
    }
    GuideReport { strokes }
}

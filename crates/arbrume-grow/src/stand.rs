//! Stands: several plants grown in one environment, sharing their light.
//!
//! [`grow_together`] grows the plants of a stand in lockstep. Every cycle has three
//! phases, each done for all the plants before the next starts:
//!
//! 1. every self-organizing plant that senses light casts the shadow of its new metamers
//!    into one shared [`ShadowGrid`], with the coefficients of its own species
//!    ([`ShadowGrid::set_coefficients`]), and takes back that of the metamers it shed;
//! 2. every plant senses the grid, which then holds the shade of the whole stand as it
//!    was at the end of the previous cycle;
//! 3. every plant allocates, grows its shoots and sheds, on its own (in parallel on the
//!    `rayon` pool).
//!
//! No plant sees the growth of another within the same cycle, so the order of the
//! entries does not matter. The casts of phase 1 are summed in an order that depends
//! only on the entries (sorted by position, then seed, age, scale and species name),
//! never on the order they are given in, so the result does not change by a single bit
//! when the entries are permuted; entries equal in all these keys keep their order.
//!
//! The grid has the voxels of the finest species of the stand. The pyramid of a species
//! with larger voxels is rescaled to them: same depth in meters, same fade per meter
//! ([`crate::ShadowParams::in_cell`]). A bud reads its exposure with the full exposure
//! `C` and the strength `a` of its own species.
//!
//! Phase 1 is the sensing of the [`Competition`] stage, which casts before it reads, so
//! a stand with more than one plant sensing light senses twice per cycle: once to cast,
//! once more, every plant having cast, to read. Sensing is idempotent (the markers taken
//! by the first pass are not there for the second), so this changes the time, not the
//! plant.
//!
//! Parametric plants are grown as by [`crate::grow`]: they neither cast nor sense
//! shade. Space colonization markers are not shared: each plant scatters its own in its
//! own envelope, the envelope of the surroundings or, when there is none, its default
//! envelope ([`crate::default_envelope`]) moved to its position.

use arbrume_core::graph::PlantGraph;
use arbrume_core::species::{GrowthMode, Sensing, Species};
use glam::Vec3;
use rayon::prelude::*;

use crate::competition::{Competition, EngineOptions, default_envelope};
use crate::confine::Confined;
use crate::cycle::{grow_sensed, seed_plant};
use crate::parametric::TropismBlend;
use crate::plant::Plant;
use crate::shadow::{ShadowGrid, ShadowParams};
use crate::shedding::ExposureShedding;
use crate::stage::{Context, Environment, Stages};
use crate::surroundings::{Shape, Surroundings};
use crate::{GrowParams, ResourceFate, SelfOrganizingStages, finish, grow};

/// One plant of a stand grown by [`grow_together`].
#[derive(Debug, Clone, Copy)]
pub struct Standing<'a> {
    /// Its species.
    pub species: &'a Species,
    /// Its seed, age and scale.
    pub params: GrowParams,
    /// Where it stands, in the frame of the surroundings.
    pub position: Vec3,
}

/// A self-organizing plant being grown in a stand.
struct Run<'a> {
    cx: Context<'a>,
    stages: SelfOrganizingStages<'a>,
    plant: Plant,
    /// Its shadow coefficients, when it senses light.
    shadow: Option<ShadowParams>,
}

impl Run<'_> {
    /// Senses cycle `cycle`, with `grid` lent to the plant when it senses light.
    fn sense(&mut self, grid: &mut Option<ShadowGrid>, cycle: u16) {
        if let (Some(params), Some(shared)) = (&self.shadow, grid.as_mut()) {
            shared.set_coefficients(params);
            *self.stages.environment.shadow_mut() = grid.take();
        }
        self.stages.environment.sense(&mut self.plant, cycle);
        if self.shadow.is_some() {
            *grid = self.stages.environment.shadow_mut().take();
        }
    }
}

/// Grows several plants that shade one another. See the [module documentation](self).
///
/// A plant younger than the oldest starts later, so that all end together. Each graph
/// is returned in the frame of its own plant, standing at the origin, in the order of
/// `plants`. A stand of one plant at the origin gives the plant of [`crate::grow_in`].
#[must_use]
pub fn grow_together(plants: &[Standing], surroundings: &Surroundings) -> Vec<PlantGraph> {
    grow_together_with(plants, surroundings, EngineOptions::default())
}

/// Same as [`grow_together`] with explicit engine options, which change the time and
/// memory taken but never the plants.
#[must_use]
pub fn grow_together_with(
    plants: &[Standing],
    surroundings: &Surroundings,
    options: EngineOptions,
) -> Vec<PlantGraph> {
    let order = canonical_order(plants);
    let sorted: Vec<&Standing> = order.iter().map(|&index| &plants[index]).collect();
    let own: Vec<Option<Surroundings>> = sorted
        .iter()
        .map(|standing| marker_surroundings(standing, surroundings))
        .collect();
    let mut runs: Vec<Option<Run>> = sorted
        .iter()
        .zip(&own)
        .map(|(standing, own)| {
            start(
                standing,
                own.as_ref().unwrap_or(surroundings),
                surroundings,
                options,
            )
        })
        .collect();
    let mut grid = shared_grid(&runs, options);
    let oldest = plants.iter().map(|p| p.params.age).max().unwrap_or(0);
    for step in 0..oldest {
        let cycles: Vec<Option<u16>> = runs
            .iter()
            .map(|run| {
                let age = run.as_ref()?.cx.age;
                step.checked_sub(oldest - age)
            })
            .collect();
        sense_all(&mut runs, &mut grid, &cycles);
        runs.par_iter_mut().zip(&cycles).for_each(|(run, cycle)| {
            if let (Some(run), Some(cycle)) = (run, cycle) {
                grow_sensed(&mut run.plant, &run.cx, &mut run.stages, *cycle);
            }
        });
    }
    let graphs: Vec<PlantGraph> = runs
        .into_par_iter()
        .zip(sorted.par_iter())
        .map(|(run, standing)| match run {
            Some(run) => finish(run.plant, &run.cx),
            None => grow(standing.species, &standing.params),
        })
        .collect();
    let mut slots: Vec<Option<PlantGraph>> =
        std::iter::repeat_with(|| None).take(plants.len()).collect();
    for (graph, index) in graphs.into_iter().zip(order) {
        slots[index] = Some(graph);
    }
    slots.into_iter().flatten().collect()
}

/// Indices of `plants` sorted by position, seed, age, scale and species name.
fn canonical_order(plants: &[Standing]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..plants.len()).collect();
    order.sort_by(|&a, &b| {
        let (a, b) = (&plants[a], &plants[b]);
        let position = |p: &Standing| p.position.to_array();
        let (pa, pb) = (position(a), position(b));
        (pa[0].total_cmp(&pb[0]))
            .then(pa[2].total_cmp(&pb[2]))
            .then(pa[1].total_cmp(&pb[1]))
            .then(a.params.seed.cmp(&b.params.seed))
            .then(a.params.age.cmp(&b.params.age))
            .then(a.params.scale.total_cmp(&b.params.scale))
            .then(a.species.name.cmp(&b.species.name))
    });
    order
}

/// Surroundings for the environment stage of a plant that scatters markers without an
/// envelope of the surroundings: its default envelope, moved to where it stands, and
/// the occluders of `shared`.
fn marker_surroundings(standing: &Standing, shared: &Surroundings) -> Option<Surroundings> {
    let model = &standing.species.growth.self_organization;
    let needed = standing.species.growth.mode == GrowthMode::SelfOrganizing
        && model.sensing != Sensing::Shadow
        && shared.envelope.is_none()
        && standing.position != Vec3::ZERO;
    needed.then(|| Surroundings {
        envelope: Some(moved(
            default_envelope(standing.species, standing.params.scale),
            standing.position,
        )),
        occluders: shared.occluders.clone(),
        water: None,
    })
}

/// `shape` moved by `by`; a voxel mask is kept as it is.
fn moved(shape: Shape, by: Vec3) -> Shape {
    match shape {
        Shape::Sphere { center, radius } => Shape::Sphere {
            center: center + by,
            radius,
        },
        Shape::Ellipsoid { center, radii } => Shape::Ellipsoid {
            center: center + by,
            radii,
        },
        Shape::Box { min, max } => Shape::Box {
            min: min + by,
            max: max + by,
        },
        Shape::Cylinder {
            base,
            radius,
            height,
        } => Shape::Cylinder {
            base: base + by,
            radius,
            height,
        },
        Shape::Cone {
            base,
            radius,
            height,
        } => Shape::Cone {
            base: base + by,
            radius,
            height,
        },
        Shape::Voxels(mask) => Shape::Voxels(mask),
    }
}

/// The run of a self-organizing plant before its first cycle, its environment stage
/// in `sensed` and its other stages in `surroundings`; `None` for a parametric plant.
fn start<'a>(
    standing: &Standing<'a>,
    sensed: &'a Surroundings,
    surroundings: &'a Surroundings,
    options: EngineOptions,
) -> Option<Run<'a>> {
    if standing.species.growth.mode != GrowthMode::SelfOrganizing {
        return None;
    }
    let cx = standing.params.context(standing.species);
    let mut stages = Stages {
        environment: Competition::new(&cx, sensed, options).at(standing.position),
        fate: ResourceFate::new(cx, surroundings).at(standing.position),
        orientation: Confined::new(TropismBlend::new(cx), &cx, surroundings).at(standing.position),
        shedding: ExposureShedding::new(&cx),
    };
    let shadow = stages
        .environment
        .shadow_mut()
        .take()
        .map(|grid| *grid.params());
    let plant = seed_plant(&cx, &mut stages.fate);
    Some(Run {
        cx,
        stages,
        plant,
        shadow,
    })
}

/// The grid shared by the plants that sense light, with the voxels of the finest of
/// them; `None` when no plant senses light.
fn shared_grid(runs: &[Option<Run>], options: EngineOptions) -> Option<ShadowGrid> {
    let finest = runs
        .iter()
        .filter_map(|run| run.as_ref()?.shadow)
        .min_by(|a, b| a.cell.total_cmp(&b.cell))?;
    Some(ShadowGrid::new(finest, options.storage))
}

/// Phases 1 and 2 of a cycle: the plants growing this cycle (`cycles`) cast, then
/// read. One pass does both when at most one of them senses light.
fn sense_all(runs: &mut [Option<Run>], grid: &mut Option<ShadowGrid>, cycles: &[Option<u16>]) {
    let growing = || {
        runs.iter()
            .zip(cycles)
            .filter(|(run, cycle)| run.is_some() && cycle.is_some())
    };
    let lit = growing()
        .filter(|(run, _)| run.as_ref().is_some_and(|run| run.shadow.is_some()))
        .count();
    for (run, cycle) in runs.iter_mut().zip(cycles) {
        if let (Some(run), Some(cycle)) = (run, cycle) {
            run.sense(grid, *cycle);
        }
    }
    if lit < 2 {
        return;
    }
    for (run, cycle) in runs.iter_mut().zip(cycles) {
        if let (Some(run), Some(cycle)) = (run, cycle)
            && run.shadow.is_some()
        {
            run.sense(grid, *cycle);
        }
    }
}

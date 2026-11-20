//! Growth engine of Arbrume: turns a species and a seed into a plant graph.
//!
//! ```
//! use arbrume_core::species::Species;
//! use arbrume_grow::{GrowParams, grow};
//!
//! let species = Species::default();
//! let graph = grow(&species, &GrowParams { seed: 7, age: 6, scale: 1.0 });
//! graph.check_invariants().unwrap();
//! assert!(graph.is_contiguous() && graph.node_count() > 0);
//! ```
//!
//! # The growth cycle
//!
//! A plant is grown by repeating one cycle with pluggable stages ([`stage`]), the
//! structure shared by Palubicki et al. (2009) and Longay et al. (2012):
//!
//! 1. [`Environment`] gives each bud an exposure `Q` and a preferred direction `V`;
//! 2. [`BudFate`] decides which buds form, which flush and how long their shoots are;
//! 3. [`ShootOrientation`] gives the direction of each new metamer;
//! 4. [`Shedding`] removes branches.
//!
//! The shoot builder between stages 2 and 3 places leaves and buds from the phyllotaxis
//! of the species. After the last cycle [`finish`] makes the chains contiguous, computes
//! the radii and applies the optional bending under weight.
//!
//! The species selects its mode (`growth.mode`). In the parametric mode the stages are
//! [`NoEnvironment`], [`GradientFate`], [`TropismBlend`] and [`NoShedding`]. In the
//! self-organizing mode buds compete for light and space: [`Competition`] (a
//! [`ShadowGrid`], [`Markers`], and the envelope and occluders of the
//! [`Surroundings`]), [`ResourceFate`] (extended Borchert-Honda or priority
//! allocation), [`TropismBlend`] kept out of obstacles by [`Confined`], and
//! [`ExposureShedding`]. [`grow`] grows a plant on
//! open ground, [`grow_in`] in given surroundings, [`grow_together`] a stand of
//! several plants that share their light ([`stand`]), and [`grow_with`] runs any other combination of stages.
//!
//! A grown plant can be shown as it was at an earlier age: see [`grow_timeline`] and
//! [`GrowthRecord`].
//!
//! # Determinism
//!
//! The result depends only on the species, the parameters, the seed and the version of
//! this crate, never on the number of threads. Every random draw is seeded by the key
//! of a meristem, derived from the seed and the position of the meristem in the plant
//! (see [`plant`]). The structure of a plant is built on one thread; the only pass
//! that may use the `rayon` pool is the sensing of the buds
//! ([`EngineOptions`]), which writes one independent result per bud and sums
//! in a fixed order. Growing several plants in parallel is safe, since nothing is
//! shared between two calls. The plants of a stand share one shadow grid; their casts
//! are summed in an order fixed by the entries, and they grow on the pool once sensed,
//! each on its own.

pub mod allocation;
mod bend;
pub mod competition;
pub mod confine;
mod cycle;
mod distance_field;
pub mod history;
pub mod markers;
mod math;
pub mod parametric;
pub mod plant;
mod radii;
pub mod shadow;
pub mod shedding;
mod shoot;
pub mod stage;
pub mod stand;
pub mod surroundings;

pub use allocation::{ResourceFate, borchert_honda, priority_weight};
pub use competition::{Competition, EngineOptions, default_envelope, shadow_params};
pub use confine::Confined;
pub use cycle::{run_cycle, run_cycles, seed_plant};
pub use history::{GrowthRecord, radii_at_cycle};
pub use markers::{Markers, Perception};
pub use parametric::{GradientFate, TropismBlend};
pub use plant::{Meristem, MeristemId, MeristemState, Plant, SiteKind};
pub use shadow::{GridStorage, ShadowGrid, ShadowParams};
pub use shedding::ExposureShedding;
pub use stage::{
    BudFate, BudSite, Context, Environment, Flush, HeadingInput, NoEnvironment, NoShedding,
    Shedding, ShootOrientation, Stages, Straight,
};
pub use stand::{Standing, grow_together, grow_together_with};
pub use surroundings::{Occluder, Shape, Surroundings, VoxelMask};

use arbrume_core::graph::PlantGraph;
use arbrume_core::species::{GrowthMode, Species};

/// Open ground, the surroundings of [`grow`].
static OPEN: Surroundings = Surroundings::none();

/// Parameters of one plant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GrowParams {
    /// Seed of every random draw.
    pub seed: u64,
    /// Number of growth cycles. Every growing axis extends once per cycle, so with
    /// rhythmic growth this is the age in growth units. 0 gives an empty graph.
    pub age: u16,
    /// Factor applied to every length and radius of the graph (stem positions, internode
    /// lengths, radii). Leaf site scales are not affected. Must be positive and finite.
    pub scale: f32,
}

impl Default for GrowParams {
    /// Seed 0, 10 cycles, scale 1.
    fn default() -> Self {
        Self {
            seed: 0,
            age: 10,
            scale: 1.0,
        }
    }
}

impl GrowParams {
    /// The run context of these parameters for `species`.
    #[must_use]
    pub const fn context<'a>(&self, species: &'a Species) -> Context<'a> {
        Context {
            species,
            seed: self.seed,
            age: self.age,
            scale: self.scale,
        }
    }
}

/// The stages of the self-organizing mode.
pub type SelfOrganizingStages<'a> =
    Stages<Competition<'a>, ResourceFate<'a>, Confined<'a, TropismBlend<'a>>, ExposureShedding>;

/// The stages of the self-organizing mode for the run `cx` in `surroundings`.
#[must_use]
pub fn self_organizing_stages<'a>(
    cx: &Context<'a>,
    surroundings: &'a Surroundings,
    options: EngineOptions,
) -> SelfOrganizingStages<'a> {
    Stages {
        environment: Competition::new(cx, surroundings, options),
        fate: ResourceFate::new(*cx, surroundings),
        orientation: Confined::new(TropismBlend::new(*cx), cx, surroundings),
        shedding: ExposureShedding::new(cx),
    }
}

/// Grows a plant of `species` on open ground, in the mode the species selects.
///
/// The species must be valid ([`Species::validate`]; species loaded from TOML are). The
/// returned graph satisfies `check_invariants`, is contiguous, has its radii set and
/// carries the leaf sites of the youngest growth.
#[must_use]
pub fn grow(species: &Species, params: &GrowParams) -> PlantGraph {
    grow_in(species, params, &OPEN)
}

/// Grows a plant of `species` in `surroundings`, in the mode the species selects. A
/// parametric species ignores the surroundings. The result is that of [`grow`].
#[must_use]
pub fn grow_in(species: &Species, params: &GrowParams, surroundings: &Surroundings) -> PlantGraph {
    grow_in_with(species, params, surroundings, EngineOptions::default())
}

/// Same as [`grow_in`] with explicit engine options, which change the time and memory
/// taken but never the plant.
#[must_use]
pub fn grow_in_with(
    species: &Species,
    params: &GrowParams,
    surroundings: &Surroundings,
    options: EngineOptions,
) -> PlantGraph {
    let cx = params.context(species);
    finish(grow_plant(&cx, surroundings, options), &cx)
}

/// Same as [`grow_in`], keeping the leaf sites of every metamer instead of those of the
/// youngest growth only, so that the plant can be shown at any earlier age: see
/// [`GrowthRecord`].
#[must_use]
pub fn grow_timeline(
    species: &Species,
    params: &GrowParams,
    surroundings: &Surroundings,
) -> GrowthRecord {
    let cx = params.context(species);
    let plant = grow_plant(&cx, surroundings, EngineOptions::default());
    let mut lasting = Vec::new();
    let (graph, shown) = finish_with(plant, &cx, Some(&mut lasting));
    GrowthRecord {
        graph,
        lasting,
        shown,
        age: params.age,
    }
}

/// Runs the cycles of the mode of the species.
fn grow_plant(cx: &Context, surroundings: &Surroundings, options: EngineOptions) -> Plant {
    match cx.species.growth.mode {
        GrowthMode::Parametric => {
            let mut stages = Stages {
                environment: NoEnvironment,
                fate: GradientFate::new(*cx),
                orientation: TropismBlend::new(*cx),
                shedding: NoShedding,
            };
            run_cycles(cx, &mut stages)
        }
        GrowthMode::SelfOrganizing => {
            run_cycles(cx, &mut self_organizing_stages(cx, surroundings, options))
        }
    }
}

/// Grows a plant with the given stages, then finishes it like [`grow`].
#[must_use]
pub fn grow_with<E, F, O, S>(cx: &Context, stages: &mut Stages<E, F, O, S>) -> PlantGraph
where
    E: Environment,
    F: BudFate,
    O: ShootOrientation,
    S: Shedding,
{
    finish(run_cycles(cx, stages), cx)
}

/// Turns a grown plant into its final graph: contiguous chains, the leaf sites that are
/// retained, radii from the radius model of the species, then bending under weight when
/// the species asks for it.
#[must_use]
pub fn finish(plant: Plant, cx: &Context) -> PlantGraph {
    finish_with(plant, cx, None).0
}

/// [`finish`]; with `lasting`, every leaf site is kept and described there (see
/// `retain_leaves`). Also returns the number of leaf sites the grown plant keeps, which
/// come first.
fn finish_with(
    mut plant: Plant,
    cx: &Context,
    lasting: Option<&mut Vec<u16>>,
) -> (PlantGraph, usize) {
    let remap = plant.graph.ensure_contiguous();
    let shown = shoot::retain_leaves(&mut plant, cx, remap.as_deref(), lasting);
    radii::assign_radii(&mut plant, cx.species, cx.scale);
    bend::bend_under_weight(&mut plant.graph, cx.species.growth.weight_bending);
    (plant.graph, shown)
}

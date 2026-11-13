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
//! [`grow`] runs the parametric mode: [`NoEnvironment`], [`GradientFate`],
//! [`TropismBlend`] and [`NoShedding`]. [`grow_with`] runs any other combination.
//!
//! # Determinism
//!
//! The result depends only on the species, the parameters, the seed and the version of
//! this crate. A plant is grown sequentially; every random draw is seeded by the key of
//! a meristem, derived from the seed and the position of the meristem in the plant (see
//! [`plant`]). Growing several plants in parallel is safe, since nothing is shared
//! between two calls.

mod bend;
mod cycle;
mod math;
pub mod parametric;
pub mod plant;
mod radii;
mod shoot;
pub mod stage;

pub use cycle::run_cycles;
pub use parametric::{GradientFate, TropismBlend};
pub use plant::{Meristem, MeristemId, MeristemState, Plant, SiteKind};
pub use stage::{
    BudFate, BudSite, Context, Environment, Flush, HeadingInput, NoEnvironment, NoShedding,
    Shedding, ShootOrientation, Stages, Straight,
};

use arbrume_core::graph::PlantGraph;
use arbrume_core::species::Species;

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

/// Grows a plant of `species` in the parametric mode.
///
/// The species must be valid ([`Species::validate`]; species loaded from TOML are). The
/// returned graph satisfies `check_invariants`, is contiguous, has its radii set and
/// carries the leaf sites of the youngest growth.
#[must_use]
pub fn grow(species: &Species, params: &GrowParams) -> PlantGraph {
    let cx = params.context(species);
    let mut stages = Stages {
        environment: NoEnvironment,
        fate: GradientFate::new(cx),
        orientation: TropismBlend::new(cx),
        shedding: NoShedding,
    };
    grow_with(&cx, &mut stages)
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
pub fn finish(mut plant: Plant, cx: &Context) -> PlantGraph {
    let remap = plant.graph.ensure_contiguous();
    shoot::retain_leaves(&mut plant, cx, remap.as_deref());
    radii::assign_radii(&mut plant, cx.species, cx.scale);
    bend::bend_under_weight(&mut plant.graph, cx.species.growth.weight_bending);
    plant.graph
}

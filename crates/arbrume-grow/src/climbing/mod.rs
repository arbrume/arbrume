//! Climbing plants: shoots that find, follow and hold supports.
//!
//! A species with a `[climbing]` table grows in the same cycle and on the same plant
//! graph as any other; only the orientation stage changes, and a last pass dresses the
//! grown plant:
//!
//! * the supports ([`Supports`]) are the blocking occluders of the [`Surroundings`]
//!   (analytic shapes or voxel masks of arbitrary meshes) and, when the species says
//!   so, the ground;
//! * [`Climb`] wraps the orientation stage of the growth mode: a held shoot follows the
//!   surface of its support at a small gap, a shoot near a support turns towards it,
//!   and a free shoot droops under its weight;
//! * [`dress`] moves thickened stems out of their support, turns the leaves of held
//!   shoots away from it, and adds rootlets or tendrils.
//!
//! In the self-organizing mode the shoots of a climber compete for light as those of a
//! tree do ([`crate::Competition`], [`crate::ResourceFate`],
//! [`crate::ExposureShedding`]): the shade a stem casts down its support sends the
//! resource to the buds that reach lit, free surface, which is what makes a climber
//! fill a wall. In the parametric mode the lengths come from the gradient curves and
//! only the orientation climbs.
//!
//! The rules are the local ones of Hädrich et al. (2017): surface adaptation, light,
//! anchoring to a surface within a distance. Their particle simulation (shape matching,
//! breaking, wind coupling) is not reproduced: a support holds a stem for good, and
//! hanging comes from the droop of the orientation stage, not from dynamics.

mod dress;
mod orient;
mod support;

pub use dress::dress;
pub use orient::Climb;
pub use support::{Contact, Supports};

use crate::competition::{Competition, EngineOptions};
use crate::cycle::run_cycles;
use crate::parametric::{GradientFate, TropismBlend};
use crate::plant::Plant;
use crate::stage::{Context, NoEnvironment, NoShedding, Stages};
use crate::surroundings::Surroundings;
use crate::{ExposureShedding, ResourceFate};
use arbrume_core::species::GrowthMode;

/// The stages of a climbing species in the self-organizing mode.
pub type ClimbingStages<'a> =
    Stages<Competition<'a>, ResourceFate<'a>, Climb<'a, TropismBlend<'a>>, ExposureShedding>;

/// The stages of a climbing species in the self-organizing mode for the run `cx` in
/// `surroundings`. The species must have a `[climbing]` table.
#[must_use]
pub fn climbing_stages<'a>(
    cx: &Context<'a>,
    surroundings: &'a Surroundings,
    options: EngineOptions,
) -> ClimbingStages<'a> {
    Stages {
        environment: Competition::new(cx, surroundings, options),
        fate: ResourceFate::new(*cx, surroundings),
        orientation: Climb::new(TropismBlend::new(*cx), cx, surroundings),
        shedding: ExposureShedding::new(cx),
    }
}

/// Runs the cycles of a climbing species in the mode it selects.
pub(crate) fn grow_climber<'a>(
    cx: &Context<'a>,
    surroundings: &'a Surroundings,
    options: EngineOptions,
) -> Plant {
    match cx.species.growth.mode {
        GrowthMode::Parametric => {
            let mut stages = Stages {
                environment: NoEnvironment,
                fate: GradientFate::new(*cx),
                orientation: Climb::new(TropismBlend::new(*cx), cx, surroundings),
                shedding: NoShedding,
            };
            run_cycles(cx, &mut stages)
        }
        GrowthMode::SelfOrganizing => {
            run_cycles(cx, &mut climbing_stages(cx, surroundings, options))
        }
    }
}

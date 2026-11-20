//! The growth cycle driver.

use crate::plant::Plant;
use crate::shoot::{grow_shoot, plant_seeds};
use crate::stage::{BudFate, Context, Environment, Shedding, ShootOrientation, Stages};

/// Runs `cx.age` growth cycles with the given stages and returns the plant.
///
/// Meristems, chains and buds are created in an order that depends only on the species
/// and the seed, and the first cycles of a long run are identical to a shorter run as
/// long as the stages do not look at `cx.age`.
///
/// Extending many chains per cycle leaves the graph non-contiguous; [`crate::finish`]
/// restores contiguity.
pub fn run_cycles<E, F, O, S>(cx: &Context, stages: &mut Stages<E, F, O, S>) -> Plant
where
    E: Environment,
    F: BudFate,
    O: ShootOrientation,
    S: Shedding,
{
    let mut plant = seed_plant(cx, &mut stages.fate);
    for cycle in 0..cx.age {
        run_cycle(&mut plant, cx, stages, cycle);
    }
    plant
}

/// A plant before its first cycle: the resting meristems of its stems, as admitted by
/// `fate`.
pub fn seed_plant<F: BudFate>(cx: &Context, fate: &mut F) -> Plant {
    let mut plant = Plant::new();
    plant_seeds(&mut plant, cx, fate);
    plant
}

/// Runs growth cycle `cycle` of `plant`, which must have gone through every earlier
/// cycle with the same stages.
///
/// The cycle senses the environment, asks the fate stage which meristems flush, builds
/// their shoots in increasing meristem id order, offers the meristems formed meanwhile
/// to the fate stage again (immediate branching), and ends with shedding.
pub fn run_cycle<E, F, O, S>(
    plant: &mut Plant,
    cx: &Context,
    stages: &mut Stages<E, F, O, S>,
    cycle: u16,
) where
    E: Environment,
    F: BudFate,
    O: ShootOrientation,
    S: Shedding,
{
    stages.environment.sense(plant, cycle);
    grow_sensed(plant, cx, stages, cycle);
}

/// The rest of growth cycle `cycle` once the environment has sensed `plant`: fate,
/// shoots and shedding (see [`run_cycle`]).
pub(crate) fn grow_sensed<E, F, O, S>(
    plant: &mut Plant,
    cx: &Context,
    stages: &mut Stages<E, F, O, S>,
    cycle: u16,
) where
    F: BudFate,
    O: ShootOrientation,
    S: Shedding,
{
    let mut flushes = Vec::new();
    let mut from = crate::plant::MeristemId(0);
    loop {
        flushes.clear();
        stages.fate.decide(plant, cycle, from, &mut flushes);
        from = plant.next_meristem();
        for flush in &flushes {
            let (fate, orientation) = (&mut stages.fate, &mut stages.orientation);
            grow_shoot(plant, cx, fate, orientation, flush, cycle);
        }
        if flushes.is_empty() || plant.next_meristem() == from {
            break;
        }
    }
    stages.shedding.shed(plant, cycle);
}

//! The growth cycle driver.

use crate::plant::Plant;
use crate::shoot::{grow_shoot, plant_seeds};
use crate::stage::{BudFate, Context, Environment, Shedding, ShootOrientation, Stages};

/// Runs `cx.age` growth cycles with the given stages and returns the plant.
///
/// Each cycle senses the environment, asks the fate stage which meristems flush, builds
/// their shoots in increasing meristem id order, offers the meristems formed meanwhile
/// to the fate stage again (immediate branching), and ends with shedding. Meristems,
/// chains and buds are therefore created in an order that depends only on the species
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
    let mut plant = Plant::new();
    plant_seeds(&mut plant, cx, &mut stages.fate);
    let mut flushes = Vec::new();
    for cycle in 0..cx.age {
        stages.environment.sense(&mut plant, cycle);
        let mut from = crate::plant::MeristemId(0);
        loop {
            flushes.clear();
            stages.fate.decide(&plant, cycle, from, &mut flushes);
            from = plant.next_meristem();
            for flush in &flushes {
                let (fate, orientation) = (&mut stages.fate, &mut stages.orientation);
                grow_shoot(&mut plant, cx, fate, orientation, flush, cycle);
            }
            if flushes.is_empty() || plant.next_meristem() == from {
                break;
            }
        }
        stages.shedding.shed(&mut plant, cycle);
    }
    plant
}

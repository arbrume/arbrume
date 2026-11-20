//! The primary roots of a root system, before its first cycle.

use crate::math::{UP, perpendicular, tilt};
use crate::plant::{Meristem, MeristemState, Plant, SiteKind, seed_key};
use crate::stage::{BudFate, BudSite, Context};
use arbrume_core::graph::{BudId, ChainId};
use arbrume_core::rng::{Rng, Stream};
use glam::Vec3;
use std::f32::consts::TAU;

/// Stream of the placement of the primary roots.
const PRIMARY: Stream = Stream::custom(41);

/// Creates the resting meristems of the `count` primary roots of a root system, in the
/// turned frame: primary `i` leaves the collar `collars[i % collars.len()]` (one per stem)
/// at an azimuth spread evenly around the vertical, the circle turned by a random
/// angle and each azimuth moved by up to a third of the spacing, and at the set-point of
/// order 0 at the base of the axis from the vertical of the turned frame, plus or minus
/// the `branching_angle_variation` of order 0. With `tap`, primary 0 leaves straight down.
pub(crate) fn seed_primaries<F: BudFate>(
    plant: &mut Plant,
    cx: &Context,
    fate: &mut F,
    collars: &[Vec3],
    (count, tap): (u8, bool),
) {
    let growth = &cx.species.growth;
    let turn = Rng::new(cx.seed, u32::MAX, PRIMARY).angle();
    let spacing = TAU / f32::from(count.max(1));
    for primary in 0..u32::from(count) {
        let key = seed_key(cx.seed, primary);
        let site = BudSite {
            id: plant.next_meristem(),
            key,
            kind: SiteKind::Seed,
            order: 0,
            parent: None,
            node_index: primary,
            slot: 0,
            position: 0.0,
            arc: 0.0,
            unit_position: 0.0,
            cycle: 0,
        };
        let Some(target) = fate.admit(plant, &site) else {
            continue;
        };
        let mut rng = Rng::new(key, 0, PRIMARY);
        let azimuth = turn + spacing * (primary as f32 + rng.signed() / 3.0);
        let outward = Vec3::new(azimuth.cos(), 0.0, azimuth.sin());
        let variation = growth.branching_angle_variation.eval(0, 0.0) * rng.signed();
        let angle = (growth.gravitropic_setpoint.eval(0, 0.0) + variation).clamp(0.0, 180.0);
        let angle = if tap && primary == 0 { 0.0 } else { angle };
        let heading = tilt(UP, outward, angle.to_radians());
        plant.meristems.push(Meristem {
            key,
            kind: SiteKind::Seed,
            parent: None,
            state: MeristemState::Resting,
            order: 0,
            born: 0,
            bud: BudId::NONE,
            chain: ChainId::NONE,
            tip: collars[primary as usize % collars.len()],
            heading,
            side: perpendicular(heading, outward),
            phase: rng.angle(),
            nodes: 0,
            shoots: 0,
            forks: 0,
            start: 0.0,
            length: 0.0,
            target,
            attach_position: 0.0,
        });
    }
}

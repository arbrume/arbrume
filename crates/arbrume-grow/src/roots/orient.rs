//! Direction of each new root internode.
//!
//! [`RootOrientation`] is the orientation stage of the root system, in the turned frame
//! ([`super::frame`]). The heading is the tropism blend of the shoots
//! ([`TropismBlend`]): `H' = alpha H + beta T + gamma E` with the curvature, gnarl and
//! wander of the species, `E` being the direction preferred by the soil. Two things are
//! particular to roots.
//!
//! * **Sinkers.** A lateral root becomes a sinker with the `sinker_probability` of the
//!   species, drawn once from the key of its meristem: its tropism then points straight
//!   down whatever its order. This is a per-root version of the switch between lateral
//!   and vertical tropism of Li et al. (2023, eq. 23), whose probability falls with the
//!   distance to the base; here the draw is made once per root.
//! * **Obstacles.** A root does not stop at an obstacle or at the ground: when the
//!   internode would end in the air or in an obstacle, it slides along the surface it
//!   meets (its heading less the part along the direction that leads out of the
//!   blocked place), then tries headings turned away from the blocked place by
//!   increasing angles; the shoot ends only when every one is blocked. Roots therefore
//!   wrap around stones. Li et al. (2023) state that their roots do not collide with
//!   obstacles; this is a rule of this crate.

use super::frame::SoilView;
use crate::math::{perpendicular, radial, tilt};
use crate::parametric::TropismBlend;
use crate::stage::{Context, HeadingInput, ShootOrientation};
use arbrume_core::rng::{Rng, Stream};
use glam::Vec3;
use std::f32::consts::FRAC_PI_4;

/// Stream of the sinker draw of a lateral root.
const SINKER: Stream = Stream::custom(40);
/// Azimuths tried around the free direction at each angle when a root is blocked.
const AROUND: u32 = 6;

/// Whether the root of meristem key `key` and order `order` is a sinker.
#[must_use]
pub(crate) fn is_sinker(key: u64, order: u8, probability: f32) -> bool {
    order > 0 && probability > 0.0 && Rng::new(key, 0, SINKER).chance(probability)
}

/// Orientation stage of the root system. See the [module documentation](self).
#[derive(Debug, Clone)]
pub(crate) struct RootOrientation<'a> {
    normal: TropismBlend<'a>,
    sinker: TropismBlend<'a>,
    view: &'a SoilView<'a>,
    probability: f32,
    /// Key of the meristem of the tap root, if any.
    tap: Option<u64>,
    scale: f32,
}

impl<'a> RootOrientation<'a> {
    /// Orientation of the run `cx` of the root species, sinkers and the tap root (whose
    /// meristem key is `tap`) reading the species of `sinkers` (the same run with a
    /// downward tropism), confined by `view`; `probability` is that of a sinker.
    pub(crate) const fn new(
        cx: Context<'a>,
        sinkers: Context<'a>,
        view: &'a SoilView<'a>,
        (probability, tap): (f32, Option<u64>),
    ) -> Self {
        Self {
            normal: TropismBlend::new(cx),
            sinker: TropismBlend::new(sinkers),
            view,
            probability,
            tap,
            scale: cx.scale,
        }
    }

    /// Whether an internode from `tip` along `heading` of `length` meters ends in the
    /// air or in an obstacle.
    fn blocked(&self, tip: Vec3, heading: Vec3, length: f32) -> bool {
        self.view.clearance(tip + heading * length) <= 0.0
    }

    /// A free heading close to `heading`, which is blocked, or zero when every one tried
    /// is blocked.
    fn deflect(&self, input: &HeadingInput, heading: Vec3, length: f32) -> Vec3 {
        let end = input.tip + heading * length;
        let free = self.view.free_direction(end, 0.5 * length);
        if free == Vec3::ZERO {
            return Vec3::ZERO;
        }
        let into = heading.dot(free).min(0.0);
        let slide = (heading - free * into).normalize_or_zero();
        let away = (slide + free * 0.5).normalize_or_zero();
        for candidate in [slide, away, input.heading] {
            if candidate != Vec3::ZERO && !self.blocked(input.tip, candidate, length) {
                return candidate;
            }
        }
        let side = perpendicular(free, input.side);
        for step in 1..=3 {
            let angle = FRAC_PI_4 * step as f32;
            for k in 0..AROUND {
                let azimuth = std::f32::consts::TAU * k as f32 / AROUND as f32;
                let around = radial(free, side, azimuth);
                let candidate = tilt(slide.lerp(free, 0.5).normalize_or(free), around, angle);
                if !self.blocked(input.tip, candidate, length) {
                    return candidate.normalize();
                }
            }
        }
        Vec3::ZERO
    }
}

impl ShootOrientation for RootOrientation<'_> {
    fn heading(&mut self, input: &HeadingInput) -> Vec3 {
        let tap = input.order == 0 && self.tap == Some(input.key);
        let heading = if tap || is_sinker(input.key, input.order, self.probability) {
            self.sinker.heading(input)
        } else {
            self.normal.heading(input)
        };
        let length = input.internode * self.scale;
        if !self.blocked(input.tip, heading, length) {
            return heading;
        }
        self.deflect(input, heading, length)
    }
}

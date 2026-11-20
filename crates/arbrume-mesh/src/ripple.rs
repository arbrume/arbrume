//! Twist and undulation of a leaf blade: ribbons that spiral, kelp blades with
//! ruffled edges.
//!
//! A blade is built station by station along its midrib (`crate::leaf`); at the
//! relative position `t` a point of the blade lies at `point + offset`, `point` on the
//! midrib and `offset` across it. [`Ripple::apply`] turns the offset and the normal
//! about the tangent of the midrib by `twist t` (`foliage.twist_deg`), then raises the
//! margin along the upper side of the twisted blade by
//! `undulation w sin(2 pi waves t + phase)`, `w` the distance of the point from the
//! midrib, with the phase a quarter turn apart on the two edges so that the margin
//! ruffles rather than the whole blade waving. Normals follow the twist only.
//!
//! Twist and undulation need stations along the length that the outline tolerance of a
//! narrow blade does not ask for: [`Ripple::segments`] gives the number of segments
//! that keep them within the tolerance. A blade without twist nor undulation is built
//! exactly as before.

use arbrume_core::species::Foliage;
use glam::{Quat, Vec3};
use std::f32::consts::{FRAC_PI_2, TAU};

/// Segments per wave of the margin.
const SEGMENTS_PER_WAVE: f32 = 4.0;

/// Twist and undulation of the blades of a species.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Ripple {
    twist: f32,
    undulation: f32,
    waves: f32,
}

impl Ripple {
    /// The ripple of the blades of `foliage`.
    pub(crate) fn new(foliage: &Foliage) -> Self {
        Self {
            twist: foliage.twist_deg.to_radians(),
            undulation: foliage.undulation,
            waves: foliage.undulation_waves,
        }
    }

    /// Whether the blade is neither twisted nor undulating.
    pub(crate) fn is_flat(&self) -> bool {
        self.twist == 0.0 && (self.undulation == 0.0 || self.waves == 0.0)
    }

    /// Segments needed along a blade of `length` and `width` (both in meters, `bend`
    /// in radians) for the twist, the waves and the bend to stay within `tolerance`.
    pub(crate) fn segments(&self, length: f32, width: f32, bend: f32, tolerance: f32) -> u32 {
        if self.is_flat() {
            return 0;
        }
        let tolerance = tolerance.max(f32::MIN_POSITIVE);
        let bent = (length * bend.abs() / (8.0 * tolerance)).sqrt();
        let twisted = self.twist.abs() * (width / (16.0 * tolerance)).sqrt();
        let wavy = if self.undulation * width > tolerance {
            SEGMENTS_PER_WAVE * self.waves
        } else {
            0.0
        };
        bent.max(twisted).max(wavy).ceil() as u32
    }

    /// Position and normal of the blade point `position` of normal `normal`, at the
    /// relative position `t` across the midrib point `point` of upward normal `up`.
    /// `edge` is 1 on the left margin, -1 on the right and 0 on the midrib. A flat
    /// blade gets `position` and `normal` back unchanged.
    pub(crate) fn apply(
        &self,
        t: f32,
        (point, up): (Vec3, Vec3),
        (position, normal): (Vec3, Vec3),
        edge: f32,
    ) -> (Vec3, Vec3) {
        if self.is_flat() {
            return (position, normal);
        }
        let tangent = Vec3::Y.cross(up);
        let turn = Quat::from_axis_angle(tangent, self.twist * t);
        let mut offset = turn * (position - point);
        if edge != 0.0 && self.undulation != 0.0 {
            let phase = if edge > 0.0 { 0.0 } else { FRAC_PI_2 };
            let wave = (TAU * self.waves * t + phase).sin();
            offset += turn * up * (self.undulation * offset.length() * wave);
        }
        (point + offset, (turn * normal).normalize())
    }
}

#[cfg(test)]
mod tests {
    use super::Ripple;
    use arbrume_core::species::Foliage;
    use glam::Vec3;

    #[test]
    fn a_flat_blade_is_unchanged_and_a_twist_turns_the_margin() {
        let flat = Ripple::new(&Foliage::default());
        assert!(flat.is_flat());
        let point = (Vec3::new(0.5, 0.0, 0.0), Vec3::Z);
        let side = (point.0 + Vec3::Y * 0.1, Vec3::Z);
        assert_eq!(flat.apply(0.5, point, side, 1.0), side);
        let twisted = Ripple::new(&Foliage {
            twist_deg: 180.0,
            ..Foliage::default()
        });
        let (position, normal) = twisted.apply(0.5, point, side, 1.0);
        assert!(
            (position - Vec3::new(0.5, 0.0, 0.1)).length() < 1e-5,
            "{position}"
        );
        assert!((normal - Vec3::new(0.0, -1.0, 0.0)).length() < 1e-5);
        assert!(twisted.segments(1.0, 0.01, 0.0, 0.002) >= 2);
    }
}

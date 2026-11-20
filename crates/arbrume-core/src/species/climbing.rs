//! Climbing layer: how the shoots of a climbing plant find, follow and hold a support.
//!
//! The table `[climbing]` is optional. A species without it grows as before; a species
//! with it grows in the same cycle with a climbing orientation stage (`arbrume-grow`,
//! module `climbing`): shoots near a support follow its surface at a small gap, turn
//! towards a support within reach of their search, and droop under their weight where
//! nothing holds them. The surface following and the turn towards light are the local
//! rules of Hädrich et al. (2017, eqs. 6 and 7); the habit says what a shoot can hold.
//!
//! Lengths are in meters and are not multiplied by the scale of the plant: they describe
//! the attachment organs, whose size does not follow the size of the plant. All
//! defaults are starting points for tuning, not measurements.

use super::{SpeciesError, check_range};
use serde::{Deserialize, Serialize};

/// How a climbing plant holds its support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Attachment {
    /// Adhesive roots or pads along the stem: any surface within reach holds, flat walls
    /// included (ivy, climbing hydrangea, Virginia creeper).
    #[default]
    Adhesive,
    /// Tendrils that coil around thin supports: only supports thinner than
    /// `max_support_thickness` hold (grapevine, passion flower, pea).
    Tendril,
    /// The stem winds around thin supports in a helix (hop, bean, wisteria): only
    /// supports thinner than `max_support_thickness` hold.
    Twining,
}

/// Climbing parameters of a species. See the [module documentation](self).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Climbing {
    /// What holds the plant to its support.
    pub attachment: Attachment,
    /// Distance from the surface of a support within which a shoot is held and follows
    /// the surface.
    pub reach: f32,
    /// Distance within which a free shoot turns towards the nearest support.
    pub search_distance: f32,
    /// Weight in `[0, 1]` of the direction towards the support for a searching shoot.
    pub seek_weight: f32,
    /// Gap between the bark of an attached stem and the surface of its support.
    pub gap: f32,
    /// Bend of a free shoot towards the ground, in degrees per meter of shoot grown since
    /// it was last held: the longer the free length, the closer to hanging straight down.
    pub droop: f32,
    /// The ground the plant stands on is a support: shoots that reach it creep along it.
    pub ground: bool,
    /// Supports thicker than this do not hold tendrils nor twining stems. Not read for
    /// the adhesive habit.
    pub max_support_thickness: f32,
    /// Angle in degrees between a twining stem and the upward direction on its support;
    /// the sign sets the hand of the helix (positive turns anticlockwise seen from
    /// above). Read by the twining habit only.
    pub twine_angle_deg: f32,
    /// Probability that a node of a held shoot bears a tendril. Read by the tendril
    /// habit only.
    pub tendrils: f32,
    /// Length of a tendril.
    pub tendril_length: f32,
    /// Number of turns of the coil of a tendril.
    pub tendril_turns: f32,
    /// Probability that a node of a held shoot bears a tuft of adhesive rootlets. Read by
    /// the adhesive habit only.
    pub rootlets: f32,
    /// Length of an adhesive rootlet.
    pub rootlet_length: f32,
    /// Weight in `[0, 1]` with which the leaves of a held shoot turn their upper side
    /// away from the support and towards the light; 0 leaves them as the phyllotaxis
    /// places them.
    pub leaf_facing: f32,
}

impl Default for Climbing {
    fn default() -> Self {
        Self {
            attachment: Attachment::Adhesive,
            reach: 0.06,
            search_distance: 0.5,
            seek_weight: 0.4,
            gap: 0.004,
            droop: 90.0,
            ground: true,
            max_support_thickness: 0.3,
            twine_angle_deg: 55.0,
            tendrils: 0.5,
            tendril_length: 0.12,
            tendril_turns: 4.0,
            rootlets: 0.0,
            rootlet_length: 0.012,
            leaf_facing: 0.8,
        }
    }
}

impl Climbing {
    /// Whether the habit holds only supports thinner than `max_support_thickness`.
    #[must_use]
    pub const fn needs_thin_support(&self) -> bool {
        !matches!(self.attachment, Attachment::Adhesive)
    }

    pub(super) fn validate(&self) -> Result<(), SpeciesError> {
        let max = f32::MAX;
        check_range("climbing.reach", self.reach, 0.0, max)?;
        check_range(
            "climbing.search_distance",
            self.search_distance,
            self.reach,
            max,
        )?;
        check_range("climbing.seek_weight", self.seek_weight, 0.0, 1.0)?;
        check_range("climbing.gap", self.gap, 0.0, self.reach.max(0.0))?;
        check_range("climbing.droop", self.droop, 0.0, max)?;
        check_range(
            "climbing.max_support_thickness",
            self.max_support_thickness,
            f32::MIN_POSITIVE,
            max,
        )?;
        check_range(
            "climbing.twine_angle_deg",
            self.twine_angle_deg,
            -85.0,
            85.0,
        )?;
        check_range("climbing.tendrils", self.tendrils, 0.0, 1.0)?;
        check_range("climbing.tendril_length", self.tendril_length, 0.0, max)?;
        check_range("climbing.tendril_turns", self.tendril_turns, 0.0, 50.0)?;
        check_range("climbing.rootlets", self.rootlets, 0.0, 1.0)?;
        check_range("climbing.rootlet_length", self.rootlet_length, 0.0, max)?;
        check_range("climbing.leaf_facing", self.leaf_facing, 0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::{Attachment, Climbing};

    #[test]
    fn defaults_are_valid_and_domains_are_checked() {
        Climbing::default().validate().unwrap();
        let invalid = [
            Climbing {
                reach: -0.1,
                ..Default::default()
            },
            Climbing {
                search_distance: 0.01,
                ..Default::default()
            },
            Climbing {
                gap: 0.5,
                ..Default::default()
            },
            Climbing {
                seek_weight: 1.5,
                ..Default::default()
            },
            Climbing {
                twine_angle_deg: 90.0,
                ..Default::default()
            },
            Climbing {
                max_support_thickness: 0.0,
                ..Default::default()
            },
            Climbing {
                leaf_facing: f32::NAN,
                ..Default::default()
            },
        ];
        for climbing in invalid {
            assert!(climbing.validate().is_err(), "{climbing:?}");
        }
    }

    #[test]
    fn only_adhesive_climbers_hold_any_support() {
        let mut climbing = Climbing::default();
        assert!(!climbing.needs_thin_support());
        climbing.attachment = Attachment::Tendril;
        assert!(climbing.needs_thin_support());
        climbing.attachment = Attachment::Twining;
        assert!(climbing.needs_thin_support());
    }
}

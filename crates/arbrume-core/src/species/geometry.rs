//! Geometry layer: how thick the axes are.
//!
//! Two radius models are supported. [`RadiusModel::Pipe`] is the pipe model (Shinozaki
//! et al. 1964): the cross-sections of the children of a node add up,
//! `r^n = sum of r_child^n`, starting from `tip_radius` at the tips. It follows the
//! actual branching and is the model of the self-organizing mode.
//! [`RadiusModel::Tapered`] is the parametric model of Weber and Penn (1995): the base
//! radius of an axis is derived from its length, and `taper` shapes it along the axis.
//!
//! The layer also shapes the swept surface: the widening of the trunk base
//! ([`Geometry::flare`], with optional buttress lobes), the collar of a lateral axis
//! where it leaves its parent, and the bounds of the number of sides of a cross-section.
//!
//! All defaults are starting points for tuning, not measured values.

use super::{SpeciesError, check_curve, check_range};
use crate::curve::{Curve, OrderCurve};
use serde::{Deserialize, Serialize};

/// Fewest sides a cross-section can have and still enclose a volume.
pub const MIN_RADIAL_SEGMENTS: u32 = 3;

/// Upper limit of [`Geometry::radial_segments`].
pub const MAX_RADIAL_SEGMENTS: u32 = 64;

/// Upper limit of [`Geometry::buttress_lobes`]: a cross-section of
/// [`MAX_RADIAL_SEGMENTS`] sides gives each lobe four sides.
pub const MAX_BUTTRESS_LOBES: u32 = MAX_RADIAL_SEGMENTS / 4;

/// How internode radii are computed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RadiusModel {
    /// Pipe model: radii accumulated from the tips with `pipe_exponent`.
    #[default]
    Pipe,
    /// Radii from axis length and the `taper` curve.
    Tapered,
}

/// Radius parameters of a species.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Geometry {
    /// Which radius model applies.
    pub radius_model: RadiusModel,
    /// Exponent `n` of the pipe model. 2 conserves cross-section area (Leonardo's rule);
    /// published tree models use 2 to 3.
    pub pipe_exponent: f32,
    /// Radius in meters of an internode without children (pipe model).
    pub tip_radius: f32,
    /// Trunk base radius divided by trunk length (tapered model; `Ratio` of Weber and
    /// Penn).
    pub radius_ratio: f32,
    /// Exponent of the child radius rule of the tapered model: `r_child = r_parent *
    /// (length_child / length_parent)^child_radius_exponent` (`RatioPower` of Weber and
    /// Penn).
    pub child_radius_exponent: f32,
    /// Widening of the trunk near the ground, 0 for none. See
    /// [`Geometry::flare_factor`].
    pub flare: f32,
    /// Number of buttress lobes around the flared base of the trunk, 0 for a round base.
    /// Lobes modulate only the widening due to `flare`, so they fade out with it
    /// (`Lobes` of Weber and Penn, restricted to the flare).
    pub buttress_lobes: u32,
    /// Depth of the buttress lobes in `[0, 1]`, as a fraction of the flare widening: the
    /// widening varies around the trunk between `1 - depth` and `1 + depth` times its
    /// mean.
    pub buttress_depth: f32,
    /// Widening of a lateral axis where it emerges from its parent, as a fraction of its
    /// own radius; 0 for no collar. The mesher never widens a collar beyond the radius of
    /// the parent.
    pub collar_flare: f32,
    /// Length over which the collar narrows back to the radius of the axis, in radii of
    /// the axis, measured from the surface of the parent.
    pub collar_length: f32,
    /// Smallest and largest number of sides of a branch cross-section. Between the two,
    /// the mesher picks the count from the radius and its tolerance.
    pub radial_segments: [u32; 2],
    /// Radius along an axis as a fraction of its base radius, in `[0, 1]`, along the
    /// axis (tapered model).
    pub taper: OrderCurve,
}

impl Default for Geometry {
    fn default() -> Self {
        let cone = Curve::linear(&[[0.0, 1.0], [1.0, 0.1]]).unwrap_or_default();
        Self {
            radius_model: RadiusModel::default(),
            pipe_exponent: 2.5,
            tip_radius: 0.003,
            radius_ratio: 0.02,
            child_radius_exponent: 1.2,
            flare: 0.5,
            buttress_lobes: 0,
            buttress_depth: 0.3,
            collar_flare: 0.35,
            collar_length: 3.0,
            radial_segments: [3, 16],
            taper: OrderCurve::uniform(cone),
        }
    }
}

impl Geometry {
    /// Factor by which the trunk radius is multiplied at relative height `z` along the
    /// trunk (0 at the ground), following Weber and Penn (1995):
    /// `1 + flare * (100^y - 1) / 100` with `y = max(0, 1 - 8 z)`. It is `1 + 0.99 *
    /// flare` at the ground and 1 above one eighth of the trunk.
    #[must_use]
    pub fn flare_factor(&self, z: f32) -> f32 {
        let y = (1.0 - 8.0 * z).max(0.0);
        1.0 + self.flare * (100.0_f32.powf(y) - 1.0) / 100.0
    }

    pub(super) fn validate(&self) -> Result<(), SpeciesError> {
        let (tiny, max) = (f32::MIN_POSITIVE, f32::MAX);
        check_range("geometry.pipe_exponent", self.pipe_exponent, tiny, max)?;
        check_range("geometry.tip_radius", self.tip_radius, tiny, max)?;
        check_range("geometry.radius_ratio", self.radius_ratio, tiny, max)?;
        check_range(
            "geometry.child_radius_exponent",
            self.child_radius_exponent,
            0.0,
            max,
        )?;
        check_curve("geometry.taper", &self.taper, 0.0, 1.0)?;
        check_range("geometry.flare", self.flare, 0.0, max)?;
        check_range(
            "geometry.buttress_lobes",
            self.buttress_lobes as f32,
            0.0,
            MAX_BUTTRESS_LOBES as f32,
        )?;
        check_range("geometry.buttress_depth", self.buttress_depth, 0.0, 1.0)?;
        check_range("geometry.collar_flare", self.collar_flare, 0.0, max)?;
        check_range("geometry.collar_length", self.collar_length, tiny, max)?;
        let [fewest, most] = self.radial_segments;
        check_range(
            "geometry.radial_segments",
            fewest as f32,
            MIN_RADIAL_SEGMENTS as f32,
            most as f32,
        )?;
        check_range(
            "geometry.radial_segments",
            most as f32,
            fewest as f32,
            MAX_RADIAL_SEGMENTS as f32,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::Geometry;

    #[test]
    fn surface_parameters_are_validated() {
        let invalid = [
            Geometry {
                radial_segments: [2, 16],
                ..Default::default()
            },
            Geometry {
                radial_segments: [8, 4],
                ..Default::default()
            },
            Geometry {
                radial_segments: [3, 65],
                ..Default::default()
            },
            Geometry {
                buttress_lobes: 17,
                ..Default::default()
            },
            Geometry {
                buttress_depth: 1.5,
                ..Default::default()
            },
            Geometry {
                collar_flare: -0.1,
                ..Default::default()
            },
            Geometry {
                collar_length: 0.0,
                ..Default::default()
            },
        ];
        Geometry::default().validate().unwrap();
        for geometry in invalid {
            assert!(geometry.validate().is_err(), "{geometry:?}");
        }
    }

    #[test]
    fn flare_widens_only_the_base() {
        let geometry = Geometry {
            flare: 1.0,
            ..Default::default()
        };
        assert!((geometry.flare_factor(0.0) - 1.99).abs() < 1e-5);
        assert!(geometry.flare_factor(0.05) > 1.0 && geometry.flare_factor(0.05) < 1.99);
        assert!((geometry.flare_factor(0.125) - 1.0).abs() < 1e-6);
        assert!((geometry.flare_factor(0.9) - 1.0).abs() < 1e-6);
    }
}

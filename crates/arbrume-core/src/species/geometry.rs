//! Geometry layer: how thick the axes are.
//!
//! Two radius models are supported. [`RadiusModel::Pipe`] is the pipe model (Shinozaki
//! et al. 1964): the cross-sections of the children of a node add up,
//! `r^n = sum of r_child^n`, starting from `tip_radius` at the tips. It follows the
//! actual branching and is the model of the self-organizing mode.
//! [`RadiusModel::Tapered`] is the parametric model of Weber and Penn (1995): the base
//! radius of an axis is derived from its length, and `taper` shapes it along the axis.
//!
//! All defaults are starting points for tuning, not measured values.

use super::{SpeciesError, check_curve, check_range};
use crate::curve::{Curve, OrderCurve};
use serde::{Deserialize, Serialize};

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
        check_range("geometry.flare", self.flare, 0.0, max)
    }
}

#[cfg(test)]
mod tests {
    use super::Geometry;

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

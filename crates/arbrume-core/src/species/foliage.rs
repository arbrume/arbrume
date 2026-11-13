//! Foliage layer: the leaves the mesher instances at organ sites.
//!
//! Where leaves sit around an axis is the phyllotaxis of the architecture layer; this
//! layer describes the leaf itself. All defaults are starting points for tuning, not
//! measured values.

use super::{SpeciesError, check_color, check_range};
use crate::curve::Curve;
use serde::{Deserialize, Serialize};

/// Leaf parameters of a species.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Foliage {
    /// Whether the plant bears leaves at all.
    pub enabled: bool,
    /// Length of a full-size leaf blade, in meters.
    pub length: f32,
    /// Blade width divided by blade length.
    pub width_ratio: f32,
    /// Leaf size factor over the position of the leaf along its axis (0 base, 1 tip).
    pub size_along_axis: Curve,
    /// Angle in degrees between the leaf and the axis bearing it, 0 pointing along the
    /// axis towards its tip and 90 perpendicular to it.
    pub inclination_deg: f32,
    /// Number of growth cycles a metamer keeps its leaves: 1 for a plant that carries
    /// leaves only on the shoots of the current cycle, more for evergreens.
    pub retention_cycles: u16,
    /// Linear RGB base colour.
    pub color: [f32; 3],
    /// Path of the leaf texture, relative to the species file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub texture: Option<String>,
}

impl Default for Foliage {
    fn default() -> Self {
        Self {
            enabled: true,
            length: 0.08,
            width_ratio: 0.5,
            size_along_axis: Curve::constant(1.0),
            inclination_deg: 45.0,
            retention_cycles: 1,
            color: [0.12, 0.3, 0.06],
            texture: None,
        }
    }
}

impl Foliage {
    pub(super) fn validate(&self) -> Result<(), SpeciesError> {
        let (tiny, max) = (f32::MIN_POSITIVE, f32::MAX);
        check_range("foliage.length", self.length, tiny, max)?;
        check_range("foliage.width_ratio", self.width_ratio, tiny, max)?;
        let (size_lo, size_hi) = self.size_along_axis.bounds();
        check_range("foliage.size_along_axis", size_lo, 0.0, max)?;
        check_range("foliage.size_along_axis", size_hi, 0.0, max)?;
        check_range("foliage.inclination_deg", self.inclination_deg, 0.0, 180.0)?;
        check_color("foliage.color", self.color)
    }
}

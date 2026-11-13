//! Bark layer: the surface of trunk and branches.
//!
//! All defaults are starting points for tuning, not measured values.

use super::{SpeciesError, check_color, check_range};
use serde::{Deserialize, Serialize};

/// Bark parameters of a species.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Bark {
    /// Linear RGB base colour.
    pub color: [f32; 3],
    /// Surface roughness in `[0, 1]`.
    pub roughness: f32,
    /// Size in meters of one repeat of the bark texture, as `[around, along]` the axis.
    pub texture_size: [f32; 2],
    /// Path of the bark texture, relative to the species file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub texture: Option<String>,
}

impl Default for Bark {
    fn default() -> Self {
        Self {
            color: [0.2, 0.14, 0.09],
            roughness: 0.9,
            texture_size: [0.5, 0.5],
            texture: None,
        }
    }
}

impl Bark {
    pub(super) fn validate(&self) -> Result<(), SpeciesError> {
        check_color("bark.color", self.color)?;
        check_range("bark.roughness", self.roughness, 0.0, 1.0)?;
        let [around, along] = self.texture_size;
        check_range("bark.texture_size", around, f32::MIN_POSITIVE, f32::MAX)?;
        check_range("bark.texture_size", along, f32::MIN_POSITIVE, f32::MAX)
    }
}

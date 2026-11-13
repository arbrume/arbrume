//! Bark layer: the surface of trunk and branches.
//!
//! All defaults are starting points for tuning, not measured values.

use super::{SpeciesError, check_color, check_range};
use serde::{Deserialize, Serialize};

/// Bark parameters of a species.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Bark {
    /// Linear RGB base colour of old bark.
    pub color: [f32; 3],
    /// Linear RGB colour of the bark of thin, young axes, when it differs: the bark
    /// blends from this colour to `color` as the radius grows to `young_radius`. The
    /// blend is written as vertex colours.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub young_color: Option<[f32; 3]>,
    /// Radius in meters from which the bark has its old colour.
    pub young_radius: f32,
    /// Variation of brightness from one axis to the next, in `[0, 1]`: an axis is
    /// darkened by up to this fraction. Written as vertex colours.
    pub color_variation: f32,
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
            young_color: None,
            young_radius: 0.03,
            color_variation: 0.0,
            roughness: 0.9,
            texture_size: [0.5, 0.5],
            texture: None,
        }
    }
}

impl Bark {
    /// `true` when the bark part carries vertex colours.
    #[must_use]
    pub fn has_vertex_colors(&self) -> bool {
        self.young_color.is_some() || self.color_variation > 0.0
    }

    /// Base colour of the bark material: per channel, the larger of `color` and
    /// `young_color`, so that vertex colours of at most 1 reach both.
    #[must_use]
    pub fn material_color(&self) -> [f32; 3] {
        let young = self.young_color.unwrap_or(self.color);
        std::array::from_fn(|channel| self.color[channel].max(young[channel]))
    }

    /// Colour of the bark of an axis of radius `radius`, in meters.
    #[must_use]
    pub fn color_at(&self, radius: f32) -> [f32; 3] {
        let Some(young) = self.young_color else {
            return self.color;
        };
        let t = (radius / self.young_radius).clamp(0.0, 1.0);
        let t = t * t * (3.0 - 2.0 * t);
        std::array::from_fn(|channel| young[channel] + (self.color[channel] - young[channel]) * t)
    }

    pub(super) fn validate(&self) -> Result<(), SpeciesError> {
        check_color("bark.color", self.color)?;
        if let Some(young) = self.young_color {
            check_color("bark.young_color", young)?;
        }
        let radius = self.young_radius;
        check_range("bark.young_radius", radius, f32::MIN_POSITIVE, f32::MAX)?;
        check_range("bark.color_variation", self.color_variation, 0.0, 1.0)?;
        check_range("bark.roughness", self.roughness, 0.0, 1.0)?;
        let [around, along] = self.texture_size;
        check_range("bark.texture_size", around, f32::MIN_POSITIVE, f32::MAX)?;
        check_range("bark.texture_size", along, f32::MIN_POSITIVE, f32::MAX)
    }
}

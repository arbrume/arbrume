//! Bark layer: the surface of trunk and branches.
//!
//! Besides its colours, a species describes the relief of its bark with a
//! [`BarkPattern`], from which `arbrume-texture` synthesizes tileable albedo, normal and
//! roughness maps (see `docs/textures.md`). All defaults are starting points for tuning,
//! not measured values.

use super::{SpeciesError, check_color, check_range};
use serde::{Deserialize, Serialize};

/// Smallest texture resolution of a species, in texels.
pub const MIN_TEXTURE_RESOLUTION: u32 = 16;
/// Largest texture resolution of a species, in texels.
pub const MAX_TEXTURE_RESOLUTION: u32 = 4096;

/// Fails unless `value` is a power of two within the texture resolutions.
pub(super) fn check_resolution(field: &'static str, value: u32) -> Result<(), SpeciesError> {
    let range = MIN_TEXTURE_RESOLUTION..=MAX_TEXTURE_RESOLUTION;
    if value.is_power_of_two() && range.contains(&value) {
        return Ok(());
    }
    Err(SpeciesError::Invalid {
        field,
        reason: format!(
            "{value} is not a power of two in [{MIN_TEXTURE_RESOLUTION}, {MAX_TEXTURE_RESOLUTION}]"
        ),
    })
}

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
    /// Path of an image (PNG or JPEG) used as the bark albedo instead of the synthesized
    /// one, covering one repeat of `texture_size`. A relative path is resolved against
    /// `Species::asset_dir` (see `docs/packs.md`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub texture: Option<String>,
    /// Path of a tangent-space normal map that goes with `texture`; a flat normal when
    /// absent. Only read with `texture`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub normal_texture: Option<String>,
    /// Width and height in texels of the synthesized bark texture, which covers one
    /// repeat of `texture_size`: a power of two from [`MIN_TEXTURE_RESOLUTION`] to
    /// [`MAX_TEXTURE_RESOLUTION`].
    pub texture_resolution: u32,
    /// Pattern of the synthesized bark texture.
    pub pattern: BarkPattern,
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
            normal_texture: None,
            texture_resolution: 512,
            pattern: BarkPattern::default(),
        }
    }
}

/// Family of bark relief synthesized for a species.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BarkKind {
    /// Smooth bark with lenticels, short marks stretched around the axis (birch,
    /// cherry, beech).
    Smooth,
    /// Long ridges along the axis separated by an interlaced network of furrows (oak,
    /// ash, poplar, old maple).
    #[default]
    Furrowed,
    /// Plates or scales separated by cracks (pine, old pear, some spruces).
    Plated,
    /// Thin fibres along the axis that part and merge (cedar, cypress, redwood).
    Fibrous,
    /// Patches of outer bark shed to reveal younger layers of other colours (plane,
    /// eucalyptus, paperbark birch).
    Peeling,
}

/// Relief and colour pattern of the synthesized bark texture.
///
/// Sizes are physical, in meters on the bark surface; the synthesizer rounds them so
/// that a whole number of features fits in one repeat of `bark.texture_size`, which is
/// what makes the texture tile.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BarkPattern {
    /// Family of the pattern.
    pub kind: BarkKind,
    /// Typical spacing of the features around the axis, in meters: furrows, plates,
    /// fibres, lenticels or patches.
    pub feature_size: f32,
    /// Length of a feature along the axis divided by its width, from 0.1 to 100: above
    /// 1 features are stretched along the axis, below 1 around it.
    pub stretch: f32,
    /// Depth of the relief relative to `feature_size`, in `[0, 1]`; it scales the
    /// normal map.
    pub relief: f32,
    /// Fraction of the surface taken by the secondary features, in `[0, 1]`: width of
    /// furrows and cracks, density of lenticels, area of shed patches.
    pub coverage: f32,
    /// How irregular the features are, in `[0, 1]`: amount of domain warping and of
    /// jitter of their size.
    pub irregularity: f32,
    /// Variation of the colour from one plate, patch or ridge to the next, and of the
    /// low-frequency mottling, in `[0, 1]`.
    pub contrast: f32,
    /// Linear RGB colour of the secondary features (furrow bottoms, cracks, lenticels,
    /// younger layers under shed patches); by default `bark.color` darkened to 35 %.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<[f32; 3]>,
    /// Variation of the roughness over the texture, in `[0, 1]`: furrows and cracks
    /// are rougher than ridges and plates.
    pub roughness_variation: f32,
}

impl Default for BarkPattern {
    fn default() -> Self {
        Self {
            kind: BarkKind::default(),
            feature_size: 0.04,
            stretch: 3.0,
            relief: 0.5,
            coverage: 0.35,
            irregularity: 0.5,
            contrast: 0.3,
            color: None,
            roughness_variation: 0.3,
        }
    }
}

impl BarkPattern {
    /// Colour of the secondary features of a bark of main colour `color`.
    #[must_use]
    pub fn secondary_color(&self, color: [f32; 3]) -> [f32; 3] {
        self.color.unwrap_or(color.map(|channel| 0.35 * channel))
    }

    fn validate(&self) -> Result<(), SpeciesError> {
        let size = self.feature_size;
        check_range("bark.pattern.feature_size", size, 1e-4, f32::MAX)?;
        check_range("bark.pattern.stretch", self.stretch, 0.1, 100.0)?;
        check_range("bark.pattern.relief", self.relief, 0.0, 1.0)?;
        check_range("bark.pattern.coverage", self.coverage, 0.0, 1.0)?;
        let irregularity = self.irregularity;
        check_range("bark.pattern.irregularity", irregularity, 0.0, 1.0)?;
        check_range("bark.pattern.contrast", self.contrast, 0.0, 1.0)?;
        if let Some(color) = self.color {
            check_color("bark.pattern.color", color)?;
        }
        let variation = self.roughness_variation;
        check_range("bark.pattern.roughness_variation", variation, 0.0, 1.0)
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
        check_range("bark.texture_size", along, f32::MIN_POSITIVE, f32::MAX)?;
        check_resolution("bark.texture_resolution", self.texture_resolution)?;
        let normal = self.normal_texture.as_deref();
        super::check_texture_paths("bark", self.texture.as_deref(), normal)?;
        self.pattern.validate()
    }
}

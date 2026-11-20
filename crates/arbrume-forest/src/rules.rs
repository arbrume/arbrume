//! What a species asks of the ground, and how its trees vary from one place to the next.

/// Placement rule of one species.
#[derive(Debug, Clone, PartialEq)]
pub struct SpeciesRule {
    /// Trees per hectare on ground that suits the species fully.
    pub density: f32,
    /// Smallest distance between the trunks of two trees of the species at its mean
    /// scale, in meters. Two species of the same layer keep the mean of their spacings.
    pub spacing: f32,
    /// Layer of the species: species of different layers do not compete for space (an
    /// understory of shrubs under a canopy of trees, say).
    pub layer: u8,
    /// Range of the uniform scale of a tree.
    pub scale: [f32; 2],
    /// Slopes the species takes fully, in degrees.
    pub slope: [f32; 2],
    /// Width of the linear fade outside `slope`, in degrees (0 for a hard limit).
    pub slope_falloff: f32,
    /// Ground heights the species takes fully, in meters.
    pub altitude: [f32; 2],
    /// Width of the linear fade outside `altitude`, in meters.
    pub altitude_falloff: f32,
    /// Number of variants a tree picks from.
    pub variants: u16,
    /// Largest lean of a trunk from the vertical, in degrees.
    pub lean_deg: f32,
    /// Half-width of the random brightness factor of a tree's colour (0 for none).
    pub tint: f32,
    /// Radius of the trunk base at scale 1, in meters: a tree on a slope is lowered to
    /// the lowest ground under its base.
    pub trunk_radius: f32,
    /// Extra depth the base is sunk by, in meters.
    pub sink: f32,
    /// The species grows under water: it stands only where the ground is below the
    /// water level of the exclusions, instead of only above it.
    pub underwater: bool,
}

impl Default for SpeciesRule {
    /// 300 trees per hectare 4 m apart, scales 0.8 to 1.2, any slope and altitude, one
    /// variant, no lean or tint: starting values to edit, not measurements.
    fn default() -> Self {
        Self {
            density: 300.0,
            spacing: 4.0,
            layer: 0,
            scale: [0.8, 1.2],
            slope: [0.0, 90.0],
            slope_falloff: 0.0,
            altitude: [f32::NEG_INFINITY, f32::INFINITY],
            altitude_falloff: 0.0,
            variants: 1,
            lean_deg: 0.0,
            tint: 0.0,
            trunk_radius: 0.0,
            sink: 0.0,
            underwater: false,
        }
    }
}

/// 1 inside `range`, falling linearly to 0 over `falloff` outside it.
fn window(value: f32, range: [f32; 2], falloff: f32) -> f32 {
    let outside = (range[0] - value).max(value - range[1]);
    if outside <= 0.0 {
        1.0
    } else if falloff > 0.0 {
        (1.0 - outside / falloff).max(0.0)
    } else {
        0.0
    }
}

impl SpeciesRule {
    /// How well a place with `slope` (degrees) and `altitude` (meters) suits the
    /// species, in `[0, 1]`.
    #[must_use]
    pub fn suitability(&self, slope: f32, altitude: f32) -> f32 {
        window(slope, self.slope, self.slope_falloff)
            * window(altitude, self.altitude, self.altitude_falloff)
    }

    /// `true` when the slope or altitude range leaves out some ground.
    #[must_use]
    pub fn restricts(&self) -> bool {
        self.slope[0] > 0.0
            || self.slope[1] < 90.0
            || self.altitude[0].is_finite()
            || self.altitude[1].is_finite()
    }

    /// Mean of the scale range.
    #[must_use]
    pub fn mean_scale(&self) -> f32 {
        f32::midpoint(self.scale[0], self.scale[1])
    }

    /// Checks every field.
    ///
    /// # Errors
    /// A description of the first invalid field.
    pub fn validate(&self) -> Result<(), String> {
        let positive = |v: f32| v > 0.0 && v.is_finite();
        let non_negative = |v: f32| v >= 0.0 && v.is_finite();
        let checks = [
            (non_negative(self.density), "density must be >= 0"),
            (positive(self.spacing), "spacing must be > 0"),
            (
                positive(self.scale[0])
                    && self.scale[1] >= self.scale[0]
                    && self.scale[1].is_finite(),
                "scale must be [low, high] with 0 < low <= high",
            ),
            (self.slope[0] <= self.slope[1], "slope must be [low, high]"),
            (
                self.altitude[0] <= self.altitude[1],
                "altitude must be [low, high]",
            ),
            (
                non_negative(self.slope_falloff) && non_negative(self.altitude_falloff),
                "falloffs must be >= 0",
            ),
            (self.variants > 0, "variants must be >= 1"),
            (
                non_negative(self.lean_deg) && self.lean_deg < 90.0,
                "lean must be in [0, 90)",
            ),
            (
                non_negative(self.tint) && self.tint < 1.0,
                "tint must be in [0, 1)",
            ),
            (
                non_negative(self.trunk_radius) && self.sink.is_finite(),
                "trunk radius must be >= 0 and sink finite",
            ),
        ];
        match checks.iter().find(|(ok, _)| !ok) {
            Some((_, reason)) => Err((*reason).to_owned()),
            None => Ok(()),
        }
    }
}

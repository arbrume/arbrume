//! The water an aquatic plant grows in.
//!
//! [`Water`] is part of the [`crate::Surroundings`] of a plant, in meters in the frame
//! of the plant (the plant stands at the origin, Y up). Only species with an
//! `[aquatic]` layer read it; a land plant grows the same with or without water.
//!
//! * `surface` is the height of the water surface. When it is not given (water given
//!   only for its current, say), or without water in the surroundings, an aquatic
//!   species uses its own `aquatic.water_depth`.
//! * `current` is a steady current, a velocity in m/s. It loads the stems and leaves by
//!   drag, `0.5 rho Cd A u^2` (the quadratic drag law of a bluff body at high Reynolds
//!   number).
//! * `attenuation` is the diffuse attenuation coefficient `Kd` of light, in 1/m: the
//!   irradiance at depth `z` below the surface is `exp(-Kd z)` times that just below it
//!   (the Beer-Lambert law for downwelling light).
//! * `density` is that of the water, in kg/m³: about 1000 for fresh water and 1025 for
//!   sea water.

use glam::Vec3;
use serde::{Deserialize, Serialize};

/// Acceleration of gravity, in m/s².
pub const GRAVITY: f32 = 9.81;

/// Water around a plant. See the [module documentation](self).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Water {
    /// Height of the water surface above the base of the plant, in meters; `None` for
    /// the `water_depth` of the species.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surface: Option<f32>,
    /// Velocity of the current, in m/s; its vertical part is ignored.
    pub current: Vec3,
    /// Diffuse attenuation coefficient of light, in 1/m.
    pub attenuation: f32,
    /// Density of the water, in kg/m³.
    pub density: f32,
}

impl Default for Water {
    /// Still fresh water 2 m deep with an attenuation of 0.2 per meter: a starting
    /// point, not a measurement.
    fn default() -> Self {
        Self::still(2.0)
    }
}

impl Water {
    /// Still fresh water whose surface is at `surface`, with the default attenuation.
    #[must_use]
    pub const fn still(surface: f32) -> Self {
        Self {
            surface: Some(surface),
            current: Vec3::ZERO,
            attenuation: 0.2,
            density: 1000.0,
        }
    }

    /// Height of the surface: `surface`, or the 2 m of [`Water::default`] when it is
    /// not given. Aquatic growth resolves it from the species first.
    #[must_use]
    pub fn level(&self) -> f32 {
        self.surface.unwrap_or(2.0)
    }

    /// The horizontal velocity of the current.
    #[must_use]
    pub const fn flow(&self) -> Vec3 {
        Vec3::new(self.current.x, 0.0, self.current.z)
    }

    /// Fraction of the light just below the surface that reaches `height`: 1 at and
    /// above the surface.
    #[must_use]
    pub fn light_at(&self, height: f32) -> f32 {
        let depth = (self.level() - height).max(0.0);
        (-self.attenuation * depth).exp()
    }

    /// Checks that every value is finite and in its domain.
    ///
    /// # Errors
    ///
    /// A message naming the bad value.
    pub fn validate(&self) -> Result<(), String> {
        if !self.level().is_finite() || !self.current.is_finite() {
            return Err("water: surface and current must be finite".to_owned());
        }
        if !(self.attenuation >= 0.0 && self.attenuation.is_finite()) {
            return Err(format!(
                "water: attenuation {} is not a finite value of 0 or more",
                self.attenuation
            ));
        }
        if !(self.density > 0.0 && self.density.is_finite()) {
            return Err(format!("water: density {} is not positive", self.density));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Water;
    use glam::Vec3;

    #[test]
    fn light_falls_exponentially_with_depth() {
        let water = Water {
            attenuation: 0.5,
            ..Water::still(10.0)
        };
        assert!((water.light_at(10.0) - 1.0).abs() < 1e-6);
        assert!((water.light_at(12.0) - 1.0).abs() < 1e-6);
        let ratio = water.light_at(6.0) / water.light_at(8.0);
        assert!((ratio - (-1.0_f32).exp()).abs() < 1e-5);
    }

    #[test]
    fn values_are_checked_and_the_flow_is_horizontal() {
        Water::default().validate().unwrap();
        let bad = Water {
            attenuation: -1.0,
            ..Water::default()
        };
        assert!(bad.validate().is_err());
        let water = Water {
            current: Vec3::new(0.3, 0.5, -0.1),
            ..Water::default()
        };
        assert_eq!(water.flow(), Vec3::new(0.3, 0.0, -0.1));
        let parsed: Water = toml::from_str("surface = 4.0\ncurrent = [0.2, 0.0, 0.0]").unwrap();
        assert!((parsed.level() - 4.0).abs() < 1e-6 && parsed.density > 999.0);
    }
}

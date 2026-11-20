//! Water from the command line: `--water-depth`, `--current` and `--water-attenuation`.
//!
//! The water an aquatic plant grows in is part of its surroundings (`[water]` in a
//! surroundings file, docs/underwater.md). These options set it without a file, or
//! change the water of the file: `--water-depth` is the height of the surface above the
//! base of the plant, `--current` the velocity of a steady current along X and Z in
//! m/s, `--water-attenuation` the attenuation of light per meter. Land plants ignore
//! water.

use std::sync::Arc;

use arbrume_gen::Surroundings;
use arbrume_grow::Water;
use clap::Args;
use glam::Vec3;

use crate::error::CliError;

/// The water options of `gen` and `sheet`.
#[derive(Debug, Clone, Default, Args)]
pub struct WaterArgs {
    /// Height of the water surface above the base of each aquatic plant, in meters
    /// [default: the `aquatic.water_depth` of the species] (docs/underwater.md).
    #[arg(long, value_name = "METERS")]
    pub water_depth: Option<f32>,
    /// Steady current, `x,z` in m/s.
    #[arg(long, value_name = "X,Z", value_parser = parse_current)]
    pub current: Option<Vec3>,
    /// Attenuation of light in the water, per meter [default: 0.2].
    #[arg(long, value_name = "PER_METER")]
    pub water_attenuation: Option<f32>,
}

impl WaterArgs {
    /// Whether any water option is given.
    pub const fn is_set(&self) -> bool {
        self.water_depth.is_some() || self.current.is_some() || self.water_attenuation.is_some()
    }

    /// `surroundings` with its water changed by these options; unchanged when none is
    /// given. Water created here without `--water-depth` takes the depth of each
    /// species.
    ///
    /// # Errors
    ///
    /// A usage error for water values out of their domain.
    pub fn apply(
        &self,
        surroundings: Option<Arc<Surroundings>>,
    ) -> Result<Option<Arc<Surroundings>>, CliError> {
        if !self.is_set() {
            return Ok(surroundings);
        }
        let mut changed = surroundings.as_deref().cloned().unwrap_or_default();
        let mut water = changed.water.unwrap_or(Water {
            surface: None,
            ..Water::default()
        });
        if let Some(depth) = self.water_depth {
            water.surface = Some(depth);
        }
        if let Some(current) = self.current {
            water.current = current;
        }
        if let Some(attenuation) = self.water_attenuation {
            water.attenuation = attenuation;
        }
        water.validate().map_err(CliError::usage)?;
        changed.water = Some(water);
        Ok(Some(Arc::new(changed)))
    }
}

/// Parses `x,z` into a horizontal velocity.
fn parse_current(text: &str) -> Result<Vec3, String> {
    let parts: Vec<&str> = text.split(',').map(str::trim).collect();
    let [x, z] = parts.as_slice() else {
        return Err(format!("expected `x,z` in m/s, got `{text}`"));
    };
    let number = |part: &str| {
        part.parse::<f32>()
            .map_err(|_| format!("`{part}` is not a number"))
    };
    Ok(Vec3::new(number(x)?, 0.0, number(z)?))
}

#[cfg(test)]
mod tests {
    use super::{WaterArgs, parse_current};
    use glam::Vec3;

    #[test]
    fn options_set_or_change_the_water() {
        assert_eq!(
            parse_current("0.3, -0.1").unwrap(),
            Vec3::new(0.3, 0.0, -0.1)
        );
        assert!(parse_current("0.3").is_err());
        assert!(WaterArgs::default().apply(None).unwrap().is_none());
        let args = WaterArgs {
            water_depth: Some(6.0),
            current: Some(Vec3::X * 0.2),
            water_attenuation: None,
        };
        let surroundings = args.apply(None).unwrap().unwrap();
        let water = surroundings.water.unwrap();
        assert!((water.level() - 6.0).abs() < 1e-6 && water.current.x > 0.1);
        let bad = WaterArgs {
            water_attenuation: Some(-1.0),
            ..WaterArgs::default()
        };
        assert!(bad.apply(None).is_err());
    }
}

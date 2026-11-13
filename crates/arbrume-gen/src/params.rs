//! Parameters of one plant and the error type of the pipeline.

use arbrume_core::species::SpeciesError;
use arbrume_grow::GrowParams;
use arbrume_mesh::{MeshError, MeshOptions};

/// Error from the generation pipeline.
#[derive(Debug, thiserror::Error)]
pub enum GenError {
    /// The species is invalid, or a species file could not be read or parsed.
    #[error(transparent)]
    Species(#[from] SpeciesError),
    /// A generation parameter is outside its domain.
    #[error("invalid parameter `{field}`: {reason}")]
    InvalidParam {
        /// Name of the parameter.
        field: &'static str,
        /// Why the value is rejected.
        reason: String,
    },
    /// An override of a species field cannot be applied.
    #[error("cannot override `{path}`: {reason}")]
    Override {
        /// Dotted path of the field.
        path: String,
        /// Why the override is rejected.
        reason: String,
    },
    /// The mesher rejected the plant or the options.
    #[error(transparent)]
    Mesh(#[from] MeshError),
}

/// Everything that defines one plant besides its species.
///
/// The plant is grown with `seed`, `age` and `scale` (see `arbrume_grow::GrowParams`) and
/// meshed with `mesh`. `mesh.seed` seeds the mesh-level variation (leaf order, size and
/// colour); [`GenParams::new`] sets it to the plant seed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GenParams {
    /// Seed of every random draw of the growth.
    pub seed: u64,
    /// Number of growth cycles.
    pub age: u16,
    /// Factor applied to every length and radius. Positive and finite.
    pub scale: f32,
    /// Resolution and level of detail of the mesh.
    pub mesh: MeshOptions,
}

impl Default for GenParams {
    /// Seed 0, the default age of `arbrume_grow::GrowParams`, scale 1, full quality.
    fn default() -> Self {
        Self::new(0, GrowParams::default().age)
    }
}

impl GenParams {
    /// Parameters at scale 1 and full mesh quality, the mesh seeded with `seed`.
    #[must_use]
    pub fn new(seed: u64, age: u16) -> Self {
        Self {
            seed,
            age,
            scale: 1.0,
            mesh: MeshOptions {
                seed,
                ..MeshOptions::default()
            },
        }
    }

    /// The same parameters with another scale.
    #[must_use]
    pub const fn with_scale(mut self, scale: f32) -> Self {
        self.scale = scale;
        self
    }

    /// The same parameters with the level-of-detail controls of
    /// `MeshOptions::with_quality(quality)`. The mesh seed, `max_cycle` and `parallel`
    /// are kept.
    #[must_use]
    pub fn with_quality(mut self, quality: f32) -> Self {
        self.mesh = quality_options(&self.mesh, quality);
        self
    }

    /// The parameters of the growth stage.
    #[must_use]
    pub const fn grow_params(&self) -> GrowParams {
        GrowParams {
            seed: self.seed,
            age: self.age,
            scale: self.scale,
        }
    }

    /// Checks the scale and the mesh options.
    ///
    /// # Errors
    ///
    /// Returns [`GenError::InvalidParam`] for a scale that is not positive and finite,
    /// and [`GenError::Mesh`] for invalid mesh options.
    pub fn validate(&self) -> Result<(), GenError> {
        if !(self.scale > 0.0 && self.scale.is_finite()) {
            return Err(GenError::InvalidParam {
                field: "scale",
                reason: format!("{} is not positive and finite", self.scale),
            });
        }
        self.mesh.validate()?;
        Ok(())
    }
}

/// `MeshOptions::with_quality(quality)` with the seed, `max_cycle` and `parallel` of
/// `base`.
#[must_use]
pub(crate) fn quality_options(base: &MeshOptions, quality: f32) -> MeshOptions {
    MeshOptions {
        seed: base.seed,
        max_cycle: base.max_cycle,
        parallel: base.parallel,
        ..MeshOptions::with_quality(quality)
    }
}

#[cfg(test)]
mod tests {
    use super::{GenError, GenParams};
    use arbrume_mesh::MeshOptions;

    #[test]
    fn quality_keeps_the_seed() {
        let params = GenParams::new(9, 5).with_quality(0.5);
        assert_eq!(params.mesh.seed, 9);
        assert_eq!(
            params.mesh.tolerance,
            MeshOptions::with_quality(0.5).tolerance
        );
        params.validate().unwrap();
    }

    #[test]
    fn bad_scale_is_rejected() {
        for scale in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            let result = GenParams::new(1, 1).with_scale(scale).validate();
            assert!(matches!(
                result,
                Err(GenError::InvalidParam { field: "scale", .. })
            ));
        }
    }
}

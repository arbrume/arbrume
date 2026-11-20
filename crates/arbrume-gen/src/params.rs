//! Parameters of one plant and the error type of the pipeline.

use arbrume_core::species::SpeciesError;
use std::sync::Arc;

use arbrume_grow::{GrowParams, Guides, Soil, Surroundings};
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
    /// An image texture of the species cannot be read or decoded.
    #[error(transparent)]
    Texture(#[from] arbrume_texture::TextureError),
}

/// Everything that defines one plant besides its species.
///
/// The plant is grown with `seed`, `age` and `scale` (see `arbrume_grow::GrowParams`) and
/// meshed with `mesh`. `mesh.seed` seeds the mesh-level variation (leaf order, size and
/// colour); [`GenParams::new`] sets it to the plant seed. A species in the
/// self-organizing mode also responds to `surroundings`.
#[derive(Debug, Clone, PartialEq)]
pub struct GenParams {
    /// Seed of every random draw of the growth.
    pub seed: u64,
    /// Number of growth cycles.
    pub age: u16,
    /// Factor applied to every length and radius. Positive and finite.
    pub scale: f32,
    /// Resolution and level of detail of the mesh.
    pub mesh: MeshOptions,
    /// Envelope, obstacles and neighbours around the plant, in meters in the frame of
    /// the plant; `None` for open ground. Read by the self-organizing mode only.
    pub surroundings: Option<Arc<Surroundings>>,
    /// Strokes and freezes that steer the growth (`arbrume_grow::guides`), in meters in
    /// the frame of the plant; `None` for none. Read by the self-organizing mode only.
    pub guides: Option<Arc<Guides>>,
    /// Ground and resources the roots grow in, for a species with a `[roots]` layer;
    /// `None` for homogeneous soil under flat ground. Its obstacles are the blocking
    /// occluders of `surroundings`.
    pub soil: Option<Arc<Soil>>,
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
            surroundings: None,
            guides: None,
            soil: None,
        }
    }

    /// The same parameters with the plant standing in `surroundings`.
    #[must_use]
    pub fn with_surroundings(mut self, surroundings: impl Into<Arc<Surroundings>>) -> Self {
        self.surroundings = Some(surroundings.into());
        self
    }

    /// The same parameters with the growth steered by `guides`.
    #[must_use]
    pub fn with_guides(mut self, guides: impl Into<Arc<Guides>>) -> Self {
        self.guides = Some(guides.into());
        self
    }

    /// The guides of the plant when they have strokes or freezes.
    #[must_use]
    pub fn active_guides(&self) -> Option<&Guides> {
        self.guides.as_deref().filter(|guides| !guides.is_empty())
    }

    /// The same parameters with the roots growing in `soil`.
    #[must_use]
    pub fn with_soil(mut self, soil: impl Into<Arc<Soil>>) -> Self {
        self.soil = Some(soil.into());
        self
    }

    /// The same parameters with another scale.
    #[must_use]
    pub const fn with_scale(mut self, scale: f32) -> Self {
        self.scale = scale;
        self
    }

    /// The same parameters with the level-of-detail controls of
    /// `MeshOptions::with_quality(quality)`. The mesh seed, `max_cycle`, `parallel` and
    /// junctions are kept.
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

    /// Checks the scale, the surroundings, the guides and the mesh options.
    ///
    /// # Errors
    ///
    /// Returns [`GenError::InvalidParam`] for a scale that is not positive and finite or
    /// for surroundings with an empty or infinite volume or invalid guides, and
    /// [`GenError::Mesh`] for
    /// invalid mesh options.
    pub fn validate(&self) -> Result<(), GenError> {
        if !(self.scale > 0.0 && self.scale.is_finite()) {
            return Err(GenError::InvalidParam {
                field: "scale",
                reason: format!("{} is not positive and finite", self.scale),
            });
        }
        if let Some(surroundings) = &self.surroundings {
            surroundings
                .validate()
                .map_err(|reason| GenError::InvalidParam {
                    field: "surroundings",
                    reason,
                })?;
        }
        if let Some(guides) = &self.guides {
            guides.validate().map_err(|reason| GenError::InvalidParam {
                field: "guides",
                reason,
            })?;
        }
        self.mesh.validate()?;
        Ok(())
    }
}

/// `MeshOptions::with_quality(quality)` with the seed, `max_cycle`, `parallel` and
/// junctions of `base`.
#[must_use]
pub(crate) fn quality_options(base: &MeshOptions, quality: f32) -> MeshOptions {
    MeshOptions {
        seed: base.seed,
        max_cycle: base.max_cycle,
        parallel: base.parallel,
        junctions: base.junctions,
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

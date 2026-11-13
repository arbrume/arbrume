//! Species schema, version 0.
//!
//! A [`Species`] is the complete, serializable description of one kind of plant. It is
//! read from a TOML file and organized in layers, one module each, so that they can
//! evolve independently:
//!
//! | Table in the file | Type | Content |
//! |---|---|---|
//! | `[architecture]` | [`Architecture`] | qualitative traits: branching, rhythm, phyllotaxis, axis categories |
//! | `[growth]` | [`Growth`] | lengths, angles, branching and tropism, as curves |
//! | `[geometry]` | [`Geometry`] | radii: pipe exponent, taper, flare |
//! | `[foliage]` | [`Foliage`] | leaf size, attitude, retention, appearance |
//! | `[bark]` | [`Bark`] | bark appearance |
//!
//! Every field has a default, so a species file lists only what differs. The defaults
//! describe a plain generic tree. They are starting points chosen to produce a plausible
//! shape, not measurements of any species. Unknown keys are rejected, so a misspelt
//! parameter is an error instead of a silent default.
//!
//! Most numeric parameters are [`OrderCurve`]s: functions of
//! the branching order and of a relative position in `[0, 1]`, documented per field as
//! either *along the parent axis* (where the lateral is attached) or *along the axis
//! itself*. A bare number in the file is a constant.
//!
//! ```
//! use arbrume_core::species::Species;
//!
//! let species = Species::from_toml_str(
//!     r#"
//!     name = "Example"
//!     [growth]
//!     branching_angle = { by_order = [[[0.0, 80.0], [1.0, 35.0]], 45.0] }
//!     "#,
//! )
//! .unwrap();
//! assert!(species.growth.branching_angle.eval(0, 1.0) < 36.0);
//! ```

mod architecture;
mod bark;
mod foliage;
mod geometry;
mod growth;

pub use architecture::{
    Architecture, AxisCategory, AxisOrientation, BranchTiming, Branching, BranchingPattern,
    GrowthRhythm, Phyllotaxis,
};
pub use bark::Bark;
pub use foliage::Foliage;
pub use geometry::{Geometry, RadiusModel};
pub use growth::{Growth, SelfOrganization};

use crate::curve::OrderCurve;
use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

/// Version of the species schema written by this crate.
pub const SCHEMA_VERSION: u32 = 0;

/// Error from loading, saving or validating a [`Species`].
#[derive(Debug, Error)]
pub enum SpeciesError {
    /// The file could not be read.
    #[error("cannot read species file: {0}")]
    Io(#[from] std::io::Error),
    /// The text is not valid TOML or does not match the schema.
    #[error("cannot parse species: {0}")]
    Parse(#[from] toml::de::Error),
    /// The species could not be written as TOML.
    #[error("cannot serialize species: {0}")]
    Serialize(#[from] toml::ser::Error),
    /// The file was written for another schema version.
    #[error("species schema version {found} is not supported (expected {SCHEMA_VERSION})")]
    UnsupportedVersion {
        /// Version found in the file.
        found: u32,
    },
    /// A parameter has a value outside its domain.
    #[error("invalid species parameter `{field}`: {reason}")]
    Invalid {
        /// Dotted path of the parameter, as written in the file.
        field: &'static str,
        /// Why the value is rejected.
        reason: String,
    },
}

/// Complete description of one kind of plant. See the [module documentation](self).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Species {
    /// Schema version of the file; must equal [`SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Display name.
    pub name: String,
    /// Qualitative architectural traits.
    pub architecture: Architecture,
    /// Growth parameters.
    pub growth: Growth,
    /// Radius parameters.
    pub geometry: Geometry,
    /// Foliage parameters.
    pub foliage: Foliage,
    /// Bark parameters.
    pub bark: Bark,
}

impl Default for Species {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            name: String::from("generic"),
            architecture: Architecture::default(),
            growth: Growth::default(),
            geometry: Geometry::default(),
            foliage: Foliage::default(),
            bark: Bark::default(),
        }
    }
}

impl Species {
    /// Parses and validates a species from TOML text.
    ///
    /// # Errors
    ///
    /// Returns [`SpeciesError::Parse`] for malformed text, unknown keys or invalid
    /// curves, and the errors of [`Species::validate`].
    pub fn from_toml_str(text: &str) -> Result<Self, SpeciesError> {
        let species: Self = toml::from_str(text)?;
        species.validate()?;
        Ok(species)
    }

    /// Reads, parses and validates a species file.
    ///
    /// # Errors
    ///
    /// Returns [`SpeciesError::Io`] when the file cannot be read, and the errors of
    /// [`Species::from_toml_str`].
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, SpeciesError> {
        Self::from_toml_str(&std::fs::read_to_string(path)?)
    }

    /// Writes the species as TOML text, every field included.
    ///
    /// # Errors
    ///
    /// Returns [`SpeciesError::Serialize`] when a value cannot be represented in TOML.
    pub fn to_toml_string(&self) -> Result<String, SpeciesError> {
        Ok(toml::to_string(self)?)
    }

    /// Checks the schema version and the domain of every parameter.
    ///
    /// # Errors
    ///
    /// Returns [`SpeciesError::UnsupportedVersion`] or the first
    /// [`SpeciesError::Invalid`] parameter found.
    pub fn validate(&self) -> Result<(), SpeciesError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(SpeciesError::UnsupportedVersion {
                found: self.schema_version,
            });
        }
        self.architecture.validate()?;
        self.growth.validate()?;
        self.geometry.validate()?;
        self.foliage.validate()?;
        self.bark.validate()
    }
}

/// Fails with [`SpeciesError::Invalid`] unless `lo <= value <= hi` (NaN fails).
fn check_range(field: &'static str, value: f32, lo: f32, hi: f32) -> Result<(), SpeciesError> {
    if value >= lo && value <= hi {
        return Ok(());
    }
    Err(SpeciesError::Invalid {
        field,
        reason: format!("{value} is outside [{lo}, {hi}]"),
    })
}

/// Fails with [`SpeciesError::Invalid`] unless every value of `curve` is in `[lo, hi]`.
fn check_curve(
    field: &'static str,
    curve: &OrderCurve,
    lo: f32,
    hi: f32,
) -> Result<(), SpeciesError> {
    let (curve_lo, curve_hi) = curve.bounds();
    check_range(field, curve_lo, lo, hi)?;
    check_range(field, curve_hi, lo, hi)
}

/// Fails with [`SpeciesError::Invalid`] unless every component is in `[0, 1]`.
fn check_color(field: &'static str, color: [f32; 3]) -> Result<(), SpeciesError> {
    color
        .iter()
        .try_for_each(|&channel| check_range(field, channel, 0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::{SCHEMA_VERSION, Species, SpeciesError};

    #[test]
    fn default_species_is_valid_and_round_trips() {
        let species = Species::default();
        species.validate().unwrap();
        let text = species.to_toml_string().unwrap();
        assert_eq!(Species::from_toml_str(&text).unwrap(), species);
    }

    #[test]
    fn empty_file_is_the_default_species() {
        assert_eq!(Species::from_toml_str("").unwrap(), Species::default());
    }

    #[test]
    fn unknown_keys_are_rejected() {
        let result = Species::from_toml_str("[growth]\nbranching_angel = 40.0\n");
        assert!(matches!(result, Err(SpeciesError::Parse(_))));
    }

    #[test]
    fn other_schema_versions_are_rejected() {
        let result = Species::from_toml_str("schema_version = 7");
        assert!(matches!(
            result,
            Err(SpeciesError::UnsupportedVersion { found: 7 })
        ));
        assert_eq!(SCHEMA_VERSION, 0);
    }

    #[test]
    fn out_of_domain_values_name_the_field() {
        let cases = [
            (
                "[growth]\nbranch_probability = 1.5",
                "growth.branch_probability",
            ),
            (
                "[growth]\ninternode_length = [[0.0, 0.1], [1.0, 0.0]]",
                "growth.internode_length",
            ),
            (
                "[growth]\ntropism_weight = 0.8\nenvironment_weight = 0.5",
                "growth.tropism_weight",
            ),
            ("[geometry]\npipe_exponent = 0.0", "geometry.pipe_exponent"),
            ("[foliage]\nwidth_ratio = -1.0", "foliage.width_ratio"),
            ("[bark]\ncolor = [0.2, 0.2, 3.0]", "bark.color"),
            (
                "[architecture]\nphyllotaxis = { leaves_per_node = 0 }",
                "architecture.phyllotaxis",
            ),
        ];
        for (text, expected) in cases {
            match Species::from_toml_str(text) {
                Err(SpeciesError::Invalid { field, .. }) => assert_eq!(field, expected),
                other => panic!("{text}: expected an invalid `{expected}`, got {other:?}"),
            }
        }
    }

    #[test]
    fn missing_file_is_an_io_error() {
        let result = Species::from_path("does/not/exist.toml");
        assert!(matches!(result, Err(SpeciesError::Io(_))));
    }
}

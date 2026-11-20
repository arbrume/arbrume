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
//! | `[foliage]` | [`Foliage`] | leaf size, attitude, retention, twigs, appearance |
//! | `[bark]` | [`Bark`] | bark appearance |
//! | `[variation]` | [`Variation`] | how individuals differ: age and scale ranges, jitter |
//!
//! `bark.texture`, `bark.normal_texture`, `foliage.texture` and `foliage.normal_texture`
//! name image files that replace the synthesized textures; relative paths are resolved
//! against [`Species::asset_dir`], which is set by whoever loads the file (a species
//! pack, see `docs/packs.md`) and is not part of the file.
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
mod leaf_texture;
mod variation;

pub use architecture::{
    Architecture, AxisCategory, AxisOrientation, BranchTiming, Branching, BranchingPattern,
    GrowthRhythm, Phyllotaxis,
};
pub use bark::{Bark, BarkKind, BarkPattern, MAX_TEXTURE_RESOLUTION, MIN_TEXTURE_RESOLUTION};
pub use foliage::{
    BladeShape, Foliage, LeafGeometry, MAX_BLADE_SEGMENTS, MAX_LEAFLET_PAIRS, MAX_TWIG_FINS,
    MAX_TWIG_LEAVES, MAX_TWIG_VARIANTS, Twig,
};
pub use geometry::{Geometry, RadiusModel};
pub use growth::{Allocation, Growth, GrowthMode, SelfOrganization, Sensing};
pub use leaf_texture::{
    BladePlacement, LeafAtlas, LeafLayout, MAX_TEXTURE_VARIANTS, MAX_VEINS, Margin, MarginKind,
    Venation, VenationKind,
};
pub use variation::{JITTERABLE, Variation};

use crate::curve::OrderCurve;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
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
    /// How individuals of the species vary.
    pub variation: Variation,
    /// Directory against which relative texture paths are resolved: the directory of
    /// the species pack or file the species was loaded from. Not read from nor written
    /// to the file; `None` resolves them against the current directory.
    #[serde(skip)]
    pub asset_dir: Option<PathBuf>,
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
            variation: Variation::default(),
            asset_dir: None,
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

    /// The image files the species names, as `(field, path)` pairs, in the order
    /// `bark.texture`, `bark.normal_texture`, `foliage.texture`, `foliage.normal_texture`.
    #[must_use]
    pub fn texture_paths(&self) -> Vec<(&'static str, &str)> {
        [
            ("bark.texture", &self.bark.texture),
            ("bark.normal_texture", &self.bark.normal_texture),
            ("foliage.texture", &self.foliage.texture),
            ("foliage.normal_texture", &self.foliage.normal_texture),
        ]
        .into_iter()
        .filter_map(|(field, path)| path.as_deref().map(|path| (field, path)))
        .collect()
    }

    /// Where the image file `path` of the species is: `path` itself when absolute,
    /// else `path` under [`Species::asset_dir`] (or the current directory).
    #[must_use]
    pub fn resolve_asset(&self, path: &str) -> PathBuf {
        let path = Path::new(path);
        match &self.asset_dir {
            Some(dir) if path.is_relative() => dir.join(path),
            _ => path.to_path_buf(),
        }
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
        self.bark.validate()?;
        self.variation.validate()
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

/// Fails with [`SpeciesError::Invalid`] for an empty texture path of the table `table`
/// (`"bark"` or `"foliage"`), or a normal map without a texture.
fn check_texture_paths(
    table: &'static str,
    texture: Option<&str>,
    normal: Option<&str>,
) -> Result<(), SpeciesError> {
    let (texture_field, normal_field) = if table == "bark" {
        ("bark.texture", "bark.normal_texture")
    } else {
        ("foliage.texture", "foliage.normal_texture")
    };
    let invalid = |field, reason: &str| SpeciesError::Invalid {
        field,
        reason: reason.to_owned(),
    };
    if texture.is_some_and(str::is_empty) {
        return Err(invalid(texture_field, "is an empty path"));
    }
    if normal.is_some_and(str::is_empty) {
        return Err(invalid(normal_field, "is an empty path"));
    }
    if normal.is_some() && texture.is_none() {
        return Err(invalid(normal_field, "is only read with a texture"));
    }
    Ok(())
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
                "[bark]\ntexture_resolution = 500",
                "bark.texture_resolution",
            ),
            (
                "[bark]\ntexture_resolution = 8192",
                "bark.texture_resolution",
            ),
            (
                "[bark]\npattern = { stretch = 0.0 }",
                "bark.pattern.stretch",
            ),
            (
                "[bark]\npattern = { coverage = 1.5 }",
                "bark.pattern.coverage",
            ),
            (
                "[bark]\npattern = { color = [0.1, 2.0, 0.1] }",
                "bark.pattern.color",
            ),
            (
                "[foliage]\ntexture_variants = 0",
                "foliage.texture_variants",
            ),
            (
                "[foliage]\ntexture_resolution = 3",
                "foliage.texture_resolution",
            ),
            (
                "[foliage]\nvenation = { angle_deg = 90.0 }",
                "foliage.venation.angle_deg",
            ),
            (
                "[foliage]\nvenation = { veins = 100 }",
                "foliage.venation.veins",
            ),
            (
                "[foliage]\nmargin = { depth = 0.95 }",
                "foliage.margin.depth",
            ),
            (
                "[architecture]\nphyllotaxis = { leaves_per_node = 0 }",
                "architecture.phyllotaxis",
            ),
            ("[bark]\ntexture = \"\"", "bark.texture"),
            ("[bark]\nnormal_texture = \"n.png\"", "bark.normal_texture"),
            (
                "[foliage]\ntexture = \"a.png\"\nnormal_texture = \"\"",
                "foliage.normal_texture",
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
    fn texture_tables_parse_and_reject_unknown_keys() {
        let species = Species::from_toml_str(
            "[bark]\npattern = { kind = \"plated\", feature_size = 0.08 }\n\
             [foliage]\nvenation = { kind = \"open\" }\nmargin = { kind = \"lobed\" }",
        )
        .unwrap();
        assert_eq!(species.bark.pattern.kind, super::BarkKind::Plated);
        assert_eq!(species.foliage.venation.kind, super::VenationKind::Open);
        assert_eq!(species.foliage.margin.kind, super::MarginKind::Lobed);
        for text in [
            "[bark]\npattern = { knd = \"plated\" }",
            "[bark]\npattern = { kind = \"scaly\" }",
            "[foliage]\nvenation = { veinz = 3 }",
            "[foliage]\nmargin = { kind = \"wavy\" }",
        ] {
            let result = Species::from_toml_str(text);
            assert!(matches!(result, Err(SpeciesError::Parse(_))), "{text}");
        }
    }

    #[test]
    fn texture_paths_are_listed_and_resolved_against_the_asset_dir() {
        let mut species = Species::from_toml_str(
            "[bark]\ntexture = \"textures/bark.png\"\n[foliage]\n\
             texture = \"textures/leaf.png\"\nnormal_texture = \"textures/n.png\"",
        )
        .unwrap();
        let fields: Vec<&str> = species.texture_paths().iter().map(|p| p.0).collect();
        assert_eq!(
            fields,
            ["bark.texture", "foliage.texture", "foliage.normal_texture"]
        );
        let relative = std::path::Path::new("textures/bark.png");
        assert_eq!(species.resolve_asset("textures/bark.png"), relative);
        species.asset_dir = Some("packs/demo".into());
        let resolved = species.resolve_asset("textures/bark.png");
        assert_eq!(resolved, std::path::Path::new("packs/demo").join(relative));
        let text = species.to_toml_string().unwrap();
        assert!(!text.contains("packs/demo"));
        assert_eq!(Species::from_toml_str(&text).unwrap().asset_dir, None);
    }

    #[test]
    fn missing_file_is_an_io_error() {
        let result = Species::from_path("does/not/exist.toml");
        assert!(matches!(result, Err(SpeciesError::Io(_))));
    }
}

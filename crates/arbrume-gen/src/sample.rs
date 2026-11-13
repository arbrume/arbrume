//! Per-plant parameters of a dataset, drawn from a base seed and an index.
//!
//! Plant `index` of a dataset with base seed `seed` gets:
//!
//! * the plant seed [`plant_seed`]`(seed, index)`, which also seeds its mesh;
//! * an age and a scale drawn from their [`Distribution`]s with the counter-based
//!   generator of `arbrume-core`, keyed by `(seed, index)` and one custom stream each.
//!
//! Every value is a pure function of `(seed, index)` and the distributions, so plants
//! can be generated in any order, on any number of threads, or one at a time to resume
//! an interrupted batch.

use arbrume_core::rng::{Rng, Stream};
use serde::{Deserialize, Serialize};

use crate::params::{GenError, GenParams, quality_options};

/// Stream of the plant seed.
const SEED_STREAM: Stream = Stream::custom(0);
/// Stream of the age draw.
const AGE_STREAM: Stream = Stream::custom(1);
/// Stream of the scale draw.
const SCALE_STREAM: Stream = Stream::custom(2);

/// Seed of plant `index` of a dataset with base seed `seed`.
#[must_use]
pub const fn plant_seed(seed: u64, index: u32) -> u64 {
    Rng::new(seed, index, SEED_STREAM).next_u64()
}

/// A distribution of a numeric parameter.
///
/// In TOML a bare number is [`Distribution::Fixed`], `{ uniform = [lo, hi] }` is
/// [`Distribution::Uniform`] and `{ normal = [mean, std_dev], min = .., max = .. }` is
/// [`Distribution::Normal`], `min` and `max` being optional.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "DistributionDef", into = "DistributionDef")]
pub enum Distribution {
    /// Always this value.
    Fixed(f32),
    /// Uniform between `lo` and `hi`. For an integer parameter every integer from
    /// `round(lo)` to `round(hi)` inclusive is equally likely.
    Uniform {
        /// Lower bound.
        lo: f32,
        /// Upper bound.
        hi: f32,
    },
    /// Normal, then clamped to `[min, max]` where given.
    Normal {
        /// Mean.
        mean: f32,
        /// Standard deviation, zero or more.
        std_dev: f32,
        /// Lower clamp.
        min: Option<f32>,
        /// Upper clamp.
        max: Option<f32>,
    },
}

/// Serialized form of a [`Distribution`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum DistributionDef {
    Fixed(f32),
    Table(DistributionTable),
}

/// Table form of a [`Distribution`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DistributionTable {
    #[serde(skip_serializing_if = "Option::is_none")]
    uniform: Option<[f32; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    normal: Option<[f32; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    min: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max: Option<f32>,
}

impl TryFrom<DistributionDef> for Distribution {
    type Error = String;

    fn try_from(def: DistributionDef) -> Result<Self, String> {
        let table = match def {
            DistributionDef::Fixed(value) => return Ok(Self::Fixed(value)),
            DistributionDef::Table(table) => table,
        };
        let distribution = match (table.uniform, table.normal) {
            (Some([lo, hi]), None) if table.min.is_none() && table.max.is_none() => {
                Self::Uniform { lo, hi }
            }
            (None, Some([mean, std_dev])) => Self::Normal {
                mean,
                std_dev,
                min: table.min,
                max: table.max,
            },
            _ => {
                return Err("expected a number, { uniform = [lo, hi] } or \
                            { normal = [mean, std_dev], min = .., max = .. }"
                    .to_owned());
            }
        };
        distribution.validate()?;
        Ok(distribution)
    }
}

impl From<Distribution> for DistributionDef {
    fn from(distribution: Distribution) -> Self {
        match distribution {
            Distribution::Fixed(value) => Self::Fixed(value),
            Distribution::Uniform { lo, hi } => Self::Table(DistributionTable {
                uniform: Some([lo, hi]),
                ..DistributionTable::default()
            }),
            Distribution::Normal {
                mean,
                std_dev,
                min,
                max,
            } => Self::Table(DistributionTable {
                normal: Some([mean, std_dev]),
                min,
                max,
                ..DistributionTable::default()
            }),
        }
    }
}

impl Distribution {
    /// Checks that every number is finite, `lo <= hi`, `std_dev >= 0` and `min <= max`.
    ///
    /// # Errors
    ///
    /// Returns a description of the first problem found.
    pub fn validate(&self) -> Result<(), String> {
        let numbers: Vec<f32> = match *self {
            Self::Fixed(value) => vec![value],
            Self::Uniform { lo, hi } => vec![lo, hi],
            Self::Normal {
                mean,
                std_dev,
                min,
                max,
            } => [Some(mean), Some(std_dev), min, max]
                .into_iter()
                .flatten()
                .collect(),
        };
        if numbers.iter().any(|value| !value.is_finite()) {
            return Err(format!("{self:?} has a value that is not finite"));
        }
        let (lo, hi) = self.support();
        if lo > hi {
            return Err(format!(
                "{self:?} has its lower bound above its upper bound"
            ));
        }
        if let Self::Normal { std_dev, .. } = *self
            && std_dev < 0.0
        {
            return Err(format!("{self:?} has a negative standard deviation"));
        }
        Ok(())
    }

    /// Smallest and largest values a draw can take (infinite for an unclamped normal).
    #[must_use]
    pub fn support(&self) -> (f32, f32) {
        match *self {
            Self::Fixed(value) => (value, value),
            Self::Uniform { lo, hi } => (lo, hi),
            Self::Normal { min, max, .. } => (
                min.unwrap_or(f32::NEG_INFINITY),
                max.unwrap_or(f32::INFINITY),
            ),
        }
    }

    /// One draw of a real parameter.
    pub fn sample(&self, rng: &mut Rng) -> f32 {
        match *self {
            Self::Fixed(value) => value,
            Self::Uniform { lo, hi } => rng.range(lo, hi),
            Self::Normal {
                mean,
                std_dev,
                min,
                max,
            } => {
                let value = rng.normal_with(mean, std_dev);
                value
                    .max(min.unwrap_or(f32::NEG_INFINITY))
                    .min(max.unwrap_or(f32::INFINITY))
            }
        }
    }

    /// One draw of an integer parameter, rounded to the nearest integer.
    pub fn sample_integer(&self, rng: &mut Rng) -> f64 {
        match *self {
            Self::Uniform { lo, hi } => {
                let (lo, hi) = (f64::from(lo).round(), f64::from(hi).round());
                let span = (hi - lo) as u32;
                lo + f64::from(rng.below(span.saturating_add(1)))
            }
            _ => f64::from(self.sample(rng)).round(),
        }
    }
}

/// Draws the [`GenParams`] of the plants of a dataset.
///
/// Plant `index` gets the seed [`plant_seed`]`(seed, index)`, which also seeds its mesh,
/// and an age and a scale drawn with the counter-based generator of `arbrume-core`,
/// keyed by `(seed, index)` and a stream of their own. Every value is a pure function of
/// `(seed, index)` and the distributions, so plants can be generated in any order, on
/// any number of threads, or one at a time to resume an interrupted batch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sampler {
    /// Base seed of the dataset.
    pub seed: u64,
    /// Distribution of the age in growth cycles.
    pub age: Distribution,
    /// Distribution of the scale.
    pub scale: Distribution,
    /// Mesh quality, as in `MeshOptions::with_quality`.
    pub quality: f32,
}

impl Sampler {
    /// Checks that every draw gives valid parameters: ages within `[0, 65535]`, scales
    /// positive and a quality positive and finite.
    ///
    /// # Errors
    ///
    /// Returns [`GenError::InvalidParam`] naming the parameter.
    pub fn validate(&self) -> Result<(), GenError> {
        let invalid = |field, reason: String| GenError::InvalidParam { field, reason };
        self.age.validate().map_err(|r| invalid("age", r))?;
        self.scale.validate().map_err(|r| invalid("scale", r))?;
        let (age_lo, age_hi) = self.age.support();
        if age_lo.round() < 0.0 || age_hi.round() > f32::from(u16::MAX) {
            let reason = format!(
                "{:?} can draw outside [0, 65535]; set min and max",
                self.age
            );
            return Err(invalid("age", reason));
        }
        if self.scale.support().0 <= 0.0 {
            let reason = format!("{:?} can draw a scale <= 0; set min", self.scale);
            return Err(invalid("scale", reason));
        }
        if !(self.quality > 0.0 && self.quality.is_finite()) {
            let reason = format!("{} is not positive and finite", self.quality);
            return Err(invalid("quality", reason));
        }
        Ok(())
    }

    /// Parameters of plant `index`. The sampler should be valid ([`Sampler::validate`]).
    #[must_use]
    pub fn params(&self, index: u32) -> GenParams {
        let seed = plant_seed(self.seed, index);
        let age = self
            .age
            .sample_integer(&mut Rng::new(self.seed, index, AGE_STREAM));
        let scale = self
            .scale
            .sample(&mut Rng::new(self.seed, index, SCALE_STREAM));
        let mut params = GenParams::new(seed, age.clamp(0.0, f64::from(u16::MAX)) as u16);
        params.scale = scale;
        params.mesh = quality_options(&params.mesh, self.quality);
        params
    }
}

#[cfg(test)]
mod tests {
    use super::{Distribution, Sampler, plant_seed};
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Holder {
        value: Distribution,
    }

    fn parse(text: &str) -> Result<Distribution, toml::de::Error> {
        toml::from_str::<Holder>(&format!("value = {text}")).map(|h| h.value)
    }

    #[test]
    fn distributions_parse_from_toml() {
        assert_eq!(parse("20").unwrap(), Distribution::Fixed(20.0));
        assert_eq!(
            parse("{ uniform = [10, 30] }").unwrap(),
            Distribution::Uniform { lo: 10.0, hi: 30.0 }
        );
        assert_eq!(
            parse("{ normal = [1.0, 0.1], min = 0.5 }").unwrap(),
            Distribution::Normal {
                mean: 1.0,
                std_dev: 0.1,
                min: Some(0.5),
                max: None
            }
        );
        assert!(parse("{ uniform = [3, 1] }").is_err());
        assert!(parse("{ uniform = [1, 3], normal = [1, 1] }").is_err());
        assert!(parse("{ normal = [1, -1] }").is_err());
        assert!(parse("{ triangle = [1, 2] }").is_err());
    }

    #[test]
    fn integer_uniform_covers_both_ends() {
        let sampler = Sampler {
            seed: 5,
            age: Distribution::Uniform { lo: 3.0, hi: 5.0 },
            scale: Distribution::Fixed(1.0),
            quality: 1.0,
        };
        sampler.validate().unwrap();
        let mut seen = [false; 3];
        for index in 0..200 {
            let age = sampler.params(index).age;
            assert!((3..=5).contains(&age));
            seen[usize::from(age - 3)] = true;
        }
        assert_eq!(seen, [true; 3]);
    }

    #[test]
    fn draws_depend_on_seed_and_index_only() {
        let sampler = Sampler {
            seed: 11,
            age: Distribution::Uniform { lo: 1.0, hi: 40.0 },
            scale: Distribution::Normal {
                mean: 1.0,
                std_dev: 0.2,
                min: Some(0.5),
                max: Some(1.5),
            },
            quality: 0.5,
        };
        sampler.validate().unwrap();
        assert_eq!(sampler.params(7), sampler.params(7));
        assert_ne!(sampler.params(7).seed, sampler.params(8).seed);
        assert_eq!(sampler.params(7).seed, plant_seed(11, 7));
        assert_eq!(sampler.params(7).mesh.seed, plant_seed(11, 7));
        let scale = sampler.params(3).scale;
        assert!((0.5..=1.5).contains(&scale));
    }

    #[test]
    fn unbounded_draws_are_rejected() {
        let mut sampler = Sampler {
            seed: 0,
            age: Distribution::Normal {
                mean: 10.0,
                std_dev: 2.0,
                min: None,
                max: None,
            },
            scale: Distribution::Fixed(1.0),
            quality: 1.0,
        };
        assert!(sampler.validate().is_err());
        sampler.age = Distribution::Fixed(10.0);
        sampler.scale = Distribution::Uniform { lo: 0.0, hi: 1.0 };
        assert!(sampler.validate().is_err());
    }
}

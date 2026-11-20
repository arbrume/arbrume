//! The search space of a fit: which species parameters move, and how.
//!
//! Each fitted parameter is a coordinate `x` of the search: a jitterable species
//! parameter (`arbrume_core::species::JITTERABLE`) is multiplied by `exp(x)` with
//! `Species::with_factor`, exactly as a jitter would multiply it (one factor for every
//! order of a curve, clamped to the domain of the parameter); the age is
//! `round(base_age * exp(x))`, at least 1; the scale is `exp(x)`. Log factors make a
//! halving and a doubling equally far from the start, and keep lengths positive. A
//! parameter that is 0 in the base species stays 0 whatever its factor.
//!
//! [`skeleton_parameters`] probes which jitterable parameters change the skeleton of the
//! base species at all: colours and leaf sizes do not, nor do the self-organization
//! parameters of a parametric species, nor parameters that are 0.

use arbrume_core::species::{JITTERABLE, Species};
use arbrume_gen::GenParams;
use arbrume_grow::{Surroundings, grow_in};
use serde::{Deserialize, Serialize};

use crate::FitError;
use crate::skeleton::TreeSkeleton;

/// Factors by which [`skeleton_parameters`] probes a parameter.
pub const PROBE_FACTORS: [f32; 2] = [0.8, 1.25];

/// One coordinate of the search.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Parameter {
    /// Age in growth cycles.
    Age,
    /// Overall scale.
    Scale,
    /// Factor on a jitterable parameter, by its dotted name.
    Factor(String),
}

impl Parameter {
    /// Parses `age`, `scale` or a jitterable parameter name.
    ///
    /// # Errors
    ///
    /// Returns [`FitError::Invalid`] for any other name.
    pub fn parse(name: &str) -> Result<Self, FitError> {
        match name {
            "age" => Ok(Self::Age),
            "scale" => Ok(Self::Scale),
            other if JITTERABLE.contains(&other) => Ok(Self::Factor(other.to_owned())),
            other => Err(FitError::invalid(format!(
                "`{other}` is not `age`, `scale` or a jitterable parameter ({})",
                JITTERABLE.join(", ")
            ))),
        }
    }

    /// The name of the parameter.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Age => "age",
            Self::Scale => "scale",
            Self::Factor(name) => name,
        }
    }
}

/// A point of the search space, decoded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Decoded {
    /// Age in growth cycles.
    pub age: u16,
    /// Overall scale.
    pub scale: f32,
    /// Factor of each fitted jitterable parameter.
    pub factors: Vec<(String, f32)>,
}

/// The coordinates of a fit, their bounds and the reference age.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Space {
    /// The coordinates.
    pub parameters: Vec<Parameter>,
    /// Lower bound of each coordinate.
    pub lower: Vec<f64>,
    /// Upper bound of each coordinate.
    pub upper: Vec<f64>,
    /// Start point of the search.
    pub start: Vec<f64>,
    /// Age at coordinate 0 (and when the age is not fitted).
    pub base_age: f64,
    /// Scale when the scale is not fitted.
    pub base_scale: f64,
}

impl Space {
    /// `x` clamped to the bounds.
    #[must_use]
    pub fn clamp(&self, x: &[f64]) -> Vec<f64> {
        x.iter()
            .zip(self.lower.iter().zip(&self.upper))
            .map(|(v, (lo, hi))| v.clamp(*lo, *hi))
            .collect()
    }

    /// Squared distance from `x` to the box of the bounds.
    #[must_use]
    pub fn outside(&self, x: &[f64]) -> f64 {
        x.iter()
            .zip(self.clamp(x))
            .map(|(v, c)| (v - c) * (v - c))
            .sum()
    }

    /// The age, scale and factors at `x`, clamped to the bounds.
    #[must_use]
    pub fn decode(&self, x: &[f64]) -> Decoded {
        let x = self.clamp(x);
        let mut decoded = Decoded {
            age: self.base_age.round().clamp(1.0, f64::from(u16::MAX)) as u16,
            scale: self.base_scale as f32,
            factors: Vec::new(),
        };
        for (parameter, value) in self.parameters.iter().zip(&x) {
            match parameter {
                Parameter::Age => {
                    let age = (self.base_age * value.exp()).round();
                    decoded.age = age.clamp(1.0, f64::from(u16::MAX)) as u16;
                }
                Parameter::Scale => decoded.scale = value.exp() as f32,
                Parameter::Factor(name) => decoded.factors.push((name.clone(), value.exp() as f32)),
            }
        }
        decoded
    }
}

impl Decoded {
    /// `base` with the factors applied.
    ///
    /// # Errors
    ///
    /// Returns [`FitError::Invalid`] when a name is not jitterable or the result is not a
    /// valid species.
    pub fn species(&self, base: &Species) -> Result<Species, FitError> {
        let mut species = base.clone();
        for (name, factor) in &self.factors {
            species = species
                .with_factor(name, *factor)
                .ok_or_else(|| FitError::invalid(format!("cannot scale `{name}`")))?;
        }
        species
            .validate()
            .map_err(|e| FitError::invalid(e.to_string()))?;
        Ok(species)
    }

    /// The generation parameters of plant seed `seed`.
    #[must_use]
    pub fn params(&self, seed: u64) -> GenParams {
        GenParams::new(seed, self.age).with_scale(self.scale)
    }
}

/// The skeleton of `species` grown with `params` on open ground.
#[must_use]
pub fn grow_skeleton(species: &Species, params: &GenParams) -> TreeSkeleton {
    let graph = grow_in(species, &params.grow_params(), &Surroundings::none());
    TreeSkeleton::from_graph(&graph)
}

/// Splits the jitterable parameters into those whose factors [`PROBE_FACTORS`] change
/// the skeleton of `base` grown with `params`, and the others.
#[must_use]
pub fn skeleton_parameters(base: &Species, params: &GenParams) -> (Vec<String>, Vec<String>) {
    let reference = grow_skeleton(base, params);
    let (mut kept, mut dropped) = (Vec::new(), Vec::new());
    for name in JITTERABLE {
        let changes = PROBE_FACTORS.iter().any(|&factor| {
            base.with_factor(name, factor)
                .filter(|s| s.validate().is_ok())
                .is_some_and(|s| grow_skeleton(&s, params) != reference)
        });
        if changes {
            kept.push((*name).to_owned());
        } else {
            dropped.push((*name).to_owned());
        }
    }
    (kept, dropped)
}

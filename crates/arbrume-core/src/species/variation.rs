//! Variation layer: how individuals of a species differ from one another.
//!
//! The other layers describe one typical plant. This layer says how far a random
//! individual may stray from it, so that a generator can draw varied plants that stay
//! plausible: the range of ages worth generating, the range of overall scale, how often
//! the species should appear in a random mix, and a *jitter* for chosen parameters.
//!
//! ```toml
//! [variation]
//! age = [15, 60]
//! scale = [0.9, 1.1]
//! weight = 1.0
//!
//! [variation.jitter]
//! "growth.axis_length" = 0.15
//! "growth.branching_angle" = 0.1
//! "foliage.color" = 0.08
//! ```
//!
//! A jitter of `a` multiplies the parameter by a factor drawn uniformly in
//! `[1 - a, 1 + a)`, then clamps the result to the domain of the parameter. A parameter
//! that is a curve per order draws one factor per order, applied to every value of that
//! order's curve, so proportions between orders vary too; a colour draws one factor per
//! channel. The parameters that can be jittered are listed in [`JITTERABLE`].
//!
//! [`Species::varied`] applies the jitter. The draws depend only on the seed, the index
//! and the name of each parameter: adding a parameter to the table does not change the
//! draws of the others.

use super::{Species, SpeciesError, check_range};
use crate::curve::OrderCurve;
use crate::rng::{Rng, Stream};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Parameters accepted as keys of [`Variation::jitter`]. The position of a name in this
/// list selects its random stream, so names are only ever appended.
pub const JITTERABLE: &[&str] = &[
    "growth.axis_length",
    "growth.shoot_length",
    "growth.internode_length",
    "growth.branch_probability",
    "growth.branching_angle",
    "growth.gravitropic_setpoint",
    "growth.tropism_weight",
    "growth.curvature",
    "growth.gnarl",
    "growth.direction_variation",
    "growth.bare_base",
    "growth.fork_probability",
    "growth.apical_dominance",
    "growth.stems",
    "growth.stem_angle",
    "growth.stem_spread",
    "geometry.radius_ratio",
    "geometry.flare",
    "foliage.length",
    "foliage.width_ratio",
    "foliage.density",
    "foliage.inclination_deg",
    "foliage.color",
    "foliage.twig.length",
    "foliage.twig.bend_deg",
    "bark.color",
    "growth.self_organization.axis_bias",
    "growth.self_organization.resource_coefficient",
    "growth.self_organization.shedding_threshold",
];

/// How individuals of a species vary: the ranges a generator draws from, and the jitter
/// that [`Species::varied`] applies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Variation {
    /// Youngest and oldest age worth generating, in growth cycles, both included.
    pub age: [u16; 2],
    /// Smallest and largest overall scale factor.
    pub scale: [f32; 2],
    /// Relative frequency of the species in a random mix; 0 leaves it out.
    pub weight: f32,
    /// Relative half-width of the random factor of each jittered parameter, in `[0, 1)`,
    /// by parameter name (see [`JITTERABLE`]).
    pub jitter: BTreeMap<String, f32>,
}

impl Default for Variation {
    /// Ages 8 to 30, scale 0.9 to 1.1, weight 1, no jitter. Starting points, not
    /// measurements.
    fn default() -> Self {
        Self {
            age: [8, 30],
            scale: [0.9, 1.1],
            weight: 1.0,
            jitter: BTreeMap::new(),
        }
    }
}

impl Variation {
    pub(super) fn validate(&self) -> Result<(), SpeciesError> {
        let [young, old] = self.age;
        if young == 0 || young > old {
            return Err(SpeciesError::Invalid {
                field: "variation.age",
                reason: format!("[{young}, {old}] is not an increasing range of positive ages"),
            });
        }
        let [small, large] = self.scale;
        check_range("variation.scale", small, f32::MIN_POSITIVE, large)?;
        check_range("variation.scale", large, small, f32::MAX)?;
        check_range("variation.weight", self.weight, 0.0, f32::MAX)?;
        for (name, &amount) in &self.jitter {
            if !JITTERABLE.contains(&name.as_str()) {
                return Err(SpeciesError::Invalid {
                    field: "variation.jitter",
                    reason: format!("`{name}` cannot be jittered"),
                });
            }
            if !(0.0..1.0).contains(&amount) {
                return Err(SpeciesError::Invalid {
                    field: "variation.jitter",
                    reason: format!("`{name}` = {amount} is outside [0, 1)"),
                });
            }
        }
        Ok(())
    }
}

/// The random factors of one jittered parameter.
struct Factors {
    rng: Rng,
    amount: f32,
}

impl Factors {
    /// Next factor, uniform in `[1 - amount, 1 + amount)`.
    fn next(&mut self) -> f32 {
        self.amount.mul_add(self.rng.signed(), 1.0)
    }

    /// `value` times the next factor, clamped to `[lo, hi]`.
    fn scalar(&mut self, value: f32, lo: f32, hi: f32) -> f32 {
        (value * self.next()).clamp(lo, hi)
    }

    /// `curve` with the values of each order multiplied by its own factor and clamped
    /// to `[lo, hi]`.
    fn curve(&mut self, curve: &OrderCurve, lo: f32, hi: f32) -> OrderCurve {
        curve.scaled(|_| self.next(), lo, hi)
    }

    /// `color` with each channel multiplied by its own factor.
    fn color(&mut self, color: [f32; 3]) -> [f32; 3] {
        color.map(|channel| self.scalar(channel, 0.0, 1.0))
    }
}

impl Species {
    /// An individual of the species: a copy whose jittered parameters
    /// ([`Variation::jitter`]) are multiplied by random factors drawn from `seed` and
    /// `index`. The copy is valid when the species is; it keeps the variation layer.
    #[must_use]
    pub fn varied(&self, seed: u64, index: u32) -> Self {
        let mut individual = self.clone();
        for (name, &amount) in &self.variation.jitter {
            let Some(position) = JITTERABLE.iter().position(|known| known == name) else {
                continue;
            };
            let stream = Stream::custom(0x100 + position as u16);
            let mut factors = Factors {
                rng: Rng::new(seed, index, stream),
                amount,
            };
            let done = individual.jitter_growth(name, &mut factors)
                || individual.jitter_competition(name, &mut factors);
            if !done {
                individual.jitter_appearance(name, &mut factors);
            }
        }
        individual
    }

    /// Jitters the growth parameter `name`; `false` when it is not one.
    fn jitter_growth(&mut self, name: &str, factors: &mut Factors) -> bool {
        let max = f32::MAX;
        let tiny = f32::MIN_POSITIVE;
        let growth = &mut self.growth;
        let free = 1.0 - growth.environment_weight;
        let (curve, lo, hi) = match name {
            "growth.axis_length" => (&mut growth.axis_length, 0.0, max),
            "growth.shoot_length" => (&mut growth.shoot_length, tiny, max),
            "growth.internode_length" => (&mut growth.internode_length, tiny, max),
            "growth.branch_probability" => (&mut growth.branch_probability, 0.0, 1.0),
            "growth.branching_angle" => (&mut growth.branching_angle, 0.0, 180.0),
            "growth.gravitropic_setpoint" => (&mut growth.gravitropic_setpoint, 0.0, 180.0),
            "growth.tropism_weight" => (&mut growth.tropism_weight, 0.0, free),
            "growth.curvature" => (&mut growth.curvature, -max, max),
            "growth.gnarl" => (&mut growth.gnarl, 0.0, 180.0),
            "growth.direction_variation" => (&mut growth.direction_variation, 0.0, 180.0),
            "growth.bare_base" => (&mut growth.bare_base, 0.0, 1.0),
            "growth.fork_probability" => (&mut growth.fork_probability, 0.0, 1.0),
            "growth.apical_dominance" => {
                growth.apical_dominance = factors.scalar(growth.apical_dominance, 0.0, 1.0);
                return true;
            }
            "growth.stems" => {
                let stems = factors.scalar(f32::from(growth.stems), 1.0, 255.0);
                growth.stems = stems.round() as u8;
                return true;
            }
            "growth.stem_angle" => {
                growth.stem_angle = factors.scalar(growth.stem_angle, 0.0, 180.0);
                return true;
            }
            "growth.stem_spread" => {
                growth.stem_spread = factors.scalar(growth.stem_spread, 0.0, max);
                return true;
            }
            _ => return false,
        };
        *curve = factors.curve(curve, lo, hi);
        true
    }

    /// Jitters the self-organization parameter `name`; `false` when it is not one.
    fn jitter_competition(&mut self, name: &str, factors: &mut Factors) -> bool {
        let model = &mut self.growth.self_organization;
        match name {
            "growth.self_organization.axis_bias" => {
                model.axis_bias = factors.curve(&model.axis_bias, 0.0, 1.0);
            }
            "growth.self_organization.resource_coefficient" => {
                let alpha = model.resource_coefficient;
                model.resource_coefficient = factors.scalar(alpha, 0.0, f32::MAX);
            }
            "growth.self_organization.shedding_threshold" => {
                let threshold = model.shedding_threshold;
                model.shedding_threshold = factors.scalar(threshold, 0.0, f32::MAX);
            }
            _ => return false,
        }
        true
    }

    /// Jitters the geometry, foliage or bark parameter `name`.
    fn jitter_appearance(&mut self, name: &str, factors: &mut Factors) {
        let (max, tiny) = (f32::MAX, f32::MIN_POSITIVE);
        let (geometry, foliage) = (&mut self.geometry, &mut self.foliage);
        match name {
            "geometry.radius_ratio" => {
                geometry.radius_ratio = factors.scalar(geometry.radius_ratio, tiny, max);
            }
            "geometry.flare" => geometry.flare = factors.scalar(geometry.flare, 0.0, max),
            "foliage.length" => foliage.length = factors.scalar(foliage.length, tiny, max),
            "foliage.width_ratio" => {
                foliage.width_ratio = factors.scalar(foliage.width_ratio, tiny, max);
            }
            "foliage.density" => foliage.density = factors.scalar(foliage.density, tiny, 1.0),
            "foliage.inclination_deg" => {
                foliage.inclination_deg = factors.scalar(foliage.inclination_deg, 0.0, 180.0);
            }
            "foliage.color" => foliage.color = factors.color(foliage.color),
            "foliage.twig.length" => {
                foliage.twig.length = factors.scalar(foliage.twig.length, tiny, max);
            }
            "foliage.twig.bend_deg" => {
                foliage.twig.bend_deg = factors.scalar(foliage.twig.bend_deg, -180.0, 180.0);
            }
            "bark.color" => {
                let bark = &mut self.bark;
                let factor = factors.next();
                let shade = |color: [f32; 3]| color.map(|c| (c * factor).clamp(0.0, 1.0));
                bark.color = factors.color(shade(bark.color));
                bark.young_color = bark.young_color.map(shade);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{JITTERABLE, Variation};
    use crate::species::{Species, SpeciesError};

    fn jittered(amount: f32) -> Species {
        let mut species = Species::default();
        species.bark.young_color = Some([0.3, 0.2, 0.1]);
        for name in JITTERABLE {
            species.variation.jitter.insert((*name).to_owned(), amount);
        }
        species
    }

    #[test]
    fn every_jitterable_parameter_changes_and_stays_valid() {
        let species = jittered(0.3);
        species.validate().unwrap();
        for index in 0..50 {
            let individual = species.varied(7, index);
            individual.validate().unwrap();
            assert_ne!(individual.growth.axis_length, species.growth.axis_length);
            assert_ne!(individual.foliage.color, species.foliage.color);
            assert_ne!(individual.bark.color, species.bark.color);
            assert_ne!(individual.geometry.flare, species.geometry.flare);
            assert_eq!(individual.variation, species.variation);
        }
    }

    #[test]
    fn draws_depend_on_seed_index_and_name_only() {
        let species = jittered(0.2);
        assert_eq!(species.varied(1, 4), species.varied(1, 4));
        assert_ne!(species.varied(1, 4), species.varied(1, 5));
        assert_ne!(species.varied(1, 4), species.varied(2, 4));
        let mut fewer = species.clone();
        fewer.variation.jitter.remove("growth.axis_length");
        let (a, b) = (species.varied(3, 9), fewer.varied(3, 9));
        assert_eq!(a.growth.branching_angle, b.growth.branching_angle);
        assert_eq!(b.growth.axis_length, species.growth.axis_length);
        assert_eq!(Species::default().varied(3, 9), Species::default());
    }

    #[test]
    fn factors_stay_within_the_amount_and_differ_between_orders() {
        let species = jittered(0.1);
        let lengths = |s: &Species| [0, 1, 2].map(|order| s.growth.axis_length.eval(order, 0.5));
        let base = lengths(&species);
        let mut spread = 0.0_f32;
        for index in 0..100 {
            let varied = lengths(&species.varied(11, index));
            let ratios = [0, 1, 2].map(|order| varied[order] / base[order]);
            assert!(ratios.iter().all(|r| (0.899..1.101).contains(r)));
            spread = spread.max((ratios[0] - ratios[1]).abs());
        }
        assert!(spread > 0.05);
    }

    #[test]
    fn invalid_variation_is_rejected() {
        let invalid = [
            "age = [0, 5]",
            "age = [9, 5]",
            "scale = [1.2, 0.8]",
            "scale = [0.0, 1.0]",
            "weight = -1.0",
            "jitter = { \"growth.max_order\" = 0.1 }",
            "jitter = { \"growth.axis_length\" = 1.0 }",
        ];
        for text in invalid {
            let result = Species::from_toml_str(&format!("[variation]\n{text}"));
            assert!(
                matches!(result, Err(SpeciesError::Invalid { field, .. }) if field.starts_with("variation")),
                "{text}"
            );
        }
        assert_eq!(Variation::default().age, [8, 30]);
    }
}

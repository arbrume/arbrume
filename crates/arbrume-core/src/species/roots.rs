//! Roots layer: the root system grown below the collar.
//!
//! The table `[roots]` is optional. A species without it has no roots and grows exactly
//! as before; a species with it grows a root system in the same plant graph, with the
//! same growth cycle as its shoot (`arbrume-grow`, module `roots`). The vocabulary is
//! that of plant architecture (Barthélémy and Caraglio 2007) and of the root types named
//! by Li et al. (2023): a **primary** root leaves the collar (order 0 of the root system),
//! its laterals are of order 1, and so on; a **sinker** is a lateral that turns straight
//! down; **fine roots** are the short axes of the highest order.
//!
//! Every parameter mirrors one of `[growth]` with the same meaning, read for the root
//! system, except that set-point angles are counted from the *downward* vertical: 0 is
//! straight down, 90 horizontal. Lengths are in meters at scale 1, multiplied by the
//! scale of the plant like those of the shoot.
//!
//! `habit` chooses the starting values of every other field, which a file only needs to
//! list where it differs:
//!
//! | Habit | Form |
//! |---|---|
//! | `tap` | a vertical tap root among long, near-horizontal primaries |
//! | `heart` | several oblique primaries that steepen with depth, a rounded mass |
//! | `plate` | many long near-horizontal primaries just under the surface, with sinkers |
//! | `fibrous` | a dense tuft of many thin primaries of similar length |
//!
//! These values are starting points chosen by eye, not measurements of any species.

use super::{GrowthMode, SpeciesError, check_curve, check_range};
use crate::curve::{Curve, OrderCurve};
use serde::{Deserialize, Deserializer, Serialize};

/// Most primary roots at the collar.
pub const MAX_PRIMARY_ROOTS: u8 = 64;
/// Highest branching order of a root system (0 being the primaries).
pub const MAX_ROOT_ORDER: u8 = 4;

/// Overall form of a root system, which sets the starting values of `[roots]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RootHabit {
    /// One vertical tap root with near-horizontal laterals.
    Tap,
    /// Oblique primaries in a rounded mass (heart or oblique root system).
    #[default]
    Heart,
    /// Shallow horizontal primaries with sinkers (plate or flat root system).
    Plate,
    /// Many thin primaries of similar length.
    Fibrous,
}

impl RootHabit {
    /// Every habit, in the order of the table of the [module documentation](self).
    pub const ALL: [Self; 4] = [Self::Tap, Self::Heart, Self::Plate, Self::Fibrous];

    /// Lower-case name, as written in species files.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Tap => "tap",
            Self::Heart => "heart",
            Self::Plate => "plate",
            Self::Fibrous => "fibrous",
        }
    }

    /// The habit named `name`, `None` for an unknown name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|habit| habit.name() == name)
    }
}

/// Parameters of the self-organizing root mode: competition of root tips for the
/// resources of the soil. Lengths are in internode units, the `internode_length` of
/// order 1 of the roots at the base of an axis, as in `[growth.self_organization]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RootSelfOrganization {
    /// Resource `v` given to a primary root per unit of the uptake gathered by all it
    /// bears (`alpha` of the shoot model).
    pub resource_coefficient: f32,
    /// Share `lambda` kept by the continuation of an axis at each branching point
    /// (extended Borchert-Honda).
    pub axis_bias: f32,
    /// Radius around a root tip in which the soil resource is taken up and removed.
    pub occupancy_radius: f32,
    /// Length of the cone in which a root tip senses the resource.
    pub perception_distance: f32,
    /// Full angle of that cone, in degrees.
    pub perception_angle_deg: f32,
    /// Resource points per cubic internode unit of homogeneous soil; a patch of richness
    /// `k` holds `1 + k` times as many.
    pub marker_density: f32,
    /// Largest number of resource points.
    pub marker_limit: u32,
}

impl Default for RootSelfOrganization {
    fn default() -> Self {
        Self {
            resource_coefficient: 2.0,
            axis_bias: 0.5,
            occupancy_radius: 1.5,
            perception_distance: 5.0,
            perception_angle_deg: 120.0,
            marker_density: 0.15,
            marker_limit: 200_000,
        }
    }
}

impl RootSelfOrganization {
    fn validate(&self) -> Result<(), SpeciesError> {
        let max = f32::MAX;
        let model = "roots.self_organization";
        let tiny = f32::MIN_POSITIVE;
        let coefficient = self.resource_coefficient;
        check_range(
            "roots.self_organization.resource_coefficient",
            coefficient,
            0.0,
            max,
        )?;
        check_range(
            "roots.self_organization.axis_bias",
            self.axis_bias,
            0.0,
            1.0,
        )?;
        let occupancy = self.occupancy_radius;
        check_range(
            "roots.self_organization.occupancy_radius",
            occupancy,
            0.0,
            max,
        )?;
        let distance = self.perception_distance;
        check_range(
            "roots.self_organization.perception_distance",
            distance,
            tiny,
            max,
        )?;
        let angle = self.perception_angle_deg;
        check_range(
            "roots.self_organization.perception_angle_deg",
            angle,
            1.0,
            360.0,
        )?;
        let density = self.marker_density;
        check_range("roots.self_organization.marker_density", density, tiny, max)?;
        if self.marker_limit == 0 {
            return Err(SpeciesError::Invalid {
                field: "roots.self_organization.marker_limit",
                reason: format!("{model} needs at least one resource point"),
            });
        }
        Ok(())
    }
}

/// Root system of a species. See the [module documentation](self).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Roots {
    /// Overall form; sets the starting value of every other field.
    pub habit: RootHabit,
    /// `parametric`: roots follow the curves below. `self_organizing`: root tips compete
    /// for the resources of the soil, and the curves set limits and biases.
    pub mode: GrowthMode,
    /// Number of primary roots leaving the collar (1 to 64).
    pub primaries: u8,
    /// Highest order of the root system, 0 for primaries alone (at most 4).
    pub max_order: u8,
    /// Length of a full-grown root, curve per order along the parent.
    pub axis_length: OrderCurve,
    /// Extension of a root per cycle, curve per order along the axis.
    pub shoot_length: OrderCurve,
    /// Internode length, curve per order along the axis.
    pub internode_length: OrderCurve,
    /// Half-width of the random variation of `axis_length`, as a fraction.
    pub length_variation: OrderCurve,
    /// Probability that a node bears a lateral root, curve per order of the bearing root
    /// along it.
    pub branch_probability: OrderCurve,
    /// Angle between a lateral root and its parent, curve per order of the lateral along
    /// the parent.
    pub branching_angle: OrderCurve,
    /// Angle of the tropism direction from the downward vertical, curve per order along
    /// the axis: 0 straight down, 90 horizontal. The first internode of a primary root
    /// takes the value at its base.
    pub gravitropic_setpoint: OrderCurve,
    /// Weight of the tropism direction per internode, curve per order along the axis.
    pub tropism_weight: OrderCurve,
    /// Half-width of a random deviation at each internode, in degrees.
    pub direction_variation: OrderCurve,
    /// Half-width of a random bend drawn once per cycle, in degrees.
    pub gnarl: OrderCurve,
    /// The first primary root is a tap root: it leaves the collar straight down and its
    /// tropism points straight down, whatever `gravitropic_setpoint` says.
    pub tap_root: bool,
    /// Primary roots keep their laterals in the horizontal plane.
    pub plagiotropic: bool,
    /// Probability that a lateral root becomes a sinker, whose tropism points straight
    /// down whatever its order.
    pub sinker_probability: f32,
    /// Weight of the direction towards the resources of the soil (`gamma`).
    pub environment_weight: f32,
    /// Exponent `n` of the pipe model of the roots: `r^n = sum of r_child^n` from the
    /// root tips. At the collar the trunk base takes `r^n = r_shoot^n + sum of
    /// r_primary^n`.
    pub pipe_exponent: f32,
    /// Radius of a root tip, in meters.
    pub tip_radius: f32,
    /// How far the pipes of the roots are matched to those of the shoot at the collar, in
    /// `[0, 1]`. The root graph has far fewer tips than a real root system, whose fine
    /// roots it leaves out, so its pipe radii are too thin; with a balance `b` every root
    /// radius of a stem is multiplied by `k^b`, `k` the factor that makes the pipe sum of
    /// its primaries equal the pipe sum of the shoot at the collar (pipe continuity
    /// through the collar). 0 keeps the radii of the root tips.
    pub pipe_balance: f32,
    /// Widening of a primary root near the collar, as a fraction of its radius.
    pub flare: f32,
    /// Length of a primary root over which `flare` fades out, in meters.
    pub flare_length: f32,
    /// Part of the primaries above the ground near the collar, in `[0, 1]`: 0 buries
    /// every root, 0.5 shows the upper half of the primaries at the collar, 1 lays them
    /// on the ground. Exposed roots follow the surface and sink back as `exposed_length`
    /// is reached.
    pub exposed: f32,
    /// Length of a primary root over which its exposure fades, in meters.
    pub exposed_length: f32,
    /// Parameters of the self-organizing mode.
    pub self_organization: RootSelfOrganization,
}

impl Default for Roots {
    /// The heart habit.
    fn default() -> Self {
        Self::of(RootHabit::Heart)
    }
}

/// A linear curve through `points`.
fn line(points: &[[f32; 2]]) -> Curve {
    Curve::linear(points).unwrap_or_else(|_| Curve::constant(points[0][1]))
}

/// One curve per order.
fn orders(curves: Vec<Curve>) -> OrderCurve {
    OrderCurve::by_order(curves).unwrap_or_default()
}

/// One constant per order.
fn per_order(values: &[f32]) -> OrderCurve {
    OrderCurve::constants(values).unwrap_or_default()
}

impl Roots {
    /// The starting values of `habit`.
    #[must_use]
    pub fn of(habit: RootHabit) -> Self {
        let base = Self::common(habit);
        match habit {
            RootHabit::Tap => Self::tap(base),
            RootHabit::Heart => Self::heart(base),
            RootHabit::Plate => Self::plate(base),
            RootHabit::Fibrous => Self::fibrous(base),
        }
    }

    /// The values shared by the habits.
    fn common(habit: RootHabit) -> Self {
        Self {
            habit,
            mode: GrowthMode::Parametric,
            primaries: 7,
            max_order: 2,
            axis_length: per_order(&[5.0, 1.8, 0.45]),
            shoot_length: per_order(&[0.3, 0.2, 0.12]),
            internode_length: per_order(&[0.15, 0.1, 0.06]),
            length_variation: per_order(&[0.15, 0.3, 0.3]),
            branch_probability: per_order(&[0.6, 0.45, 0.0]),
            branching_angle: per_order(&[60.0, 70.0, 65.0]),
            gravitropic_setpoint: per_order(&[45.0, 75.0, 60.0]),
            tropism_weight: per_order(&[0.15, 0.1, 0.05]),
            direction_variation: per_order(&[9.0, 13.0, 18.0]),
            gnarl: per_order(&[22.0, 25.0, 25.0]),
            tap_root: false,
            plagiotropic: false,
            sinker_probability: 0.0,
            environment_weight: 0.3,
            pipe_exponent: 2.0,
            tip_radius: 0.005,
            pipe_balance: 1.0,
            flare: 0.6,
            flare_length: 1.2,
            exposed: 0.0,
            exposed_length: 1.5,
            self_organization: RootSelfOrganization::default(),
        }
    }

    /// A vertical tap root among near-horizontal primaries; the laterals of the tap are
    /// longest near the collar.
    fn tap(base: Self) -> Self {
        Self {
            primaries: 6,
            tap_root: true,
            axis_length: orders(vec![
                Curve::constant(3.8),
                line(&[[0.0, 2.4], [0.4, 1.4], [1.0, 0.4]]),
                Curve::constant(0.4),
            ]),
            shoot_length: per_order(&[0.3, 0.2, 0.12]),
            branch_probability: per_order(&[0.6, 0.45, 0.0]),
            branching_angle: per_order(&[60.0, 75.0, 65.0]),
            gravitropic_setpoint: orders(vec![
                line(&[[0.0, 78.0], [1.0, 55.0]]),
                Curve::constant(72.0),
                Curve::constant(60.0),
            ]),
            tropism_weight: per_order(&[0.2, 0.1, 0.05]),
            ..base
        }
    }

    /// Oblique primaries that steepen with depth.
    fn heart(base: Self) -> Self {
        Self {
            gravitropic_setpoint: orders(vec![
                line(&[[0.0, 64.0], [1.0, 35.0]]),
                Curve::constant(70.0),
                Curve::constant(55.0),
            ]),
            ..base
        }
    }

    /// Long horizontal primaries near the surface, with sinkers.
    fn plate(base: Self) -> Self {
        Self {
            primaries: 9,
            axis_length: orders(vec![
                Curve::constant(6.5),
                line(&[[0.0, 2.0], [1.0, 0.6]]),
                Curve::constant(0.35),
            ]),
            shoot_length: per_order(&[0.4, 0.2, 0.12]),
            branch_probability: per_order(&[0.55, 0.4, 0.0]),
            branching_angle: per_order(&[60.0, 75.0, 65.0]),
            gravitropic_setpoint: per_order(&[86.0, 88.0, 70.0]),
            tropism_weight: per_order(&[0.35, 0.2, 0.05]),
            plagiotropic: true,
            sinker_probability: 0.22,
            ..base
        }
    }

    /// Many thin primaries of similar length, fanning out then turning down.
    fn fibrous(base: Self) -> Self {
        Self {
            primaries: 24,
            max_order: 1,
            axis_length: per_order(&[1.6, 0.35]),
            shoot_length: per_order(&[0.2, 0.1]),
            internode_length: per_order(&[0.12, 0.06]),
            length_variation: per_order(&[0.25, 0.3]),
            branch_probability: per_order(&[0.55, 0.0]),
            branching_angle: per_order(&[60.0, 65.0]),
            gravitropic_setpoint: orders(vec![
                line(&[[0.0, 70.0], [1.0, 15.0]]),
                Curve::constant(55.0),
            ]),
            tropism_weight: per_order(&[0.12, 0.05]),
            direction_variation: per_order(&[12.0, 18.0]),
            gnarl: per_order(&[16.0, 20.0]),
            tip_radius: 0.003,
            flare: 0.3,
            flare_length: 0.4,
            ..base
        }
    }

    /// Checks the domain of every parameter.
    ///
    /// # Errors
    ///
    /// Returns the first [`SpeciesError::Invalid`] parameter found.
    pub fn validate(&self) -> Result<(), SpeciesError> {
        let (tiny, max) = (f32::MIN_POSITIVE, f32::MAX);
        let primaries = f32::from(self.primaries);
        check_range(
            "roots.primaries",
            primaries,
            1.0,
            f32::from(MAX_PRIMARY_ROOTS),
        )?;
        let order = f32::from(self.max_order);
        check_range("roots.max_order", order, 0.0, f32::from(MAX_ROOT_ORDER))?;
        check_curve("roots.axis_length", &self.axis_length, 0.0, max)?;
        check_curve("roots.shoot_length", &self.shoot_length, tiny, max)?;
        check_curve("roots.internode_length", &self.internode_length, tiny, max)?;
        check_curve("roots.length_variation", &self.length_variation, 0.0, 1.0)?;
        let probability = &self.branch_probability;
        check_curve("roots.branch_probability", probability, 0.0, 1.0)?;
        check_curve("roots.branching_angle", &self.branching_angle, 0.0, 180.0)?;
        let setpoint = &self.gravitropic_setpoint;
        check_curve("roots.gravitropic_setpoint", setpoint, 0.0, 180.0)?;
        let weight = self.environment_weight;
        check_range("roots.environment_weight", weight, 0.0, 1.0)?;
        let free = 1.0 - weight;
        check_curve("roots.tropism_weight", &self.tropism_weight, 0.0, free)?;
        let variation = &self.direction_variation;
        check_curve("roots.direction_variation", variation, 0.0, 180.0)?;
        check_curve("roots.gnarl", &self.gnarl, 0.0, 180.0)?;
        self.validate_shape()?;
        self.self_organization.validate()
    }

    /// Checks the radii, the flare and the exposure.
    fn validate_shape(&self) -> Result<(), SpeciesError> {
        let (tiny, max) = (f32::MIN_POSITIVE, f32::MAX);
        let sinkers = self.sinker_probability;
        check_range("roots.sinker_probability", sinkers, 0.0, 1.0)?;
        check_range("roots.pipe_exponent", self.pipe_exponent, 1.0, 4.0)?;
        check_range("roots.tip_radius", self.tip_radius, tiny, max)?;
        check_range("roots.pipe_balance", self.pipe_balance, 0.0, 1.0)?;
        check_range("roots.flare", self.flare, 0.0, 4.0)?;
        check_range("roots.flare_length", self.flare_length, 0.0, max)?;
        check_range("roots.exposed", self.exposed, 0.0, 1.0)?;
        check_range("roots.exposed_length", self.exposed_length, 0.0, max)
    }
}

/// Reads the optional `[roots]` table: the values of its `habit` (heart when absent),
/// then the fields the table lists.
///
/// # Errors
///
/// Fails on an unknown habit, an unknown key or a value of the wrong type.
pub(super) fn deserialize_layer<'de, D>(deserializer: D) -> Result<Option<Roots>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;
    let Some(table) = Option::<toml::Table>::deserialize(deserializer)? else {
        return Ok(None);
    };
    let habit = match table.get("habit") {
        None => RootHabit::default(),
        Some(value) => {
            let name = value.as_str().unwrap_or_default();
            RootHabit::from_name(name)
                .ok_or_else(|| D::Error::custom(format!("unknown root habit `{value}`")))?
        }
    };
    let mut merged = toml::Table::try_from(Roots::of(habit)).map_err(D::Error::custom)?;
    for (key, value) in table {
        let nested = key == "self_organization";
        match (merged.get_mut(&key), value) {
            (Some(toml::Value::Table(inner)), toml::Value::Table(given)) if nested => {
                inner.extend(given);
            }
            (_, value) => {
                merged.insert(key, value);
            }
        }
    }
    merged.try_into().map(Some).map_err(D::Error::custom)
}

#[cfg(test)]
mod tests {
    use super::{RootHabit, Roots};
    use crate::species::{GrowthMode, Species, SpeciesError};

    #[test]
    fn habits_are_valid_and_round_trip() {
        for habit in RootHabit::ALL {
            let species = Species {
                roots: Some(Roots::of(habit)),
                ..Species::default()
            };
            species.validate().unwrap();
            let text = species.to_toml_string().unwrap();
            assert_eq!(Species::from_toml_str(&text).unwrap(), species, "{habit:?}");
            assert_eq!(RootHabit::from_name(habit.name()), Some(habit));
        }
    }

    #[test]
    fn a_table_overrides_the_values_of_its_habit() {
        let species = Species::from_toml_str(
            "[roots]\nhabit = \"plate\"\nprimaries = 5\n\
             [roots.self_organization]\naxis_bias = 0.45",
        )
        .unwrap();
        let roots = species.roots.unwrap();
        let plate = Roots::of(RootHabit::Plate);
        assert_eq!(roots.primaries, 5);
        assert_eq!(roots.sinker_probability, plate.sinker_probability);
        assert_eq!(roots.axis_length, plate.axis_length);
        assert!((roots.self_organization.axis_bias - 0.45).abs() < 1e-6);
        assert_eq!(roots.mode, GrowthMode::Parametric);
    }

    #[test]
    fn no_table_means_no_roots_and_nothing_written() {
        let species = Species::from_toml_str("").unwrap();
        assert!(species.roots.is_none());
        assert!(!species.to_toml_string().unwrap().contains("roots"));
        let empty = Species::from_toml_str("[roots]").unwrap();
        assert_eq!(empty.roots, Some(Roots::default()));
    }

    #[test]
    fn bad_values_and_keys_are_rejected() {
        let invalid = [
            ("[roots]\nprimaries = 0", "roots.primaries"),
            ("[roots]\nmax_order = 9", "roots.max_order"),
            ("[roots]\nexposed = 1.5", "roots.exposed"),
            ("[roots]\npipe_exponent = 0.5", "roots.pipe_exponent"),
            (
                "[roots.self_organization]\nmarker_density = 0.0",
                "roots.self_organization.marker_density",
            ),
        ];
        for (text, expected) in invalid {
            match Species::from_toml_str(text) {
                Err(SpeciesError::Invalid { field, .. }) => assert_eq!(field, expected),
                other => panic!("{text}: expected `{expected}`, got {other:?}"),
            }
        }
        for text in ["[roots]\nhabit = \"taproot\"", "[roots]\nprimarys = 3"] {
            let result = Species::from_toml_str(text);
            assert!(matches!(result, Err(SpeciesError::Parse(_))), "{text}");
        }
    }
}

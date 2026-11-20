//! Growth layer: the quantities that drive the growth cycle.
//!
//! Following Prusinkiewicz et al. (2001), the parameters of the parametric mode are
//! curves over the branching order and a relative position in `[0, 1]`. Each field says
//! which position it uses:
//!
//! * *along the parent*: where the axis is attached on its parent, 0 at the parent's
//!   base and 1 at its tip. For the trunk (order 0) the position is 0.
//! * *along the axis*: where the metamer lies on its own axis, 0 at its base and 1 at
//!   its tip.
//!
//! Coverage follows the parameter sets of Weber and Penn (1995) and Stava et al. (2014):
//! axis and internode lengths, extension per cycle, insertion angle and its variation,
//! branching density, tiers and whorls, direction noise, curvature and tropism, forks,
//! relay axes, basal stems and bending under weight. [`Growth::mode`] selects the
//! parametric mode or the self-organizing mode of Palubicki et al. (2009), whose
//! parameters are in [`SelfOrganization`].
//!
//! A *growth cycle* is one step of the growth engine: every growing axis extends once.
//! With rhythmic growth the metamers an axis gains in one cycle form a growth unit.
//!
//! All defaults are starting points for tuning, not measured values.

use super::{SpeciesError, check_curve, check_range};
use crate::curve::{Curve, OrderCurve};
use serde::{Deserialize, Serialize};

/// How a species grows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrowthMode {
    /// Every axis follows the gradient curves of the species; nothing is sensed.
    #[default]
    Parametric,
    /// Buds compete for light and space ([`SelfOrganization`]); the gradient curves set
    /// limits and biases.
    SelfOrganizing,
}

/// What the buds of a self-organizing plant sense (Palubicki et al. 2009, section 4.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sensing {
    /// Shadow propagation: a continuous exposure to light.
    #[default]
    Shadow,
    /// Space colonization: markers inside the crown envelope, a bud has space or not.
    Markers,
    /// Both: the exposure of the shadow model for the buds that have markers, the
    /// direction of the markers (the combination of Longay et al. 2012).
    Both,
}

/// How the resource is shared between buds (Palubicki et al. 2009, section 4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Allocation {
    /// Extended Borchert-Honda model: one split per branching point, biased by
    /// `axis_bias`.
    #[default]
    BorchertHonda,
    /// Priority model: the branches of an axis are ranked by the light they receive per
    /// bud and weighted by their rank.
    Priority,
}

/// Parameters of self-organizing growth (Palubicki et al. 2009), in which buds compete
/// for light or space. Used when [`Growth::mode`] is [`GrowthMode::SelfOrganizing`].
///
/// Distances are in *internode units*, as in the paper: one unit is the
/// `internode_length` of order 1 at the base of the axis ([`Growth::internode_unit`]).
///
/// The paper gives typical values for the occupancy radius, the perception volume, the
/// depth of the shadow pyramid, `alpha`, `lambda` and the priority weights. It gives
/// none for the shadow coefficients `a`, `b` and `C` nor for the shedding threshold:
/// those defaults are starting points only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SelfOrganization {
    /// What the buds sense.
    pub sensing: Sensing,
    /// How the resource is shared between buds.
    pub allocation: Allocation,
    /// Bias `lambda` of the extended Borchert-Honda split between the continuation of an
    /// axis and a lateral: `v_main = v * lambda * Q_main / (lambda * Q_main +
    /// (1 - lambda) * Q_lateral)`. In `[0, 1]`; 0.5 is neutral, larger favours the main
    /// axis. The order is that of the bearing axis, the position that of the branching
    /// point along it, so the bias can fade as an axis nears its full length.
    pub axis_bias: OrderCurve,
    /// Coefficient `alpha` turning the exposure gathered at the base into resource:
    /// `v_base = alpha * Q_base`.
    pub resource_coefficient: f32,
    /// Exponent applied to the exposure of a bud before it is summed (the light
    /// sensitivity of Longay et al. 2012): above 1 penalizes shaded buds more.
    pub light_exponent: f32,
    /// Gravimorphism `d` of Longay et al. (2012, eq. 1): the exposure of a bud is
    /// multiplied by `b = cos² tau + d sin² tau`, `tau` the angle between the vertical and
    /// the internode bearing the bud, after it is raised to `light_exponent`. In
    /// `[0, 1]`: buds on horizontal axes get `b = d`, buds on vertical axes `b = 1`. The
    /// default 1 gives `b = 1` everywhere.
    pub gravimorphism: f32,
    /// Edge of a voxel of the shadow grid, in internode units.
    pub shadow_cell: f32,
    /// Shadow `a` a metamer adds to its own voxel; `a * b^-q` reaches the voxels `q`
    /// layers below.
    pub shadow_strength: f32,
    /// Base `b` of the decay of the shadow with depth, above 1.
    pub shadow_decay: f32,
    /// Number `q_max` of layers below a metamer that its shadow reaches.
    pub shadow_depth: u8,
    /// Exposure `C` of a bud in full light: `Q = max(C - s + a, 0)`.
    pub full_exposure: f32,
    /// Radius of the zone around a bud in which markers are removed, in internode units.
    pub occupancy_radius: f32,
    /// Reach of the perception cone of a bud, in internode units.
    pub perception_distance: f32,
    /// Full opening angle of the perception cone, in degrees.
    pub perception_angle_deg: f32,
    /// Number of markers per cubic internode unit of the envelope.
    pub marker_density: f32,
    /// Largest number of markers; the density is lowered to fit.
    pub marker_limit: u32,
    /// Weight of the first branch of the priority list (`w_max`).
    pub priority_max: f32,
    /// Weight of the branches past the knee of the priority list (`w_min`).
    pub priority_min: f32,
    /// Fraction of the priority list over which the weight falls linearly from
    /// `priority_max` to `priority_min`, in `(0, 1]`.
    pub priority_knee: f32,
    /// Apical control of the priority model: where it is at least 0.5 the terminal bud
    /// of an axis heads its priority list whatever its exposure. The order is that of
    /// the axis, the position that of its tip, so control can be lost with age.
    pub apical_control: OrderCurve,
    /// A branch is shed when the exposure gathered by its buds divided by its number of
    /// internodes falls below this value. 0 disables shedding. Meant for the shadow
    /// model: with markers alone the exposure is 0 or 1.
    pub shedding_threshold: f32,
    /// Number of growth cycles a lateral bud stays able to flush; 0 for ever. A limit
    /// bounds the number of dormant buds a large plant carries.
    pub bud_lifespan: u16,
}

impl Default for SelfOrganization {
    /// The typical values quoted by Palubicki et al. (2009) where the paper gives one:
    /// occupancy 2 and perception 4 internodes (of 4 to 6) over about 90 degrees, a
    /// shadow 6 layers deep (of 4 to 8), `alpha` 2 and the neutral `lambda` 0.5 of its
    /// figure 7, the weights 1 and 0.006 and the knee 0.5 of its figure 9. The other
    /// values are starting points; shedding is off and buds live for ever.
    fn default() -> Self {
        Self {
            sensing: Sensing::Shadow,
            allocation: Allocation::BorchertHonda,
            axis_bias: OrderCurve::constant(0.5),
            resource_coefficient: 2.0,
            light_exponent: 1.0,
            gravimorphism: 1.0,
            shadow_cell: 1.0,
            shadow_strength: 0.2,
            shadow_decay: 2.0,
            shadow_depth: 6,
            full_exposure: 1.0,
            occupancy_radius: 2.0,
            perception_distance: 4.0,
            perception_angle_deg: 90.0,
            marker_density: 0.5,
            marker_limit: 200_000,
            priority_max: 1.0,
            priority_min: 0.006,
            priority_knee: 0.5,
            apical_control: OrderCurve::constant(1.0),
            shedding_threshold: 0.0,
            bud_lifespan: 0,
        }
    }
}

/// Growth parameters of a species.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Growth {
    /// Parametric or self-organizing growth.
    pub mode: GrowthMode,
    /// Weight `gamma` of the direction `E` preferred by the environment in the heading
    /// equation of `tropism_weight`. Ignored when there is no environment.
    pub environment_weight: f32,
    /// Length in meters of a full-grown axis, over its attachment position along the
    /// parent. Order 0 gives the height of the trunk. Its curve for order 1 draws the
    /// silhouette of the crown.
    pub axis_length: OrderCurve,
    /// Internode length in meters, along the axis. Nodes are placed where the integral
    /// of its inverse reaches whole numbers (Prusinkiewicz et al. 2001, eq. 11).
    pub internode_length: OrderCurve,
    /// Probability in `[0, 1]` that a lateral bud develops into an axis, over the bud's
    /// position along its bearing axis; the order is that of the bearing axis. A curve
    /// that is 0 near the base leaves a bare trunk.
    pub branch_probability: OrderCurve,
    /// Insertion angle in degrees between a new lateral axis and its parent, over the
    /// attachment position along the parent; the order is that of the lateral.
    pub branching_angle: OrderCurve,
    /// Half-width in degrees of the uniform random variation added to
    /// `branching_angle`.
    pub branching_angle_variation: OrderCurve,
    /// Half-width in degrees of the uniform random deviation of the growth direction at
    /// each new metamer, along the axis.
    pub direction_variation: OrderCurve,
    /// Direction `T` that tropism pulls towards, as an angle in degrees from the upward
    /// vertical in the vertical plane of the axis (the gravitropic set-point angle):
    /// 0 is up, 90 horizontal, 180 down. Along the axis.
    pub gravitropic_setpoint: OrderCurve,
    /// Weight `beta` of the tropism direction in the new heading
    /// `H' = alpha * H + beta * T + gamma * E` with `alpha + beta + gamma = 1`
    /// (Longay et al. 2012, eq. 2). Along the axis.
    pub tropism_weight: OrderCurve,
    /// Extension of an axis in one growth cycle, in meters, along the axis. An axis
    /// grows by this much per cycle until it reaches its `axis_length`, so the number of
    /// cycles an axis takes to mature follows from both. A curve that decreases along
    /// the axis gives vigorous early growth that slows down.
    pub shoot_length: OrderCurve,
    /// Half-width of the uniform random variation of `axis_length`, as a fraction of
    /// it, over the attachment position along the parent.
    pub length_variation: OrderCurve,
    /// Shortening of lateral axes towards the tip of their parent, in `[0, 1]`: the
    /// length of a lateral attached at relative position `x` is multiplied by
    /// `1 - apical_dominance * x`. 1 gives the conical crown of Weber and Penn (1995).
    pub apical_dominance: f32,
    /// Exponent `e` of the dependence of a lateral on the length of its parent: its
    /// length is multiplied by `(parent length / longest axis_length of the parent's
    /// order)^e`. 0 makes `axis_length` absolute, 1 makes laterals proportional to their
    /// parent as in Weber and Penn (1995).
    pub parent_length_exponent: f32,
    /// Fraction of an axis, from its base, that bears no lateral axis, over the
    /// attachment position of that axis along its own parent; the order is that of the
    /// bearing axis. The fraction is of the length the axis has at the age of the plant.
    pub bare_base: OrderCurve,
    /// Number of growth cycles a lateral axis lives before it is shed with all it
    /// bears, over its attachment position along its parent; the order is that of the
    /// bearing axis. 0 keeps laterals for ever. A palm keeps its fronds a few cycles; a
    /// pine clears its trunk this way.
    pub branch_lifespan: OrderCurve,
    /// Angle in degrees added to `branching_angle` over the relative age of a lateral
    /// with a `branch_lifespan`: 0 for a lateral formed in the last cycle, 1 for one
    /// about to be shed. A rising curve lets old fronds droop under young ones.
    pub aging_angle: Curve,
    /// Factor in `[0, 1]` applied to `branch_probability` over the position of a node in
    /// its growth unit (towards 0 for the first node, 1 for the last). Used when both
    /// growth and branching pattern are rhythmic; it shapes the tiers of branches.
    pub unit_branching: Curve,
    /// Number of lateral buds on the last node of a growth unit, evenly spread around
    /// the axis (a pseudo-whorl), along the axis; the order is that of the bearing
    /// axis. Rounded to an integer; below 0.5 the buds of that node follow the
    /// phyllotaxis like any other. Needs rhythmic growth.
    pub whorl: OrderCurve,
    /// Half-width in degrees of the uniform random variation of the divergence angle
    /// between successive nodes.
    pub phyllotaxis_variation: f32,
    /// Systematic bend of an axis in degrees, counted over its whole length and
    /// distributed along the axis by this curve: a metamer of length `l` on an axis of
    /// length `L` turns by `curvature * l / L`. Positive bends downwards, negative
    /// upwards; a curve changing sign gives an S shape (`Curve` and `CurveBack` of
    /// Weber and Penn).
    pub curvature: OrderCurve,
    /// Half-width in degrees of a random bend drawn once per growth cycle and spread
    /// over the metamers of that cycle, along the axis. Unlike `direction_variation`
    /// it persists over several metamers, which makes axes crooked.
    pub gnarl: OrderCurve,
    /// Probability in `[0, 1]` that a monopodial axis forks at the end of a growth
    /// cycle, along the axis: a second axis of the same order starts at the tip and
    /// both continue to the same `axis_length` (the stem splits of Weber and Penn).
    pub fork_probability: OrderCurve,
    /// Largest number of successive forks along one line of descent: 1 lets an axis
    /// split in two once, 2 gives at most four leaders, and so on. Without a limit a
    /// fork probability applied over many cycles doubles the axes again and again.
    pub fork_limit: u8,
    /// Angle in degrees by which each of the two axes of a fork turns away from the
    /// previous direction, along the axis.
    pub fork_angle: OrderCurve,
    /// Angle in degrees between a relay axis of a sympodial plant and the axis it
    /// continues, along the axis.
    pub relay_angle: OrderCurve,
    /// Number of stems rising from the ground (order 0 axes); more than one gives a
    /// shrub.
    pub stems: u8,
    /// Radius in meters of the circle on the ground from which several stems rise.
    pub stem_spread: f32,
    /// Outward lean of each of several stems, in degrees from the vertical. Its random
    /// variation is `branching_angle_variation` of order 0.
    pub stem_angle: f32,
    /// Compliance of the wood under its own weight, in 1/m: specific weight divided by
    /// Young's modulus (`rho * g / E`). 0 disables the bending pass.
    pub weight_bending: f32,
    /// Self-organizing mode parameters.
    pub self_organization: SelfOrganization,
}

impl Default for Growth {
    fn default() -> Self {
        Self {
            mode: GrowthMode::Parametric,
            environment_weight: 0.0,
            axis_length: curve(&[8.0, 3.0, 1.2, 0.4]),
            internode_length: curve(&[0.25, 0.15, 0.08, 0.04]),
            branch_probability: OrderCurve::constant(0.5),
            branching_angle: OrderCurve::constant(50.0),
            branching_angle_variation: OrderCurve::constant(10.0),
            direction_variation: curve(&[2.0, 6.0]),
            gravitropic_setpoint: curve(&[0.0, 60.0]),
            tropism_weight: OrderCurve::constant(0.1),
            shoot_length: curve(&[0.5, 0.4, 0.25, 0.15]),
            length_variation: OrderCurve::constant(0.1),
            apical_dominance: 0.0,
            parent_length_exponent: 0.0,
            bare_base: OrderCurve::constant(0.0),
            branch_lifespan: OrderCurve::constant(0.0),
            aging_angle: Curve::constant(0.0),
            unit_branching: Curve::linear(&[[0.0, 0.0], [1.0, 1.0]]).unwrap_or_default(),
            whorl: OrderCurve::constant(0.0),
            phyllotaxis_variation: 0.0,
            curvature: OrderCurve::constant(0.0),
            gnarl: OrderCurve::constant(0.0),
            fork_probability: OrderCurve::constant(0.0),
            fork_limit: u8::MAX,
            fork_angle: OrderCurve::constant(20.0),
            relay_angle: OrderCurve::constant(20.0),
            stems: 1,
            stem_spread: 0.0,
            stem_angle: 0.0,
            weight_bending: 0.0,
            self_organization: SelfOrganization::default(),
        }
    }
}

/// One constant per order, for the defaults.
fn curve(values: &[f32]) -> OrderCurve {
    OrderCurve::constants(values).unwrap_or_default()
}

impl Growth {
    /// The internode unit of [`SelfOrganization`], in meters: the `internode_length` of
    /// order 1 at the base of the axis.
    #[must_use]
    pub fn internode_unit(&self) -> f32 {
        self.internode_length.eval(1, 0.0)
    }

    pub(super) fn validate(&self) -> Result<(), SpeciesError> {
        let tiny = f32::MIN_POSITIVE;
        check_curve("growth.axis_length", &self.axis_length, 0.0, f32::MAX)?;
        check_curve(
            "growth.internode_length",
            &self.internode_length,
            tiny,
            f32::MAX,
        )?;
        check_curve(
            "growth.branch_probability",
            &self.branch_probability,
            0.0,
            1.0,
        )?;
        check_curve("growth.branching_angle", &self.branching_angle, 0.0, 180.0)?;
        let variation = &self.branching_angle_variation;
        check_curve("growth.branching_angle_variation", variation, 0.0, 180.0)?;
        check_curve(
            "growth.direction_variation",
            &self.direction_variation,
            0.0,
            180.0,
        )?;
        check_curve(
            "growth.gravitropic_setpoint",
            &self.gravitropic_setpoint,
            0.0,
            180.0,
        )?;
        check_range(
            "growth.environment_weight",
            self.environment_weight,
            0.0,
            1.0,
        )?;
        let free = 1.0 - self.environment_weight;
        check_curve("growth.tropism_weight", &self.tropism_weight, 0.0, free)?;
        self.validate_lengths()?;
        self.validate_shape()?;
        self.self_organization.validate()
    }

    /// Checks the parameters of axis extension, branch length and branch placement.
    fn validate_lengths(&self) -> Result<(), SpeciesError> {
        let (tiny, max) = (f32::MIN_POSITIVE, f32::MAX);
        check_curve("growth.shoot_length", &self.shoot_length, tiny, max)?;
        let variation = &self.length_variation;
        check_curve("growth.length_variation", variation, 0.0, 1.0)?;
        check_range("growth.apical_dominance", self.apical_dominance, 0.0, 1.0)?;
        let exponent = self.parent_length_exponent;
        check_range("growth.parent_length_exponent", exponent, 0.0, max)?;
        check_curve("growth.bare_base", &self.bare_base, 0.0, 1.0)?;
        let lifespan = &self.branch_lifespan;
        check_curve("growth.branch_lifespan", lifespan, 0.0, 65535.0)?;
        let (aging_lo, aging_hi) = self.aging_angle.bounds();
        check_range("growth.aging_angle", aging_lo, -180.0, 180.0)?;
        check_range("growth.aging_angle", aging_hi, -180.0, 180.0)?;
        let (unit_lo, unit_hi) = self.unit_branching.bounds();
        check_range("growth.unit_branching", unit_lo, 0.0, 1.0)?;
        check_range("growth.unit_branching", unit_hi, 0.0, 1.0)?;
        check_curve("growth.whorl", &self.whorl, 0.0, 255.0)
    }

    /// Checks the parameters of axis shape, forks, relays, stems and bending.
    fn validate_shape(&self) -> Result<(), SpeciesError> {
        let max = f32::MAX;
        let jitter = self.phyllotaxis_variation;
        check_range("growth.phyllotaxis_variation", jitter, 0.0, 180.0)?;
        check_curve("growth.curvature", &self.curvature, -max, max)?;
        check_curve("growth.gnarl", &self.gnarl, 0.0, 180.0)?;
        let forks = &self.fork_probability;
        check_curve("growth.fork_probability", forks, 0.0, 1.0)?;
        check_curve("growth.fork_angle", &self.fork_angle, 0.0, 180.0)?;
        check_curve("growth.relay_angle", &self.relay_angle, 0.0, 180.0)?;
        check_range("growth.stems", f32::from(self.stems), 1.0, 255.0)?;
        check_range("growth.stem_spread", self.stem_spread, 0.0, max)?;
        check_range("growth.stem_angle", self.stem_angle, 0.0, 180.0)?;
        check_range("growth.weight_bending", self.weight_bending, 0.0, max)
    }
}

/// Full name of a field of [`SelfOrganization`], for error messages.
macro_rules! so {
    ($name:literal) => {
        concat!("growth.self_organization.", $name)
    };
}

impl SelfOrganization {
    fn validate(&self) -> Result<(), SpeciesError> {
        let (tiny, max) = (f32::MIN_POSITIVE, f32::MAX);
        check_curve(so!("axis_bias"), &self.axis_bias, 0.0, 1.0)?;
        let alpha = self.resource_coefficient;
        check_range(so!("resource_coefficient"), alpha, 0.0, max)?;
        let exponent = self.light_exponent;
        check_range(so!("light_exponent"), exponent, tiny, max)?;
        check_range(so!("gravimorphism"), self.gravimorphism, 0.0, 1.0)?;
        let cell = self.shadow_cell;
        check_range(so!("shadow_cell"), cell, 1e-3, max)?;
        let strength = self.shadow_strength;
        check_range(so!("shadow_strength"), strength, tiny, max)?;
        let (decay, above_one) = (self.shadow_decay, 1.0 + f32::EPSILON);
        check_range(so!("shadow_decay"), decay, above_one, max)?;
        let depth = f32::from(self.shadow_depth);
        check_range(so!("shadow_depth"), depth, 0.0, 16.0)?;
        let full = self.full_exposure;
        check_range(so!("full_exposure"), full, tiny, max)?;
        self.validate_space()
    }

    /// Checks the parameters of the markers, of the priority model and of shedding.
    fn validate_space(&self) -> Result<(), SpeciesError> {
        let (tiny, max) = (f32::MIN_POSITIVE, f32::MAX);
        let occupancy = self.occupancy_radius;
        check_range(so!("occupancy_radius"), occupancy, 0.0, max)?;
        let reach = self.perception_distance;
        check_range(so!("perception_distance"), reach, 1e-3, max)?;
        let angle = self.perception_angle_deg;
        check_range(so!("perception_angle_deg"), angle, 0.0, 360.0)?;
        let density = self.marker_density;
        check_range(so!("marker_density"), density, tiny, max)?;
        let limit = self.marker_limit as f32;
        check_range(so!("marker_limit"), limit, 1.0, max)?;
        let (high, low) = (self.priority_max, self.priority_min);
        check_range(so!("priority_max"), high, tiny, max)?;
        check_range(so!("priority_min"), low, 0.0, high)?;
        let knee = self.priority_knee;
        check_range(so!("priority_knee"), knee, 1e-3, 1.0)?;
        let control = &self.apical_control;
        check_curve(so!("apical_control"), control, 0.0, 1.0)?;
        let shedding = self.shedding_threshold;
        check_range(so!("shedding_threshold"), shedding, 0.0, max)
    }
}

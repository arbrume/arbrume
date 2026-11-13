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
//! axis and internode lengths, insertion angle and its variation, branching density,
//! direction noise and tropism. [`SelfOrganization`] holds the parameters of the
//! self-organizing mode of Palubicki et al. (2009).
//!
//! All defaults are starting points for tuning, not measured values.

use super::{SpeciesError, check_curve, check_range};
use crate::curve::OrderCurve;
use serde::{Deserialize, Serialize};

/// Parameters of self-organizing growth (Palubicki et al. 2009), in which buds compete
/// for light or space. Distances are in internode lengths, as in the paper.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SelfOrganization {
    /// Bias `lambda` of the extended Borchert-Honda split between the continuation of an
    /// axis and its lateral: `v_main = v * lambda * Q_main / (lambda * Q_main +
    /// (1 - lambda) * Q_lateral)`. In `[0, 1]`; 0.5 is neutral, larger favours the main
    /// axis.
    pub axis_bias: f32,
    /// Coefficient `alpha` turning the exposure gathered at the base into resource:
    /// `v_base = alpha * Q_base`.
    pub resource_coefficient: f32,
    /// Radius of the zone around a bud that other buds cannot use.
    pub occupancy_radius: f32,
    /// Reach of the perception cone of a bud.
    pub perception_distance: f32,
    /// Full opening angle of the perception cone, in degrees.
    pub perception_angle_deg: f32,
    /// A branch is shed when its gathered exposure divided by its number of internodes
    /// falls below this value. 0 disables shedding.
    pub shedding_threshold: f32,
}

impl Default for SelfOrganization {
    /// The typical values quoted by Palubicki et al. (2009) where the paper gives one
    /// (occupancy 2, perception 4 to 6 internodes and about 90 degrees, `alpha` 2 in its
    /// examples); shedding is off.
    fn default() -> Self {
        Self {
            axis_bias: 0.5,
            resource_coefficient: 2.0,
            occupancy_radius: 2.0,
            perception_distance: 4.0,
            perception_angle_deg: 90.0,
            shedding_threshold: 0.0,
        }
    }
}

/// Growth parameters of a species.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Growth {
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
    /// Self-organizing mode parameters.
    pub self_organization: SelfOrganization,
}

impl Default for Growth {
    fn default() -> Self {
        Self {
            environment_weight: 0.0,
            axis_length: curve(&[8.0, 3.0, 1.2, 0.4]),
            internode_length: curve(&[0.25, 0.15, 0.08, 0.04]),
            branch_probability: OrderCurve::constant(0.5),
            branching_angle: OrderCurve::constant(50.0),
            branching_angle_variation: OrderCurve::constant(10.0),
            direction_variation: curve(&[2.0, 6.0]),
            gravitropic_setpoint: curve(&[0.0, 60.0]),
            tropism_weight: OrderCurve::constant(0.1),
            self_organization: SelfOrganization::default(),
        }
    }
}

/// One constant per order, for the defaults.
fn curve(values: &[f32]) -> OrderCurve {
    OrderCurve::constants(values).unwrap_or_default()
}

impl Growth {
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
        self.self_organization.validate()
    }
}

impl SelfOrganization {
    fn validate(&self) -> Result<(), SpeciesError> {
        let max = f32::MAX;
        check_range(
            "growth.self_organization.axis_bias",
            self.axis_bias,
            0.0,
            1.0,
        )?;
        let alpha = self.resource_coefficient;
        check_range(
            "growth.self_organization.resource_coefficient",
            alpha,
            0.0,
            max,
        )?;
        let occupancy = self.occupancy_radius;
        check_range(
            "growth.self_organization.occupancy_radius",
            occupancy,
            0.0,
            max,
        )?;
        let reach = self.perception_distance;
        check_range(
            "growth.self_organization.perception_distance",
            reach,
            0.0,
            max,
        )?;
        let angle = self.perception_angle_deg;
        check_range(
            "growth.self_organization.perception_angle_deg",
            angle,
            0.0,
            360.0,
        )?;
        let shedding = self.shedding_threshold;
        check_range(
            "growth.self_organization.shedding_threshold",
            shedding,
            0.0,
            max,
        )
    }
}

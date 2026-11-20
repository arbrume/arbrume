//! The species the growth cycle reads to grow a root system.
//!
//! The stages of the cycle read a [`Species`]. The root system is grown with one built
//! from the `[roots]` layer: every field of `[roots]` goes to the field of `[growth]` of
//! the same name, the architecture is that of a root system (one monopodial axis per
//! root, laterals spread around their parent or, for plagiotropic primaries, on its two
//! horizontal sides), and nothing bears leaves. Since the roots grow in the turned frame
//! ([`super::frame`]), a set-point counted from the downward vertical is the set-point
//! of that frame as written.

use arbrume_core::curve::OrderCurve;
use arbrume_core::species::{
    Allocation, Architecture, AxisCategory, AxisOrientation, BranchTiming, Branching,
    BranchingPattern, Geometry, GrowthRhythm, Phyllotaxis, RadiusModel, Roots, Sensing, Species,
};

/// The species of the root system `roots` of `shoot`.
#[must_use]
pub(crate) fn root_species(shoot: &Species, roots: &Roots) -> Species {
    let mut species = Species {
        name: format!("{} roots", shoot.name),
        architecture: architecture(roots),
        geometry: Geometry {
            radius_model: RadiusModel::Pipe,
            pipe_exponent: roots.pipe_exponent,
            tip_radius: roots.tip_radius,
            flare: 0.0,
            ..Geometry::default()
        },
        ..Species::default()
    };
    species.foliage.enabled = false;
    let growth = &mut species.growth;
    growth.mode = roots.mode;
    growth.environment_weight = roots.environment_weight;
    growth.axis_length = roots.axis_length.clone();
    growth.shoot_length = roots.shoot_length.clone();
    growth.internode_length = roots.internode_length.clone();
    growth.length_variation = roots.length_variation.clone();
    growth.branch_probability = roots.branch_probability.clone();
    growth.branching_angle = roots.branching_angle.clone();
    growth.branching_angle_variation = OrderCurve::constant(10.0);
    growth.gravitropic_setpoint = roots.gravitropic_setpoint.clone();
    growth.tropism_weight = roots.tropism_weight.clone();
    growth.direction_variation = roots.direction_variation.clone();
    growth.gnarl = roots.gnarl.clone();
    growth.weight_bending = 0.0;
    let model = &mut growth.self_organization;
    let given = &roots.self_organization;
    model.sensing = Sensing::Markers;
    model.allocation = Allocation::BorchertHonda;
    model.axis_bias = OrderCurve::constant(given.axis_bias);
    model.resource_coefficient = given.resource_coefficient;
    model.occupancy_radius = given.occupancy_radius;
    model.perception_distance = given.perception_distance;
    model.perception_angle_deg = given.perception_angle_deg;
    model.marker_density = given.marker_density;
    model.marker_limit = given.marker_limit;
    model.shedding_threshold = 0.0;
    species
}

/// The same species with a tropism that points straight down at every order: the
/// species read for sinker roots.
#[must_use]
pub(crate) fn sinker_species(species: &Species) -> Species {
    let mut sinker = species.clone();
    sinker.growth.gravitropic_setpoint = OrderCurve::constant(0.0);
    let weight = species.growth.tropism_weight.bounds().1.max(0.3);
    let free = 1.0 - species.growth.environment_weight;
    sinker.growth.tropism_weight = OrderCurve::constant(weight.min(free));
    sinker
}

/// Architecture of a root system: monopodial axes growing continuously, laterals spread
/// along their parent, up to `max_order`.
fn architecture(roots: &Roots) -> Architecture {
    let primary = AxisCategory {
        orientation: if roots.plagiotropic {
            AxisOrientation::Plagiotropic
        } else {
            AxisOrientation::Orthotropic
        },
        ..AxisCategory::default()
    };
    Architecture {
        branching: Branching::Monopodial,
        growth_rhythm: GrowthRhythm::Continuous,
        branching_pattern: BranchingPattern::Diffuse,
        branch_timing: BranchTiming::Proleptic,
        max_order: roots.max_order,
        phyllotaxis: Phyllotaxis::spiral(),
        axes: vec![primary, AxisCategory::default()],
    }
}

#[cfg(test)]
mod tests {
    use super::{root_species, sinker_species};
    use arbrume_core::species::{RootHabit, Roots, Species};

    #[test]
    fn root_species_are_valid_and_leafless() {
        for habit in RootHabit::ALL {
            let roots = Roots::of(habit);
            let species = root_species(&Species::default(), &roots);
            species.validate().unwrap();
            assert!(!species.foliage.enabled);
            assert_eq!(species.architecture.max_order, roots.max_order);
            let sinker = sinker_species(&species);
            sinker.validate().unwrap();
            assert_eq!(sinker.growth.gravitropic_setpoint.bounds(), (0.0, 0.0));
        }
    }
}

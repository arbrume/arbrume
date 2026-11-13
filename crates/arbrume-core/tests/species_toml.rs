//! Loading and round-tripping the example species file.

use arbrume_core::curve::Interpolation;
use arbrume_core::species::{AxisOrientation, BranchingPattern, Phyllotaxis, RadiusModel, Species};
use std::path::PathBuf;

fn example_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/example_broadleaf.toml")
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

#[test]
fn example_file_loads_with_the_expected_values() {
    let species = Species::from_path(example_path()).unwrap();
    assert_eq!(species.name, "Example broadleaf");

    let architecture = &species.architecture;
    assert_eq!(architecture.branching_pattern, BranchingPattern::Diffuse);
    assert_eq!(architecture.max_order, 4);
    assert_eq!(architecture.axes.len(), 3);
    assert_eq!(
        architecture.axis(1).orientation,
        AxisOrientation::Plagiotropic
    );
    assert_eq!(architecture.phyllotaxis_of(0), Phyllotaxis::spiral());
    assert_eq!(architecture.phyllotaxis_of(1), Phyllotaxis::distichous());
    assert!(architecture.axis(7).short);

    let growth = &species.growth;
    assert!(close(growth.axis_length.eval(0, 0.0), 12.0));
    assert!(close(growth.axis_length.eval(1, 0.1), 0.0));
    assert!(close(growth.axis_length.eval(1, 0.45), 5.0));
    assert_eq!(
        growth.axis_length.order(1).interpolation(),
        Interpolation::Smooth
    );
    assert!(close(growth.axis_length.eval(2, 0.5), 1.1));
    assert!(close(growth.axis_length.eval(9, 0.5), 0.4));
    assert!(close(growth.branch_probability.eval(0, 0.1), 0.0));
    assert!(close(growth.branching_angle.eval(0, 0.5), 55.0));
    assert!(close(growth.branching_angle.eval(3, 0.5), 50.0));
    assert!(close(growth.self_organization.axis_bias.eval(0, 0.0), 0.52));
    assert!(close(growth.self_organization.occupancy_radius, 2.0));

    assert_eq!(species.geometry.radius_model, RadiusModel::Pipe);
    assert!(close(species.geometry.pipe_exponent, 2.3));
    assert!(close(species.foliage.size_along_axis.eval(0.5), 0.85));
    assert_eq!(species.bark.texture, None);
}

#[test]
fn example_file_round_trips_through_toml() {
    let species = Species::from_path(example_path()).unwrap();
    let text = species.to_toml_string().unwrap();
    let again = Species::from_toml_str(&text).unwrap();
    assert_eq!(again, species);
    assert_eq!(again.to_toml_string().unwrap(), text);
}

#[test]
fn defaults_fill_everything_the_file_leaves_out() {
    let species = Species::from_toml_str("name = \"Bare\"\n[geometry]\nflare = 0.0\n").unwrap();
    let defaults = Species::default();
    assert_eq!(species.growth, defaults.growth);
    assert_eq!(species.architecture, defaults.architecture);
    assert!(close(species.geometry.flare_factor(0.0), 1.0));
}

//! Snapshot of every self-organizing preset grown on open ground, shared by the tests
//! that must leave those plants unchanged.
//!
//! One line per case: its name, then a hash of the exact `f32` bits of the graph node
//! positions and one of the mesh vertex positions, compared with the golden file of the
//! platform (see `golden`).

use std::fmt::Write as _;

use crate::golden;

use arbrume_core::species::GrowthMode;
use arbrume_gen::{GenParams, Plant, generate, preset, presets};
use glam::Vec3;

/// Age of every case.
const AGE: u16 = 10;

/// FNV-1a over the bits of `values`.
fn hash_floats(values: impl Iterator<Item = f32>) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for value in values {
        for byte in value.to_bits().to_le_bytes() {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    hash
}

/// The golden line of `plant`.
fn line(name: &str, plant: &Plant) -> String {
    let graph = hash_floats(plant.graph.positions().iter().flat_map(Vec3::to_array));
    let mesh = hash_floats(
        plant
            .mesh
            .parts
            .iter()
            .flat_map(|part| part.buffers.positions.iter().flatten().copied()),
    );
    format!("{name} graph={graph:016x} mesh={mesh:016x}")
}

/// Golden lines of every self-organizing preset, seeds 1, 2 and 3, without surroundings.
fn snapshot() -> String {
    let mut text = String::new();
    for (name, _) in presets() {
        let species = preset(name).unwrap();
        if species.growth.mode != GrowthMode::SelfOrganizing {
            continue;
        }
        for seed in [1, 2, 3] {
            let params = GenParams::new(seed, AGE).with_quality(0.5);
            let plant = generate(&species, &params).unwrap();
            writeln!(text, "{}", line(&format!("{name}/seed{seed}"), &plant)).unwrap();
        }
    }
    text
}

/// Compares the snapshot with the golden file `<name>` of the platform
/// ([`golden::check`]).
pub fn check(name: &str) {
    let got = snapshot();
    assert!(got.lines().count() >= 3, "no self-organizing preset");
    golden::check(name, &got);
}

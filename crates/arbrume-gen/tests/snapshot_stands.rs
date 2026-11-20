//! Snapshot of plants generated alone, which growing stands must not change.
//!
//! One line per case: its name, then a hash of the exact `f32` bits of the graph node
//! positions and one of the mesh vertex positions. With `ARBRUME_REGOLD=1` the test
//! writes the golden file instead of comparing with it.

use std::fmt::Write as _;
use std::path::PathBuf;

use arbrume_gen::{GenParams, Plant, generate, preset};

/// One parametric preset and two self-organizing ones that sense light.
const PRESETS: [(&str, u16); 3] = [
    ("broadleaf", 8),
    ("open_grown_broadleaf", 10),
    ("excurrent_conifer", 12),
];

/// FNV-1a over the bits of `values`.
fn hash_floats<'a>(values: impl Iterator<Item = &'a f32>) -> u64 {
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
    let graph = hash_floats(
        plant
            .graph
            .positions()
            .iter()
            .flat_map(AsRef::<[f32; 3]>::as_ref),
    );
    let mesh = hash_floats(
        plant
            .mesh
            .parts
            .iter()
            .flat_map(|part| part.buffers.positions.iter().flatten()),
    );
    format!("{name} graph={graph:016x} mesh={mesh:016x}")
}

fn snapshot() -> String {
    let mut text = String::new();
    for (name, age) in PRESETS {
        let species = preset(name).unwrap();
        for seed in [1, 2] {
            let params = GenParams::new(seed, age).with_quality(0.5);
            let plant = generate(&species, &params).unwrap();
            writeln!(text, "{}", line(&format!("{name}/seed{seed}"), &plant)).unwrap();
        }
    }
    text
}

#[test]
fn plants_grown_alone_match_the_golden_file() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/stands_golden.txt");
    let got = snapshot();
    if std::env::var("ARBRUME_REGOLD").as_deref() == Ok("1") {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &got).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path)
        .unwrap()
        .replace("\r\n", "\n");
    assert_eq!(got, want);
}

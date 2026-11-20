//! `[[stand]]` in a config file: one file per plant, the whole stand in one file, the
//! same bytes on any number of threads, and `sheet --stand`.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_arbrume");

const STAND: &str = r#"
quality = 0.3
textures = "none"
stand_file = true

[[stand]]
species = "hedge_shrub"
position = [0.0, 0.0, 0.0]
age = 5
seed = 1

[[stand]]
species = "open_grown_broadleaf"
position = [1.5, 0.0, 0.5]
age = 7
seed = 2
name = "tree"

[[stand]]
species = "broadleaf"
position = [-2.0, 0.0, 0.0]
age = 5
seed = 3
scale = 0.8
"#;

fn arbrume(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .env_remove("RAYON_NUM_THREADS")
        .output()
        .unwrap()
}

fn ok(args: &[&str]) -> Output {
    let output = arbrume(args);
    assert!(
        output.status.success(),
        "arbrume {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

/// The JSON chunk of a `.glb` file.
fn glb_json(bytes: &[u8]) -> Value {
    let length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    serde_json::from_slice(&bytes[20..20 + length]).unwrap()
}

fn write_config(dir: &Path, text: &str) -> String {
    let path = dir.join("stand.toml");
    fs::write(&path, text).unwrap();
    path.to_str().unwrap().to_owned()
}

#[test]
fn a_stand_writes_one_file_per_plant_and_the_whole_stand() {
    let dir = tempfile::tempdir().unwrap();
    let config = write_config(dir.path(), STAND);
    let one = dir.path().join("one");
    let many = dir.path().join("many");
    ok(&[
        "gen",
        "--config",
        &config,
        "--out",
        one.to_str().unwrap(),
        "-j",
        "1",
        "-q",
    ]);
    ok(&[
        "gen",
        "--config",
        &config,
        "--out",
        many.to_str().unwrap(),
        "-q",
    ]);
    let names = [
        "stand_0_hedge_shrub_1.glb",
        "stand_1_tree_2.glb",
        "stand_2_broadleaf_3.glb",
        "stand.glb",
    ];
    for name in names {
        let bytes = fs::read(one.join(name)).unwrap();
        assert_eq!(bytes, fs::read(many.join(name)).unwrap(), "{name}");
    }
    let plant = glb_json(&fs::read(one.join(names[1])).unwrap());
    let extras = &plant["nodes"][0]["extras"];
    assert_eq!(extras["stand_index"], 1);
    assert_eq!(extras["position"], serde_json::json!([1.5, 0.0, 0.5]));
    let stand = glb_json(&fs::read(one.join("stand.glb")).unwrap());
    let nodes = stand["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 4);
    assert_eq!(nodes[0]["children"], serde_json::json!([1, 2, 3]));
    assert_eq!(nodes[0]["extras"]["plants"], 3);
    assert!(nodes[1].get("translation").is_none());
    assert_eq!(nodes[2]["translation"], serde_json::json!([1.5, 0.0, 0.5]));
    assert_eq!(nodes[3]["translation"], serde_json::json!([-2.0, 0.0, 0.0]));
    assert_eq!(stand["meshes"].as_array().unwrap().len(), 3);
}

#[test]
fn a_stand_rejects_what_it_cannot_write() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out");
    let out = out.to_str().unwrap();
    let config = write_config(dir.path(), &format!("lods = 2\n{STAND}"));
    let output = arbrume(&["gen", "--config", &config, "--out", out]);
    assert_eq!(output.status.code(), Some(2));
    let config = write_config(dir.path(), "[[species]]\npreset = \"shrub\"\ncount = 1\n");
    let output = arbrume(&["gen", "--config", &config, "--out", out, "--stand-file"]);
    assert_eq!(output.status.code(), Some(2));
    let bad = STAND.replace("age = 5\nseed = 1", "age = 5\nseed = 1\nheight = 3");
    let config = write_config(dir.path(), &bad);
    let output = arbrume(&["gen", "--config", &config, "--out", out]);
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn sheet_shows_the_stand_and_the_plants_grown_alone() {
    let dir = tempfile::tempdir().unwrap();
    let config = write_config(dir.path(), STAND);
    let png = dir.path().join("stand.png");
    let png = png.to_str().unwrap();
    let output = ok(&["sheet", "--stand", &config, "--size", "64", "--out", png]);
    let text = String::from_utf8(output.stdout).unwrap();
    assert_eq!(text.lines().count(), 3, "{text}");
    assert!(text.contains("(alone"), "{text}");
    let bytes = fs::read(png).unwrap();
    assert!(bytes.starts_with(b"\x89PNG"));
    let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    assert_eq!(width, 128);
}

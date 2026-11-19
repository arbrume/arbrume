//! Runs `arbrume gen` with levels of detail, billboards and wind data.

use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_arbrume");

fn ok(args: &[&str]) {
    let output = Command::new(BIN)
        .args(args)
        .env_remove("RAYON_NUM_THREADS")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "arbrume {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The JSON chunk of a glb file.
fn json_of(path: &Path) -> Value {
    let bytes = fs::read(path).unwrap();
    let len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    serde_json::from_slice(&bytes[20..20 + len]).unwrap()
}

#[test]
fn levels_and_billboard_are_separate_files_with_their_budgets() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().to_str().unwrap();
    let args = [
        "gen",
        "--species",
        "shrub",
        "--age",
        "5",
        "--lods",
        "2",
        "--billboard",
    ];
    ok(&[&args[..], &["--wind", "--out", out, "--quiet"]].concat());
    let mut triangles = Vec::new();
    for (lod, kind) in [(0, "mesh"), (1, "mesh"), (2, "billboard")] {
        let json = json_of(&dir.path().join(format!("shrub_0_0_lod{lod}.glb")));
        let extras = &json["nodes"][0]["extras"];
        assert_eq!(extras["lod"], lod);
        assert_eq!(extras["lod_kind"], kind);
        let count = extras["lod_triangles"].as_u64().unwrap();
        assert!(count <= extras["lod_budget"].as_u64().unwrap());
        triangles.push(count);
        let attributes = &json["meshes"][0]["primitives"][0]["attributes"];
        assert!(attributes.get("TEXCOORD_3").is_some(), "{attributes}");
        assert!(
            json["meshes"][0]["extras"]["arbrume_wind"]["branches"]["count"].as_u64() > Some(0)
        );
    }
    assert!(triangles[0] > triangles[1] && triangles[1] > triangles[2]);
}

#[test]
fn one_file_holds_every_level_with_skin_and_instances() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().to_str().unwrap();
    let args = [
        "gen",
        "--species",
        "excurrent_conifer",
        "--age",
        "6",
        "--lods",
        "2",
    ];
    let flags = [
        "--wind",
        "--skin",
        "--billboard",
        "cross",
        "--lod-file",
        "--instance-leaves",
    ];
    ok(&[&args[..], &flags, &["--out", out, "--quiet"]].concat());
    let json = json_of(&dir.path().join("excurrent_conifer_0_0.glb"));
    let names: Vec<&str> = json["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|node| node["name"].as_str())
        .collect();
    for level in [
        "excurrent_conifer_0_0_LOD0",
        "excurrent_conifer_0_0_LOD1",
        "excurrent_conifer_0_0_LOD2",
    ] {
        assert!(names.contains(&level), "{names:?}");
    }
    assert!(
        json["skins"]
            .as_array()
            .is_some_and(|skins| skins.len() == 3)
    );
    assert_eq!(json["extensionsUsed"][0], "EXT_mesh_gpu_instancing");
    assert_eq!(json["nodes"][0]["extras"]["mesh"]["min_radius"], 0.0);
}

#[test]
fn the_lod_table_of_a_config_file_is_read() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("batch.toml");
    let text = "lods = 2\nlod = { wind = \"custom\", billboard = \"cloud\", file = true }\n\
                [[species]]\npreset = \"shrub\"\ncount = 1\nage = 4\n";
    fs::write(&config, text).unwrap();
    let out = dir.path().join("out");
    ok(&[
        "gen",
        "--config",
        config.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "-q",
    ]);
    let json = json_of(&out.join("shrub_0_0.glb"));
    let attributes = &json["meshes"][0]["primitives"][0]["attributes"];
    assert!(attributes.get("_WIND_X").is_some(), "{attributes}");
    assert_eq!(json["meshes"].as_array().unwrap().len(), 3);
}

//! `arbrume gen --annotations` and `arbrume metrics`: files, manifest, resume, threads.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_arbrume");

fn ok(args: &[&str]) -> Output {
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
    output
}

/// File name to contents of every file in `dir` but the manifest.
fn files(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.file_name().unwrap() != "manifest.jsonl")
        .map(|path| {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            (name, fs::read(&path).unwrap())
        })
        .collect()
}

fn generate(out: &Path, jobs: &str, extra: &[&str]) {
    let out = out.to_str().unwrap();
    let mut args = vec!["gen", "--species", "conifer", "--count", "3", "--age", "8"];
    args.extend([
        "--quality",
        "0.4",
        "--seed",
        "2",
        "--out",
        out,
        "--jobs",
        jobs,
    ]);
    args.extend(["--manifest", "--annotations"]);
    args.push("skeleton,labels,points=300,branches=4,metrics,json");
    args.extend(extra);
    ok(&args);
}

#[test]
fn annotations_are_written_listed_and_independent_of_threads() {
    let one = tempfile::tempdir().unwrap();
    let many = tempfile::tempdir().unwrap();
    generate(one.path(), "1", &[]);
    generate(many.path(), "4", &[]);
    let written = files(one.path());
    assert_eq!(written, files(many.path()));
    for index in 0..3 {
        for suffix in ["glb", "ann.npz", "ann.json", "points.ply"] {
            assert!(written.contains_key(&format!("conifer_2_{index}.{suffix}")));
        }
    }
    let manifest = fs::read_to_string(one.path().join("manifest.jsonl")).unwrap();
    for line in manifest.lines() {
        let record: Value = serde_json::from_str(line).unwrap();
        let index = &record["index"];
        assert_eq!(
            record["annotations"]["sidecar"],
            format!("conifer_2_{index}.ann.npz")
        );
        assert!(record["species_params"]["growth"].is_object());
        assert!(record["metrics"]["local"]["sibling_angle"]["count"].as_u64() > Some(0));
        assert!(record["annotate_ms"].as_f64().is_some());
    }
}

#[test]
fn resume_regenerates_a_plant_whose_annotation_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    generate(dir.path(), "2", &[]);
    let before = files(dir.path());
    fs::remove_file(dir.path().join("conifer_2_1.points.ply")).unwrap();
    generate(dir.path(), "2", &["--resume"]);
    assert_eq!(files(dir.path()), before);
    let manifest = fs::read_to_string(dir.path().join("manifest.jsonl")).unwrap();
    assert_eq!(manifest.lines().count(), 4, "plant 1 is listed again");
}

#[test]
fn metrics_summarise_sidecars_and_presets() {
    let dir = tempfile::tempdir().unwrap();
    generate(dir.path(), "2", &[]);
    let output = ok(&["metrics", dir.path().to_str().unwrap()]);
    let table = String::from_utf8(output.stdout).unwrap();
    assert!(table.contains("| conifer | 3 |"), "{table}");
    let output = ok(&["metrics", "--presets", "shrub", "--seeds", "4", "--json"]);
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    let shrub = &json["shrub (age 9)"];
    assert_eq!(shrub["plants"], 4, "{json}");
    assert!(shrub["local"]["segment_volume"]["mean"].as_f64() > Some(0.0));
}

#[test]
fn bad_annotation_lists_are_usage_errors() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().to_str().unwrap();
    for list in ["points", "bones", "json"] {
        let output = Command::new(BIN)
            .args([
                "gen",
                "--species",
                "shrub",
                "--out",
                out,
                "--annotations",
                list,
            ])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{list}");
    }
}

#[test]
fn a_config_file_asks_for_annotations() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("batch.toml");
    let text = "out = \"data\"\nannotations = \"skeleton,branches=3\"\n\
                [[species]]\npreset = \"shrub\"\ncount = 2\n";
    fs::write(&config, text).unwrap();
    ok(&["gen", "--config", config.to_str().unwrap(), "--quiet"]);
    let out = dir.path().join("data");
    assert!(out.join("shrub_0_1.ann.npz").is_file());
    assert!(!out.join("shrub_0_1.points.ply").exists());
    fs::write(&config, text.replace("branches=3", "branches=1")).unwrap();
    let output = Command::new(BIN)
        .args(["gen", "--config", config.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
}

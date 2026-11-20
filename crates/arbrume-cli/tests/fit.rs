//! `arbrume fit` and `arbrume distance`: inputs, outputs, threads and resume.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_arbrume");

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .env_remove("RAYON_NUM_THREADS")
        .output()
        .unwrap()
}

fn ok(args: &[&str]) -> Output {
    let output = run(args);
    assert!(
        output.status.success(),
        "arbrume {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn path(dir: &Path, name: &str) -> String {
    dir.join(name).to_str().unwrap().to_owned()
}

/// Writes a sapling plant with its skeleton sidecar to `dir`.
fn target(dir: &Path) {
    let out = dir.to_str().unwrap();
    ok(&[
        "gen",
        "--species",
        "sapling",
        "--age",
        "6",
        "--seed",
        "4",
        "--out",
        out,
        "--annotations",
        "skeleton",
        "--textures",
        "none",
        "-q",
    ]);
}

#[test]
fn distance_reads_sidecars_glb_and_csv() {
    let dir = tempfile::tempdir().unwrap();
    target(dir.path());
    let sidecar = path(dir.path(), "sapling_4_0.ann.npz");
    let glb = path(dir.path(), "sapling_4_0.glb");
    let text = String::from_utf8(ok(&["distance", &sidecar, &glb]).stdout).unwrap();
    assert!(text.starts_with("envelope 0.0000"), "{text}");
    let csv = path(dir.path(), "fork.csv");
    fs::write(
        &csv,
        "id,x,y,z,radius,parent\n0,0,0,0,0.1,\n1,0,0,1,0.1,0\n2,0.5,0,1.5,0.05,1\n",
    )
    .unwrap();
    let json = ok(&["distance", &csv, &sidecar, "--up", "z", "--json"]).stdout;
    let terms: serde_json::Value = serde_json::from_slice(&json).unwrap();
    assert!(terms["total"].as_f64().unwrap() > 0.0);
    let missing = run(&["distance", &csv, &path(dir.path(), "none.csv")]);
    assert_eq!(missing.status.code(), Some(2));
    let bare = tempfile::tempdir().unwrap();
    let out = bare.path().to_str().unwrap();
    ok(&[
        "gen",
        "--species",
        "sapling",
        "--age",
        "6",
        "--seed",
        "4",
        "--out",
        out,
        "-q",
    ]);
    let regrown = path(bare.path(), "sapling_4_0.glb");
    let text = String::from_utf8(ok(&["distance", &regrown, &sidecar]).stdout).unwrap();
    assert!(text.starts_with("envelope 0.0000"), "{text}");
}

#[test]
fn fit_writes_species_and_report_and_resumes() {
    let dir = tempfile::tempdir().unwrap();
    target(dir.path());
    let sidecar = path(dir.path(), "sapling_4_0.ann.npz");
    let fit = |out: &str, jobs: &str, extra: &[&str]| {
        let mut args = vec![
            "fit",
            "--target",
            &sidecar,
            "--from",
            "sapling",
            "--params",
            "growth.axis_length,growth.branching_angle,age",
            "--seeds",
            "2",
            "--budget",
            "40",
            "--holdout",
            "2",
            "--jobs",
            jobs,
            "-q",
            "--out",
            out,
        ];
        args.extend_from_slice(extra);
        ok(&args);
        fs::read_to_string(out).unwrap()
    };
    let one = fit(&path(dir.path(), "one.toml"), "1", &[]);
    let report = path(dir.path(), "report.json");
    let many = fit(&path(dir.path(), "many.toml"), "4", &["--report", &report]);
    assert_eq!(one, many);
    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&report).unwrap()).unwrap();
    assert_eq!(report["format"], "arbrume-fit-report");
    assert!(report["after"]["holdout"]["total"].as_f64().is_some());
    assert!(dir.path().join("many.checkpoint.json").exists());
    let resumed = fit(&path(dir.path(), "many.toml"), "2", &["--resume"]);
    assert_eq!(resumed, one);
    let species = path(dir.path(), "one.toml");
    ok(&["info", &species]);
}

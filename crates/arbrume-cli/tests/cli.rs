//! Runs the `arbrume` binary: files, manifest, resume, configs, errors and determinism.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_arbrume");

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

fn path(dir: &Path) -> &str {
    dir.to_str().unwrap()
}

/// File name to contents of every `.glb` in `dir`.
fn glb_files(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "glb"))
        .map(|p| {
            (
                p.file_name().unwrap().to_string_lossy().into_owned(),
                fs::read(&p).unwrap(),
            )
        })
        .collect()
}

fn manifest(dir: &Path) -> Vec<Value> {
    fs::read_to_string(dir.join("manifest.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn gen_writes_files_and_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let out = path(dir.path());
    ok(&[
        "gen",
        "--species",
        "shrub",
        "--count",
        "3",
        "--seed",
        "9",
        "--age",
        "4",
    ]
    .into_iter()
    .chain(["--out", out, "--manifest", "--quiet"])
    .collect::<Vec<_>>());
    let files = glb_files(dir.path());
    let names: Vec<&str> = files.keys().map(String::as_str).collect();
    assert_eq!(names, ["shrub_9_0.glb", "shrub_9_1.glb", "shrub_9_2.glb"]);
    for bytes in files.values() {
        assert_eq!(&bytes[..4], b"glTF");
        let length = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
        assert_eq!(length as usize, bytes.len());
    }
    let mut lines = manifest(dir.path());
    lines.sort_by_key(|line| line["index"].as_u64());
    assert_eq!(lines.len(), 3);
    for (index, line) in lines.iter().enumerate() {
        assert_eq!(line["index"], index);
        assert_eq!(line["species"], "shrub");
        assert_eq!(line["seed"], 9);
        assert_eq!(line["age"], 4);
        assert_eq!(line["files"][0]["file"], format!("shrub_9_{index}.glb"));
        let bytes = files[&format!("shrub_9_{index}.glb")].len();
        assert_eq!(line["files"][0]["bytes"], bytes);
        assert!(line["stats"]["triangles"].as_u64().unwrap() > 0);
        assert!(line["timings_ms"]["grow"].as_f64().unwrap() >= 0.0);
    }
    assert!(
        !fs::read_dir(dir.path())
            .unwrap()
            .any(|e| { e.unwrap().path().extension().is_some_and(|x| x == "tmp") })
    );
}

#[test]
fn output_is_identical_for_any_number_of_threads() {
    let args = [
        "gen",
        "--species",
        "broadleaf",
        "--count",
        "6",
        "--seed",
        "21",
    ];
    let extra = [
        "--age-range",
        "3..7",
        "--scale",
        "0.8..1.2",
        "--lods",
        "2",
        "--quiet",
    ];
    let mut runs = Vec::new();
    for jobs in ["1", "8"] {
        let dir = tempfile::tempdir().unwrap();
        let mut all: Vec<&str> = args.iter().chain(&extra).copied().collect();
        all.extend(["--out", path(dir.path()), "--jobs", jobs]);
        ok(&all);
        runs.push(glb_files(dir.path()));
    }
    for threads in ["1", "5"] {
        let dir = tempfile::tempdir().unwrap();
        let mut all: Vec<&str> = args.iter().chain(&extra).copied().collect();
        all.extend(["--out", path(dir.path())]);
        let output = Command::new(BIN)
            .args(&all)
            .env("RAYON_NUM_THREADS", threads)
            .output()
            .unwrap();
        assert!(output.status.success());
        runs.push(glb_files(dir.path()));
    }
    assert_eq!(runs[0].len(), 12);
    assert!(runs[0].contains_key("broadleaf_21_5_lod1.glb"));
    for run in &runs[1..] {
        assert!(run == &runs[0], "outputs differ between thread counts");
    }
}

#[test]
fn resume_skips_complete_plants() {
    let dir = tempfile::tempdir().unwrap();
    let out = path(dir.path());
    let run = |count: &str, resume: bool| {
        let mut args = vec![
            "gen",
            "--species",
            "conifer",
            "--count",
            count,
            "--age",
            "3",
        ];
        args.extend(["--out", out, "--manifest", "--quiet"]);
        if resume {
            args.push("--resume");
        }
        ok(&args);
    };
    run("3", false);
    let kept = dir.path().join("conifer_0_0.glb");
    let before = fs::metadata(&kept).unwrap().modified().unwrap();
    fs::remove_file(dir.path().join("conifer_0_1.glb")).unwrap();
    let mut text = fs::read_to_string(dir.path().join("manifest.jsonl")).unwrap();
    text.push_str("{\"species\": \"conif");
    fs::write(dir.path().join("manifest.jsonl"), text).unwrap();
    run("5", true);
    assert_eq!(fs::metadata(&kept).unwrap().modified().unwrap(), before);
    assert_eq!(glb_files(dir.path()).len(), 5);
    let mut indices: Vec<u64> = manifest(dir.path())
        .iter()
        .map(|line| line["index"].as_u64().unwrap())
        .collect();
    indices.sort_unstable();
    assert_eq!(indices, [0, 1, 1, 2, 3, 4]);
}

#[test]
fn config_lists_species_with_distributions_and_overrides() {
    let dir = tempfile::tempdir().unwrap();
    let species_file = dir.path().join("my oak.toml");
    fs::write(
        &species_file,
        "name = \"Test oak\"\n[growth]\naxis_length = 3.0\n",
    )
    .unwrap();
    let config = dir.path().join("batch.toml");
    let text = r#"
        out = "dataset"
        seed = 5
        quality = 0.5
        manifest = true
        age = { uniform = [3, 5] }

        [[species]]
        preset = "shrub"
        count = 2
        scale = { normal = [1.0, 0.1], min = 0.5, max = 1.5 }

        [[species]]
        preset = "shrub"
        name = "shrub_small_leaves"
        count = 1
        overrides = { "foliage.length" = 0.02 }

        [[species]]
        path = "my oak.toml"
        count = 2
        seed = 8
        age = 4
    "#;
    fs::write(&config, text).unwrap();
    ok(&["gen", "--config", path(&config), "--quiet"]);
    let out = dir.path().join("dataset");
    let names: Vec<String> = glb_files(&out).into_keys().collect();
    let expected = [
        "my_oak_8_0.glb",
        "my_oak_8_1.glb",
        "shrub_5_0.glb",
        "shrub_5_1.glb",
        "shrub_small_leaves_5_0.glb",
    ];
    assert_eq!(names, expected);
    let lines = manifest(&out);
    assert_eq!(lines.len(), 5);
    for line in &lines {
        assert_eq!(line["quality"], 0.5);
        let age = line["age"].as_u64().unwrap();
        assert!((3..=5).contains(&age));
    }
    let oak = lines.iter().find(|l| l["species"] == "my_oak").unwrap();
    assert_eq!(oak["species_name"], "Test oak");
    assert_eq!(oak["age"], 4);
}

#[test]
fn bad_input_exits_with_code_2() {
    let dir = tempfile::tempdir().unwrap();
    let out = path(dir.path());
    let unknown = arbrume(&["gen", "--species", "baobab", "--out", out]);
    assert_eq!(unknown.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("broadleaf"));
    let no_out = arbrume(&["gen", "--species", "shrub"]);
    assert_eq!(no_out.status.code(), Some(2));
    let both = arbrume(&[
        "gen",
        "--species",
        "shrub",
        "--age",
        "3",
        "--age-range",
        "1..2",
    ]);
    assert_eq!(both.status.code(), Some(2));
    let config = dir.path().join("bad.toml");
    fs::write(
        &config,
        "[[species]]\npreset = \"shrub\"\ncount = 1\nage = { normal = [5, 1] }\n",
    )
    .unwrap();
    let unbounded = arbrume(&["gen", "--config", path(&config), "--out", out]);
    assert_eq!(unbounded.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&unbounded.stderr).contains("age"));
    fs::write(
        &config,
        "[[species]]\npreset = \"shrub\"\ncount = 1\nspeed = 3\n",
    )
    .unwrap();
    let unknown_key = arbrume(&["gen", "--config", path(&config), "--out", out]);
    assert_eq!(unknown_key.status.code(), Some(2));
    assert_eq!(
        arbrume(&["species", "show", "baobab"]).status.code(),
        Some(2)
    );
}

#[test]
fn species_commands_and_info() {
    let list = String::from_utf8(ok(&["species", "list"]).stdout).unwrap();
    assert!(list.lines().any(|line| line.starts_with("conifer\t")));
    let shown = String::from_utf8(ok(&["species", "show", "conifer"]).stdout).unwrap();
    assert_eq!(shown, arbrume_gen::preset_toml("conifer").unwrap());
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("conifer.toml");
    fs::write(&file, &shown).unwrap();
    let info = String::from_utf8(ok(&["info", path(&file)]).stdout).unwrap();
    assert!(info.contains("valid species `Starter conifer`"));
    fs::write(&file, "[growth]\nbranch_probability = 2.0\n").unwrap();
    let invalid = arbrume(&["info", path(&file)]);
    assert_eq!(invalid.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("growth.branch_probability"));
}

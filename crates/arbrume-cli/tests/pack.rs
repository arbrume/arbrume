//! `arbrume pack`: every subcommand, and pack species used by `arbrume gen`.

use std::path::Path;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_arbrume");

/// Runs `arbrume` with the pack directory `packs`.
fn arbrume(packs: &Path, args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .env("ARBRUME_PACKS_DIR", packs)
        .env_remove("RAYON_NUM_THREADS")
        .output()
        .unwrap()
}

/// Runs `arbrume` and returns its standard output; fails the test if it fails.
fn ok(packs: &Path, args: &[&str]) -> String {
    let output = arbrume(packs, args);
    assert!(
        output.status.success(),
        "arbrume {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn text(path: &Path) -> &str {
    path.to_str().unwrap()
}

/// `pack new`, `pack add-species` and `pack validate` of a pack directory `dir`.
fn author(packs: &Path, dir: &Path) {
    let d = text(dir);
    ok(packs, &["pack", "new", d, "--name", "Small plants"]);
    let added = ok(
        packs,
        &[
            "pack",
            "add-species",
            d,
            "rosette_herb",
            "--category",
            "herb",
        ],
    );
    assert!(added.contains("rosette_herb"), "{added}");
    let young = ["--name", "young", "--display-name", "Young tree"];
    ok(
        packs,
        &[&["pack", "add-species", d, "sapling"][..], &young].concat(),
    );
    let duplicate = arbrume(
        packs,
        &["pack", "add-species", d, "sapling", "--name", "young"],
    );
    assert_eq!(duplicate.status.code(), Some(2));
    let unknown = arbrume(packs, &["pack", "add-species", d, "no_such_preset"]);
    assert_eq!(unknown.status.code(), Some(2));
    let output = arbrume(packs, &["pack", "validate", d]);
    assert!(output.status.success());
    let warnings = String::from_utf8_lossy(&output.stderr);
    assert!(warnings.contains("placeholder"), "{warnings}");
    assert!(warnings.contains("no thumbnail"), "{warnings}");
    let thumbs = ok(packs, &["pack", "thumbs", d, "--size", "64"]);
    assert!(thumbs.contains("2 thumbnails written"), "{thumbs}");
    assert!(dir.join("thumbnails/young.png").is_file());
    assert!(ok(packs, &["pack", "thumbs", d]).contains("0 thumbnails"));
    assert_eq!(
        arbrume(packs, &["pack", "thumbs", d, "--size", "48"])
            .status
            .code(),
        Some(2)
    );
}

/// `pack build` twice, `pack validate` of the archive, `pack install` and `pack list`.
fn build_and_install(packs: &Path, dir: &Path, root: &Path) {
    let first = root.join("a.arbpack");
    let second = root.join("b.arbpack");
    ok(packs, &["pack", "build", text(dir), "-o", text(&first)]);
    ok(packs, &["pack", "build", text(dir), "--out", text(&second)]);
    assert_eq!(
        std::fs::read(&first).unwrap(),
        std::fs::read(&second).unwrap()
    );
    assert!(ok(packs, &["pack", "validate", text(&first)]).contains("valid pack `small`"));
    let other = root.join("other");
    let installed = ok(
        packs,
        &["pack", "install", text(&first), "--dir", text(&other)],
    );
    assert!(
        installed.contains("installed pack `small` 0.1.0"),
        "{installed}"
    );
    assert!(other.join("small/pack.toml").is_file());
    ok(packs, &["pack", "install", text(&first)]);
    let list = ok(packs, &["pack", "list"]);
    assert!(list.contains("small\t0.1.0\tSmall plants"), "{list}");
    assert!(list.contains("  small/young\tYoung tree"), "{list}");
}

/// `gen --species pack/name` and `gen --random --packs` from the installed pack.
fn generate(packs: &Path, root: &Path) {
    let out = root.join("out");
    let species = [
        "gen",
        "--species",
        "small/young",
        "--count",
        "2",
        "--age",
        "4",
    ];
    ok(
        packs,
        &[&species[..], &["--out", text(&out), "--quiet"]].concat(),
    );
    assert!(out.join("small-young_0_0.glb").is_file());
    assert!(out.join("small-young_0_1.glb").is_file());
    let random = root.join("random");
    let draw = [
        "gen", "--random", "--packs", "small", "--count", "6", "--seed", "4",
    ];
    ok(
        packs,
        &[&draw[..], &["--out", text(&random), "--quiet"]].concat(),
    );
    let names: Vec<String> = std::fs::read_dir(&random)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "glb"))
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), 6);
    assert!(
        names.iter().all(|name| name.starts_with("small-")),
        "{names:?}"
    );
    let missing = arbrume(
        packs,
        &["gen", "--species", "small/nothing", "--out", text(&out)],
    );
    assert_eq!(missing.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("no species `nothing`"));
    let absent = ["gen", "--random", "--packs", "absent", "--out", text(&out)];
    assert_eq!(arbrume(packs, &absent).status.code(), Some(2));
}

#[test]
fn author_build_install_list_generate_remove() {
    let root = tempfile::tempdir().unwrap();
    let packs = root.path().join("installed");
    let dir = root.path().join("small");
    author(&packs, &dir);
    build_and_install(&packs, &dir, root.path());
    generate(&packs, root.path());
    assert!(ok(&packs, &["pack", "remove", "small"]).contains("removed pack `small`"));
    assert!(ok(&packs, &["pack", "list"]).contains("no pack installed"));
    assert_eq!(
        arbrume(&packs, &["pack", "remove", "small"]).status.code(),
        Some(2)
    );
}

#[test]
fn invalid_packs_fail_validation_and_installation() {
    let root = tempfile::tempdir().unwrap();
    let packs = root.path().join("installed");
    let dir = root.path().join("bad");
    let d = text(&dir);
    ok(&packs, &["pack", "new", d]);
    let empty = arbrume(&packs, &["pack", "validate", d]);
    assert_eq!(empty.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&empty.stderr).contains("at least one species"));
    ok(&packs, &["pack", "add-species", d, "shrub"]);
    std::fs::write(
        dir.join("species/shrub.toml"),
        "[geometry]\npipe_exponent = 0.0\n",
    )
    .unwrap();
    let output = arbrume(&packs, &["pack", "validate", d]);
    assert_eq!(output.status.code(), Some(2));
    let errors = String::from_utf8_lossy(&output.stderr);
    assert!(errors.contains("error: species/shrub.toml"), "{errors}");
    assert!(errors.contains("geometry.pipe_exponent"), "{errors}");
    let install = arbrume(&packs, &["pack", "install", d]);
    assert_eq!(install.status.code(), Some(2));
    assert!(!packs.join("bad").exists());
    let build = arbrume(
        &packs,
        &[
            "pack",
            "build",
            d,
            "-o",
            text(&root.path().join("x.arbpack")),
        ],
    );
    assert_eq!(build.status.code(), Some(2));
    let occupied = arbrume(&packs, &["pack", "new", d]);
    assert_eq!(occupied.status.code(), Some(2));
}

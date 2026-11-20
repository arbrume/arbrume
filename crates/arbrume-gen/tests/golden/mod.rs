//! Golden files of the snapshot tests, one per platform.
//!
//! The snapshots hash the exact `f32` bits of plants. Those bits are the same for every
//! thread count on one platform, but not across platforms: functions such as `powf`,
//! `sin` and `cos` come from the math library of the platform and may round their last
//! bit differently. Each golden file is therefore named `<name>_golden.<os>.txt`, `<os>`
//! being `windows`, `linux` or `macos` (`std::env::consts::OS`).
//!
//! With `ARBRUME_REGOLD=1` a test writes the golden file of the current platform instead
//! of comparing with it. Without it, a missing golden file fails the test with a message
//! that says how to make it.

use std::path::PathBuf;

/// Compares `got` with the golden file `tests/fixtures/<name>_golden.<os>.txt` of the
/// current platform, or writes that file when `ARBRUME_REGOLD=1`.
pub fn check(name: &str, got: &str) {
    let os = std::env::consts::OS;
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("{name}_golden.{os}.txt"));
    if std::env::var("ARBRUME_REGOLD").as_deref() == Ok("1") {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, got).unwrap();
        return;
    }
    let Ok(want) = std::fs::read_to_string(&path) else {
        panic!(
            "no golden file {} for {os}: run this test on {os} with ARBRUME_REGOLD=1 to \
             write it, then commit it. For Linux and macOS the manual workflow \
             .github/workflows/regold.yml writes the files and uploads them as the \
             artifacts goldens-linux and goldens-macos.",
            path.display()
        );
    };
    assert_eq!(got, want.replace("\r\n", "\n"));
}

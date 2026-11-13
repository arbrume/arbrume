//! Helpers shared by the integration tests.

use arbrume_core::species::Species;
use std::path::PathBuf;

/// Names of the starter species files under `tests/data`.
pub const STARTERS: [&str; 3] = ["broadleaf", "conifer", "shrub"];

/// Loads and validates the starter species `name`.
pub fn starter(name: &str) -> Species {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(format!("{name}.toml"));
    Species::from_path(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

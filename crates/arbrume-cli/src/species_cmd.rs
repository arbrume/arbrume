//! `arbrume species list`, `arbrume species show` and `arbrume info`.

use std::path::Path;

use arbrume_core::species::Species;
use arbrume_gen::{GenParams, generate, preset, preset_toml, presets};

use crate::error::CliError;

/// Prints the name and the display name of every preset.
pub fn list() {
    for (name, _) in presets() {
        let display = preset(name).map(|species| species.name).unwrap_or_default();
        println!("{name}\t{display}");
    }
}

/// Prints the TOML text of the preset `name`.
pub fn show(name: &str) -> Result<(), CliError> {
    let text = preset_toml(name).ok_or_else(|| {
        let names: Vec<&str> = presets().iter().map(|(name, _)| *name).collect();
        CliError::usage(format!("no preset `{name}`; presets: {}", names.join(", ")))
    })?;
    print!("{text}");
    Ok(())
}

/// Validates the species file at `path`, then generates one plant with the default
/// parameters and prints its statistics.
pub fn info(path: &Path) -> Result<(), CliError> {
    let species = Species::from_path(path)
        .map_err(|error| CliError::usage(format!("{}: {error}", path.display())))?;
    println!("{}: valid species `{}`", path.display(), species.name);
    let params = GenParams::default();
    let plant = generate(&species, &params).map_err(CliError::failed)?;
    let stats = &plant.stats;
    println!(
        "seed {} age {}: {} nodes, {} chains, {} leaf sites, {} vertices, {} triangles, \
         height {:.2} m",
        params.seed,
        params.age,
        stats.nodes,
        stats.chains,
        stats.leaf_sites,
        stats.vertices,
        stats.triangles,
        stats.height
    );
    Ok(())
}

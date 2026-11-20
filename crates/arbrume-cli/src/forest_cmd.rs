//! `arbrume forest`: a forest of a config file, placed over its terrain, with its variant
//! library, written as variant files, an instanced file and instance tables
//! (`docs/forest.md`).

use std::path::{Path, PathBuf};
use std::time::Instant;

use arbrume_export::{ExportOptions, WindLayout};
use arbrume_forest::{Forest, ForestConfig, write_forest};
use clap::Args;

use crate::error::CliError;

/// Arguments of `arbrume forest`.
#[derive(Debug, Args)]
pub struct ForestArgs {
    /// Forest config file (docs/forest.md).
    #[arg(long)]
    config: PathBuf,
    /// Output directory.
    #[arg(long)]
    out: PathBuf,
    /// Replace the seed of the config.
    #[arg(long)]
    seed: Option<u64>,
    /// Place the trees and write the tables only: no variants are grown, so every species
    /// needs a `spacing`.
    #[arg(long)]
    placement_only: bool,
}

/// Runs `arbrume forest`.
pub fn run(args: &ForestArgs) -> Result<(), CliError> {
    let start = Instant::now();
    let mut config = ForestConfig::from_path(&args.config).map_err(CliError::usage)?;
    if let Some(seed) = args.seed {
        config.seed = seed;
    }
    let base = args.config.parent().unwrap_or(Path::new("."));
    let forest = Forest::build(&config, base, !args.placement_only).map_err(|e| match e {
        arbrume_forest::ForestError::Config(_)
        | arbrume_forest::ForestError::Terrain(_)
        | arbrume_forest::ForestError::Exclusion(_) => CliError::usage(e),
        other => CliError::failed(other),
    })?;
    let export = ExportOptions {
        wind: if config.library.wind {
            WindLayout::TexCoord
        } else {
            WindLayout::None
        },
        ..ExportOptions::default()
    };
    let mut output = config.output.clone();
    if args.placement_only {
        output.variants = false;
        output.instanced = false;
    }
    let written = write_forest(&forest, &output, &export, &args.out).map_err(CliError::failed)?;
    summary(&forest, &written, start.elapsed().as_secs_f64());
    Ok(())
}

/// Prints the trees per species, the times and the files.
fn summary(forest: &Forest, written: &[arbrume_forest::Written], seconds: f64) {
    println!(
        "{}: {} trees, placed in {:.3} s, library in {:.2} s",
        forest.name,
        forest.instances.len(),
        forest.placement.time.as_secs_f64(),
        forest.library_time.as_secs_f64()
    );
    for (s, spec) in forest.specs.iter().enumerate() {
        let counts = &forest.placement.species[s];
        let warning = if counts.saturated_cells > 0 {
            format!(
                " (density out of reach in {} cells: lower it or the spacing)",
                counts.saturated_cells
            )
        } else {
            String::new()
        };
        println!(
            "  {}: {} trees, {:.0} expected on open ground, spacing {:.2} m{warning}",
            spec.name, counts.kept, counts.expected_open, forest.rules[s].spacing
        );
    }
    if let Some(coupled) = &forest.coupled {
        println!(
            "  coupled region: {} trees in {} tiles, largest stand {}, {:.2} s",
            coupled.trees.len(),
            coupled.tiles,
            coupled.largest_stand,
            coupled.time.as_secs_f64()
        );
    }
    for file in written {
        println!("  {} ({} bytes)", file.path.display(), file.bytes);
    }
    println!("done in {seconds:.2} s");
}

//! `arbrume metrics`: the `ICTree` features of skeletons, summarised per species.
//!
//! Two sources of skeletons:
//!
//! * paths: directories (not searched recursively) or files of `.ann.npz` sidecars
//!   written by `gen --annotations skeleton`; a plant is grouped under the label of its
//!   file name (`<label>_<seed>_<index>.ann.npz`);
//! * `--presets`: plants grown here, without a mesh: indices `0..seeds` of base seed
//!   `--seed`, at `--age` or, by default, the middle of the `variation.age` range of the
//!   preset, at scale 1, with the preset parameters (no jitter).
//!
//! For every group the table gives, for a few features, the mean and the standard
//! deviation over the plants of the per-plant value (the mean of a local feature, the
//! value of a global one). `--json` prints every feature.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use arbrume_core::species::Species;
use arbrume_export::annotations::Annotations;
use arbrume_gen::{plant_seed, preset, presets};
use arbrume_grow::{GrowParams, Surroundings, grow_in};
use arbrume_metrics::{Features, GLOBAL_FEATURES, LOCAL_FEATURES, features};
use clap::Args;
use rayon::prelude::*;
use serde_json::{Map, Value, json};

use crate::error::CliError;

/// Arguments of `arbrume metrics`.
#[derive(Debug, Args)]
pub struct MetricsArgs {
    /// Directories or `.ann.npz` files holding skeletons.
    #[arg(required_unless_present = "presets")]
    paths: Vec<PathBuf>,
    /// Grow plants of these presets instead (comma-separated, or `all`).
    #[arg(long, value_delimiter = ',', conflicts_with = "paths")]
    presets: Vec<String>,
    /// Plants per preset.
    #[arg(long, default_value_t = 64)]
    seeds: u32,
    /// Base seed of the preset plants.
    #[arg(long, default_value_t = 1)]
    seed: u64,
    /// Age of the preset plants [default: middle of the preset's age range].
    #[arg(long)]
    age: Option<u16>,
    /// Print every feature as JSON instead of the table.
    #[arg(long)]
    json: bool,
}

/// Columns of the table: feature, whether it is global, heading.
const COLUMNS: [(&str, bool, &str); 10] = [
    ("sibling_angle", false, "sibling angle deg"),
    ("segment_volume", false, "segment volume m3"),
    ("chain_length", false, "chain length m"),
    ("parent_length_ratio", false, "length ratio"),
    ("chain_slope", false, "slope deg"),
    ("chain_tapering", false, "tapering"),
    ("chain_straightness", false, "straightness"),
    ("asymmetry_tips", false, "tip asymmetry"),
    ("tip_count", true, "tips"),
    ("age", true, "age"),
];

/// Runs `arbrume metrics`.
pub fn run(args: &MetricsArgs) -> Result<(), CliError> {
    let groups = if args.presets.is_empty() {
        from_paths(&args.paths)?
    } else {
        from_presets(args)?
    };
    if args.json {
        let all: Map<String, Value> = groups
            .iter()
            .map(|(label, plants)| (label.clone(), group_json(plants)))
            .collect();
        println!("{}", Value::Object(all));
        return Ok(());
    }
    let headings: Vec<&str> = COLUMNS.iter().map(|(_, _, heading)| *heading).collect();
    println!("| species | plants | {} |", headings.join(" | "));
    println!("|---|---|{}", "---|".repeat(COLUMNS.len()));
    for (label, plants) in &groups {
        let cells: Vec<String> = COLUMNS
            .iter()
            .map(|(name, global, _)| {
                let (mean, sd) = spread(plants.iter().map(|f| value(f, name, *global)));
                format!("{} ± {}", significant(mean), significant(sd))
            })
            .collect();
        println!("| {label} | {} | {} |", plants.len(), cells.join(" | "));
    }
    Ok(())
}

/// Per-plant value of a feature: the mean of a local one, the value of a global one.
fn value(features: &Features, name: &str, global: bool) -> f64 {
    if global {
        features.global.get(name).copied().unwrap_or(0.0)
    } else {
        features.local.get(name).map_or(0.0, |summary| summary.mean)
    }
}

/// Mean and population standard deviation.
fn spread(values: impl Iterator<Item = f64>) -> (f64, f64) {
    let values: Vec<f64> = values.collect();
    if values.is_empty() {
        return (0.0, 0.0);
    }
    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n;
    let var = values.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / n;
    (mean, var.sqrt())
}

/// Three significant digits, in scientific notation outside `[0.01, 10000)`.
fn significant(value: f64) -> String {
    let magnitude = value.abs();
    if magnitude == 0.0 {
        "0".to_owned()
    } else if !(0.01..10_000.0).contains(&magnitude) {
        format!("{value:.2e}")
    } else {
        let decimals = (2 - magnitude.log10().floor() as i32).max(0) as usize;
        format!("{value:.decimals$}")
    }
}

fn group_json(plants: &[Features]) -> Value {
    let summarise = |name: &str, global: bool| {
        let (mean, sd) = spread(plants.iter().map(|f| value(f, name, global)));
        json!({ "mean": mean, "sd": sd })
    };
    let local: Map<String, Value> = LOCAL_FEATURES
        .iter()
        .map(|name| ((*name).to_owned(), summarise(name, false)))
        .collect();
    let global: Map<String, Value> = GLOBAL_FEATURES
        .iter()
        .map(|name| ((*name).to_owned(), summarise(name, true)))
        .collect();
    json!({ "plants": plants.len(), "local": local, "global": global })
}

/// Features of every sidecar under `paths`, grouped by label.
fn from_paths(paths: &[PathBuf]) -> Result<BTreeMap<String, Vec<Features>>, CliError> {
    let mut files = Vec::new();
    for path in paths {
        if path.is_dir() {
            let entries = std::fs::read_dir(path)
                .map_err(|e| CliError::usage(format!("{}: {e}", path.display())))?;
            let mut found: Vec<PathBuf> = entries
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .filter(|file| file.to_string_lossy().ends_with(".ann.npz"))
                .collect();
            found.sort();
            files.extend(found);
        } else {
            files.push(path.clone());
        }
    }
    let results: Vec<Result<(String, Features), CliError>> =
        files.par_iter().map(|file| read_one(file)).collect();
    let mut groups: BTreeMap<String, Vec<Features>> = BTreeMap::new();
    for result in results {
        let (label, features) = result?;
        groups.entry(label).or_default().push(features);
    }
    Ok(groups)
}

/// Label and features of the sidecar `file`.
fn read_one(file: &Path) -> Result<(String, Features), CliError> {
    let fail = |e: &dyn std::fmt::Display| CliError::failed(format!("{}: {e}", file.display()));
    let bytes = std::fs::read(file).map_err(|e| fail(&e))?;
    let annotations = Annotations::read_npz(&bytes).map_err(|e| fail(&e))?;
    let skeleton = annotations
        .skeleton
        .ok_or_else(|| fail(&"no skeleton in this sidecar"))?;
    let graph = skeleton.to_graph().map_err(|e| fail(&e))?;
    let name = file.file_name().unwrap_or_default().to_string_lossy();
    let stem = name.trim_end_matches(".ann.npz");
    let label = stem.rsplitn(3, '_').nth(2).unwrap_or(stem).to_owned();
    Ok((label, features(&graph)))
}

/// Features of the preset plants of `args`, grouped by preset.
fn from_presets(args: &MetricsArgs) -> Result<BTreeMap<String, Vec<Features>>, CliError> {
    let names: Vec<String> = if args.presets.iter().any(|name| name == "all") {
        presets()
            .iter()
            .map(|(name, _)| (*name).to_owned())
            .collect()
    } else {
        args.presets.clone()
    };
    let mut groups = BTreeMap::new();
    for name in names {
        let species: Species =
            preset(&name).ok_or_else(|| CliError::usage(format!("no preset `{name}`")))?;
        let [young, old] = species.variation.age;
        let age = args.age.unwrap_or(u16::midpoint(young, old));
        let open = Surroundings::none();
        let plants: Vec<Features> = (0..args.seeds)
            .into_par_iter()
            .map(|index| {
                let params = GrowParams {
                    seed: plant_seed(args.seed, index),
                    age,
                    scale: 1.0,
                };
                features(&grow_in(&species, &params, &open))
            })
            .collect();
        groups.insert(format!("{name} (age {age})"), plants);
    }
    Ok(groups)
}

#[cfg(test)]
mod tests {
    use super::{significant, spread};

    #[test]
    fn numbers_print_with_three_significant_digits() {
        assert_eq!(significant(123.456), "123");
        assert_eq!(significant(1.23456), "1.23");
        assert_eq!(significant(0.012_345_6), "0.0123");
        assert_eq!(significant(0.000_123), "1.23e-4");
        assert_eq!(significant(0.0), "0");
        assert_eq!(spread([1.0, 3.0].into_iter()), (2.0, 1.0));
    }
}

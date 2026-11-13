//! The batch to generate: from the `gen` flags or from a batch config file.
//!
//! A config file lists species with their counts and parameter distributions:
//!
//! ```toml
//! out = "dataset"                 # output directory, `--out` takes precedence
//! seed = 42                       # base seed of every entry that sets none
//! quality = 1.0                   # mesh quality of every entry that sets none
//! lods = 1                        # levels of detail per plant
//! manifest = true                 # write manifest.jsonl, like `--manifest`
//! age = { uniform = [10, 30] }    # default age distribution
//! scale = 1.0                     # default scale distribution
//!
//! [[species]]
//! preset = "broadleaf"            # or `path = "oak.toml"`, relative to this file
//! count = 100
//!
//! [[species]]
//! path = "oak.toml"
//! name = "oak_tall"               # label in file names, default the preset or file stem
//! count = 50
//! age = { normal = [25, 5], min = 5, max = 40 }
//! scale = { uniform = [0.8, 1.2] }
//! overrides = { "growth.axis_length" = 18.0 }
//! ```

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use arbrume_core::species::Species;
use arbrume_gen::{Distribution, GenParams, Sampler, species_with_overrides};
use serde::Deserialize;

use crate::error::CliError;
use crate::source::{SpeciesSource, resolve, sanitize};

/// One species of a batch.
#[derive(Debug, Clone)]
pub struct Entry {
    /// Label in file names and in the manifest, unique within the batch.
    pub label: String,
    /// The species, overrides applied.
    pub species: Species,
    /// Number of plants, indexed `0..count`.
    pub count: u32,
    /// Parameters of each plant.
    pub sampler: Sampler,
}

/// Everything to generate.
#[derive(Debug, Clone)]
pub struct Batch {
    /// Output directory from the config file.
    pub out: Option<PathBuf>,
    /// Levels of detail per plant, at least 1.
    pub lods: u32,
    /// Whether the config asks for a manifest.
    pub manifest: bool,
    /// The species to generate.
    pub entries: Vec<Entry>,
}

/// Top level of a batch config file.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigFile {
    out: Option<PathBuf>,
    seed: Option<u64>,
    quality: Option<f32>,
    lods: Option<u32>,
    #[serde(default)]
    manifest: bool,
    age: Option<Distribution>,
    scale: Option<Distribution>,
    species: Vec<EntryFile>,
}

/// One `[[species]]` entry of a batch config file.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EntryFile {
    preset: Option<String>,
    path: Option<String>,
    name: Option<String>,
    count: u32,
    seed: Option<u64>,
    quality: Option<f32>,
    age: Option<Distribution>,
    scale: Option<Distribution>,
    #[serde(default)]
    overrides: toml::Table,
}

/// Defaults applied to the entries of a batch.
#[derive(Debug, Clone, Copy)]
pub struct Defaults {
    /// Base seed.
    pub seed: u64,
    /// Mesh quality.
    pub quality: f32,
    /// Age distribution.
    pub age: Distribution,
    /// Scale distribution.
    pub scale: Distribution,
}

impl Default for Defaults {
    /// Seed 0, quality 1, the default age of `GenParams`, scale 1.
    fn default() -> Self {
        Self {
            seed: 0,
            quality: 1.0,
            age: Distribution::Fixed(f32::from(GenParams::default().age)),
            scale: Distribution::Fixed(1.0),
        }
    }
}

/// Builds the entry of `source` and checks its sampler.
pub fn entry(
    source: &SpeciesSource,
    overrides: &toml::Table,
    count: u32,
    sampler: Sampler,
) -> Result<Entry, CliError> {
    let species = species_with_overrides(&source.text, overrides)
        .map_err(|error| CliError::usage(format!("species `{}`: {error}", source.label)))?;
    sampler
        .validate()
        .map_err(|error| CliError::usage(format!("species `{}`: {error}", source.label)))?;
    Ok(Entry {
        label: source.label.clone(),
        species,
        count,
        sampler,
    })
}

/// Reads the batch config file at `path`.
pub fn load(path: &Path) -> Result<Batch, CliError> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| CliError::usage(format!("{}: {error}", path.display())))?;
    let file: ConfigFile = toml::from_str(&text)
        .map_err(|error| CliError::usage(format!("{}: {error}", path.display())))?;
    let base = path.parent().unwrap_or_else(|| Path::new(""));
    let fallback = Defaults::default();
    let defaults = Defaults {
        seed: file.seed.unwrap_or(fallback.seed),
        quality: file.quality.unwrap_or(fallback.quality),
        age: file.age.unwrap_or(fallback.age),
        scale: file.scale.unwrap_or(fallback.scale),
    };
    let entries = file
        .species
        .iter()
        .map(|entry| load_entry(entry, base, &defaults))
        .collect::<Result<Vec<_>, _>>()?;
    let lods = file.lods.unwrap_or(1);
    check_batch(&entries, lods)?;
    Ok(Batch {
        out: file.out.map(|out| base.join(out)),
        lods,
        manifest: file.manifest,
        entries,
    })
}

fn load_entry(file: &EntryFile, base: &Path, defaults: &Defaults) -> Result<Entry, CliError> {
    let spec = match (&file.preset, &file.path) {
        (Some(preset), None) => preset,
        (None, Some(path)) => path,
        _ => {
            return Err(CliError::usage(
                "each [[species]] needs one of `preset` or `path`",
            ));
        }
    };
    if file.preset.is_some() && arbrume_gen::preset_toml(spec).is_none() {
        return Err(CliError::usage(format!("no preset `{spec}`")));
    }
    let mut source = resolve(spec, base)?;
    if let Some(name) = &file.name {
        source.label = sanitize(name);
    }
    let sampler = Sampler {
        seed: file.seed.unwrap_or(defaults.seed),
        age: file.age.unwrap_or(defaults.age),
        scale: file.scale.unwrap_or(defaults.scale),
        quality: file.quality.unwrap_or(defaults.quality),
    };
    entry(&source, &file.overrides, file.count, sampler)
}

/// Checks that labels are unique and that `lods` is at least 1.
pub fn check_batch(entries: &[Entry], lods: u32) -> Result<(), CliError> {
    if lods == 0 {
        return Err(CliError::usage("lods must be at least 1"));
    }
    let mut labels = BTreeSet::new();
    for entry in entries {
        if !labels.insert(entry.label.as_str()) {
            return Err(CliError::usage(format!(
                "two species are labelled `{}`; set `name` on one of them",
                entry.label
            )));
        }
    }
    Ok(())
}

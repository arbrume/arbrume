//! `arbrume gen`: generate a batch of plants and write them as `.glb` files.
//!
//! Each worker thread generates a plant, writes its files and its manifest line, then
//! reuses its buffers for the next plant, so memory stays bounded by one plant per
//! thread. A file is written under a temporary name and renamed when complete: a file
//! with its final name is always whole, which is what `--resume` relies on.

use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use arbrume_export::{GlbOptions, write_glb};
use arbrume_gen::{Distribution, GENERATOR, Generator, Plant, Sampler, Stats, metadata, run_batch};
use clap::Args;
use serde::Serialize;
use serde_json::Value;

use crate::config::{self, Batch, Defaults, Entry};
use crate::error::CliError;
use crate::progress::Progress;
use crate::source::resolve;

/// Name of the manifest file in the output directory.
pub const MANIFEST: &str = "manifest.jsonl";

/// Arguments of `arbrume gen`.
#[derive(Debug, Args)]
pub struct GenArgs {
    /// Preset name or path to a species TOML file.
    #[arg(long, required_unless_present = "config", conflicts_with = "config")]
    species: Option<String>,
    /// Number of plants, indexed from 0.
    #[arg(long, default_value_t = 1, conflicts_with = "config")]
    count: u32,
    /// Base seed; the seed of plant i is derived from (seed, i).
    #[arg(long, default_value_t = 0, conflicts_with = "config")]
    seed: u64,
    /// Age in growth cycles [default: 10].
    #[arg(long, conflicts_with_all = ["config", "age_range"])]
    age: Option<u16>,
    /// Age drawn uniformly per plant from the integers lo..hi, both included.
    #[arg(long, value_name = "LO..HI", value_parser = parse_age_range, conflicts_with = "config")]
    age_range: Option<Distribution>,
    /// Scale factor, or lo..hi for a uniform draw per plant [default: 1].
    #[arg(long, value_parser = parse_scale, conflicts_with = "config")]
    scale: Option<Distribution>,
    /// Mesh quality, 1 being full detail [default: 1].
    #[arg(long, conflicts_with = "config")]
    quality: Option<f32>,
    /// Levels of detail per plant; level k is meshed at quality * 0.5^k [default: 1].
    #[arg(long, conflicts_with = "config")]
    lods: Option<u32>,
    /// Batch config file listing species, counts and distributions.
    #[arg(long)]
    config: Option<PathBuf>,
    /// Output directory, created if missing (required unless the config sets `out`).
    #[arg(long)]
    out: Option<PathBuf>,
    /// Worker threads [default: all cores, or `RAYON_NUM_THREADS`].
    #[arg(long, short)]
    jobs: Option<usize>,
    /// Skip plants whose files (and manifest line, with --manifest) already exist.
    #[arg(long)]
    resume: bool,
    /// Write one JSON line per plant to manifest.jsonl in the output directory.
    #[arg(long)]
    manifest: bool,
    /// Print nothing but errors.
    #[arg(long, short)]
    quiet: bool,
}

/// Parses `lo..hi` into a uniform distribution over integers.
fn parse_age_range(text: &str) -> Result<Distribution, String> {
    let (lo, hi) = text
        .split_once("..")
        .ok_or("expected LO..HI, such as 10..30")?;
    let lo: u16 = lo.trim().parse().map_err(|e| format!("`{lo}`: {e}"))?;
    let hi: u16 = hi.trim().parse().map_err(|e| format!("`{hi}`: {e}"))?;
    if lo > hi {
        return Err(format!("{lo} is above {hi}"));
    }
    Ok(Distribution::Uniform {
        lo: f32::from(lo),
        hi: f32::from(hi),
    })
}

/// Parses `x` into a fixed value or `lo..hi` into a uniform distribution.
fn parse_scale(text: &str) -> Result<Distribution, String> {
    let number = |s: &str| s.trim().parse::<f32>().map_err(|e| format!("`{s}`: {e}"));
    let distribution = match text.split_once("..") {
        Some((lo, hi)) => Distribution::Uniform {
            lo: number(lo)?,
            hi: number(hi)?,
        },
        None => Distribution::Fixed(number(text)?),
    };
    distribution.validate()?;
    Ok(distribution)
}

/// The batch of the `--species` flags.
fn batch_from_flags(args: &GenArgs) -> Result<Batch, CliError> {
    let spec = args
        .species
        .as_deref()
        .ok_or_else(|| CliError::usage("--species or --config is required"))?;
    let source = resolve(spec, Path::new(""))?;
    let defaults = Defaults::default();
    let age = match (args.age, args.age_range) {
        (Some(age), _) => Distribution::Fixed(f32::from(age)),
        (None, Some(range)) => range,
        (None, None) => defaults.age,
    };
    let sampler = Sampler {
        seed: args.seed,
        age,
        scale: args.scale.unwrap_or(defaults.scale),
        quality: args.quality.unwrap_or(defaults.quality),
    };
    let entry = config::entry(&source, &toml::Table::new(), args.count, sampler)?;
    let lods = args.lods.unwrap_or(1);
    config::check_batch(std::slice::from_ref(&entry), lods)?;
    Ok(Batch {
        out: None,
        lods,
        manifest: false,
        entries: vec![entry],
    })
}

/// Runs `arbrume gen`.
pub fn run(args: &GenArgs) -> Result<(), CliError> {
    let batch = match &args.config {
        Some(path) => config::load(path)?,
        None => batch_from_flags(args)?,
    };
    let out = args
        .out
        .clone()
        .or_else(|| batch.out.clone())
        .ok_or_else(|| CliError::usage("--out is required"))?;
    if args.jobs == Some(0) {
        return Err(CliError::usage("--jobs must be at least 1"));
    }
    let options = RunOptions {
        out,
        jobs: args.jobs,
        resume: args.resume,
        manifest: args.manifest || batch.manifest,
        quiet: args.quiet,
    };
    execute(&batch, &options)
}

/// Options of a run, from the command line.
#[derive(Debug)]
struct RunOptions {
    out: PathBuf,
    jobs: Option<usize>,
    resume: bool,
    manifest: bool,
    quiet: bool,
}

/// What the workers share.
#[derive(Debug)]
struct Context<'a> {
    out: &'a Path,
    lods: u32,
    manifest: Option<Mutex<File>>,
    progress: Progress,
}

/// Generates every plant of `batch` that is not already complete.
fn execute(batch: &Batch, options: &RunOptions) -> Result<(), CliError> {
    let out = options.out.as_path();
    fs::create_dir_all(out)
        .map_err(|e| CliError::failed(format!("cannot create {}: {e}", out.display())))?;
    let manifest_path = out.join(MANIFEST);
    let recorded = if options.resume && options.manifest {
        read_manifest(&manifest_path)?
    } else {
        BTreeSet::new()
    };
    let mut tasks = Vec::new();
    let mut skipped = 0;
    for (slot, entry) in batch.entries.iter().enumerate() {
        for index in 0..entry.count {
            let done = options.resume
                && (!options.manifest || recorded.contains(&key(entry, index)))
                && files_exist(out, entry, index, batch.lods);
            if done {
                skipped += 1;
            } else {
                tasks.push((slot, index));
            }
        }
    }
    let manifest = options
        .manifest
        .then(|| open_manifest(&manifest_path, options.resume))
        .transpose()?
        .map(Mutex::new);
    let context = Context {
        out,
        lods: batch.lods,
        manifest,
        progress: Progress::new(tasks.len(), skipped, options.quiet),
    };
    let work = || {
        run_batch(tasks.len(), |generator, plant, task| {
            let (slot, index) = tasks[task];
            let entry = &batch.entries[slot];
            let result = make_plant(&context, generator, plant, entry, index);
            context.progress.tick(result.is_ok());
            result
        })
    };
    let results = match options.jobs {
        Some(jobs) => thread_pool(jobs)?.install(work),
        None => work(),
    };
    context.progress.finish();
    report(&results, skipped, &context, options.quiet)
}

/// Prints the failures and a summary; fails if any plant failed.
fn report(
    results: &[Result<(), String>],
    skipped: usize,
    context: &Context,
    quiet: bool,
) -> Result<(), CliError> {
    let failures: Vec<&String> = results.iter().filter_map(|r| r.as_ref().err()).collect();
    for failure in &failures {
        eprintln!("error: {failure}");
    }
    if !failures.is_empty() {
        return Err(CliError::failed(format!(
            "{} of {} plants failed",
            failures.len(),
            results.len()
        )));
    }
    if !quiet {
        eprintln!(
            "{} plants written to {} in {:.1} s ({skipped} already present)",
            results.len(),
            context.out.display(),
            context.progress.elapsed()
        );
    }
    Ok(())
}

fn thread_pool(jobs: usize) -> Result<rayon::ThreadPool, CliError> {
    rayon::ThreadPoolBuilder::new()
        .num_threads(jobs)
        .build()
        .map_err(|e| CliError::failed(format!("cannot start {jobs} threads: {e}")))
}

/// Identifies a plant in the manifest: label, base seed, index.
type Key = (String, u64, u32);

fn key(entry: &Entry, index: u32) -> Key {
    (entry.label.clone(), entry.sampler.seed, index)
}

/// Quality of level of detail `lod`: each level halves the quality of the previous one.
fn lod_quality(quality: f32, lod: u32) -> f32 {
    quality * 0.5_f32.powi(lod.cast_signed())
}

/// File name of level `lod` of plant `index` of `entry`.
fn file_name(entry: &Entry, index: u32, lod: u32, lods: u32) -> String {
    let stem = format!("{}_{}_{index}", entry.label, entry.sampler.seed);
    if lods > 1 {
        format!("{stem}_lod{lod}.glb")
    } else {
        format!("{stem}.glb")
    }
}

fn files_exist(out: &Path, entry: &Entry, index: u32, lods: u32) -> bool {
    (0..lods).all(|lod| out.join(file_name(entry, index, lod, lods)).is_file())
}

/// One written file, as recorded in the manifest.
#[derive(Debug, Serialize)]
struct FileRecord {
    file: String,
    lod: u32,
    quality: f32,
    vertices: usize,
    triangles: usize,
    bytes: u64,
}

/// Wall-clock milliseconds of each stage, summed over the levels of detail.
#[derive(Debug, Serialize)]
struct TimingsMs {
    grow: f64,
    mesh: f64,
    export: f64,
}

/// One line of the manifest.
#[derive(Debug, Serialize)]
struct Record<'a> {
    species: &'a str,
    species_name: &'a str,
    index: u32,
    seed: u64,
    plant_seed: u64,
    age: u16,
    scale: f32,
    quality: f32,
    files: Vec<FileRecord>,
    stats: Stats,
    timings_ms: TimingsMs,
}

/// Generates plant `index` of `entry`, writes its files and its manifest line.
fn make_plant(
    context: &Context,
    generator: &mut Generator,
    plant: &mut Plant,
    entry: &Entry,
    index: u32,
) -> Result<(), String> {
    let fail = |e: &dyn std::fmt::Display| format!("{} plant {index}: {e}", entry.label);
    let params = entry.sampler.params(index);
    let timings = generator
        .generate_into(&entry.species, &params, plant)
        .map_err(|e| fail(&e))?;
    let stats = plant.stats;
    let mut mesh = timings.mesh;
    let mut export = Duration::ZERO;
    let mut files = Vec::new();
    for lod in 0..context.lods {
        let quality = lod_quality(entry.sampler.quality, lod);
        if lod > 0 {
            let options = params.with_quality(quality).mesh;
            mesh += generator
                .remesh(plant, &entry.species, &options)
                .map_err(|e| fail(&e))?;
        }
        let name = file_name(entry, index, lod, context.lods);
        let start = Instant::now();
        let bytes = write_file(context.out, &name, entry, plant, (index, lod, quality))
            .map_err(|e| fail(&e))?;
        export += start.elapsed();
        files.push(FileRecord {
            file: name,
            lod,
            quality,
            vertices: plant.stats.vertices,
            triangles: plant.stats.triangles,
            bytes,
        });
    }
    let Some(manifest) = &context.manifest else {
        return Ok(());
    };
    let record = Record {
        species: &entry.label,
        species_name: &entry.species.name,
        index,
        seed: entry.sampler.seed,
        plant_seed: params.seed,
        age: params.age,
        scale: params.scale,
        quality: entry.sampler.quality,
        files,
        stats,
        timings_ms: TimingsMs {
            grow: ms(timings.grow),
            mesh: ms(mesh),
            export: ms(export),
        },
    };
    append_line(manifest, &record).map_err(|e| fail(&e))
}

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1e3
}

/// Writes `plant` to `out/name` through a temporary file; returns the file size.
/// `(index, lod, quality)` go to the metadata.
fn write_file(
    out: &Path,
    name: &str,
    entry: &Entry,
    plant: &Plant,
    (index, lod, quality): (u32, u32, f32),
) -> Result<u64, String> {
    let mut extras = metadata(&entry.species, plant);
    if let Value::Object(map) = &mut extras {
        map.insert("label".to_owned(), entry.label.clone().into());
        map.insert("base_seed".to_owned(), entry.sampler.seed.into());
        map.insert("index".to_owned(), index.into());
        map.insert("lod".to_owned(), lod.into());
        map.insert("quality".to_owned(), quality.into());
    }
    let options = GlbOptions {
        name: name.trim_end_matches(".glb").to_owned(),
        generator: GENERATOR.to_owned(),
        extras: Some(extras),
    };
    let path = out.join(name);
    let temp = out.join(format!("{name}.tmp"));
    let file = File::create(&temp).map_err(|e| format!("{}: {e}", temp.display()))?;
    let mut writer = BufWriter::with_capacity(1 << 16, file);
    write_glb(&plant.mesh, &options, &mut writer).map_err(|e| e.to_string())?;
    let file = writer.into_inner().map_err(|e| e.error().to_string())?;
    let bytes = file.metadata().map_err(|e| e.to_string())?.len();
    drop(file);
    fs::rename(&temp, &path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(bytes)
}

/// Appends one JSON line in a single write.
fn append_line(manifest: &Mutex<File>, record: &Record) -> Result<(), String> {
    let mut line = serde_json::to_vec(record).map_err(|e| e.to_string())?;
    line.push(b'\n');
    let mut file = manifest.lock().map_err(|e| e.to_string())?;
    file.write_all(&line).map_err(|e| e.to_string())
}

/// Opens the manifest for appending (`resume`) or truncates it.
fn open_manifest(path: &Path, resume: bool) -> Result<File, CliError> {
    let mut options = OpenOptions::new();
    if resume {
        options.append(true).create(true);
    } else {
        options.write(true).create(true).truncate(true);
    }
    options
        .open(path)
        .map_err(|e| CliError::failed(format!("{}: {e}", path.display())))
}

/// Keys of the plants recorded in the manifest at `path`. A last line cut short by an
/// interrupted run is removed from the file.
fn read_manifest(path: &Path) -> Result<BTreeSet<Key>, CliError> {
    let fail = |e: &dyn std::fmt::Display| CliError::failed(format!("{}: {e}", path.display()));
    let mut text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(e) => return Err(fail(&e)),
    };
    if !text.is_empty() && !text.ends_with('\n') {
        text.truncate(text.rfind('\n').map_or(0, |end| end + 1));
        fs::write(path, &text).map_err(|e| fail(&e))?;
    }
    let mut keys = BTreeSet::new();
    for (number, line) in text.lines().enumerate() {
        let value: Value =
            serde_json::from_str(line).map_err(|e| fail(&format!("line {}: {e}", number + 1)))?;
        let species = value["species"].as_str().map(str::to_owned);
        let seed = value["seed"].as_u64();
        let index = value["index"].as_u64().and_then(|i| u32::try_from(i).ok());
        if let (Some(species), Some(seed), Some(index)) = (species, seed, index) {
            keys.insert((species, seed, index));
        }
    }
    Ok(keys)
}

#[cfg(test)]
mod tests {
    use super::{lod_quality, parse_age_range, parse_scale};
    use arbrume_gen::Distribution;

    #[test]
    fn ranges_parse() {
        assert_eq!(
            parse_age_range("5..9").unwrap(),
            Distribution::Uniform { lo: 5.0, hi: 9.0 }
        );
        assert!(parse_age_range("9..5").is_err());
        assert!(parse_age_range("9").is_err());
        assert_eq!(parse_scale("1.5").unwrap(), Distribution::Fixed(1.5));
        assert_eq!(
            parse_scale("0.5..2").unwrap(),
            Distribution::Uniform { lo: 0.5, hi: 2.0 }
        );
        assert!(parse_scale("2..1").is_err());
    }

    #[test]
    fn lod_quality_halves() {
        assert_eq!(lod_quality(1.0, 0), 1.0);
        assert_eq!(lod_quality(0.8, 2), 0.2);
    }
}

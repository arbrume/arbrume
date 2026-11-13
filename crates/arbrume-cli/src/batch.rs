//! `arbrume gen`: generate a batch of plants and write them as `.glb` files.
//!
//! Each worker thread generates a plant, writes its files and its manifest line, then
//! reuses its buffers for the next plant, so memory stays bounded by one plant per
//! thread. A file is written under a temporary name and renamed when complete: a file
//! with its final name is always whole, which is what `--resume` relies on.
//!
//! A batch is either a list of species entries, each with its own sampler of ages and
//! scales, or (`--random`) plants drawn by a `RandomSampler`, each of a random preset.
//! Both resolve a plant to a [`Job`]: a label, a species and its parameters.

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use arbrume_core::species::Species;
use arbrume_export::{GlbOptions, write_glb};
use arbrume_gen::{Budget, Distribution, GENERATOR, GenParams, Generator, Plant};
use arbrume_gen::{RandomSampler, Sampler};
use arbrume_gen::{Stats, metadata, run_batch};
use arbrume_raster::{Renderer, ThumbnailOptions};
use clap::{Args, ValueEnum};
use serde::Serialize;
use serde_json::Value;

use crate::config::{self, Batch, Defaults, Entry, Thumb};
use crate::error::CliError;
use crate::progress::Progress;
use crate::source::resolve;

/// Name of the manifest file in the output directory.
pub const MANIFEST: &str = "manifest.jsonl";

/// Arguments of `arbrume gen`.
#[derive(Debug, Args)]
pub struct GenArgs {
    /// Preset name or path to a species TOML file.
    #[arg(long, required_unless_present_any = ["config", "random"], conflicts_with = "config")]
    species: Option<String>,
    /// Random plants.
    #[command(flatten)]
    random: RandomArgs,
    /// Write a thumbnail next to each plant, SIZE pixels wide and high.
    #[arg(long, value_name = "SIZE", num_args = 0..=1, default_missing_value = "256")]
    thumb: Option<u32>,
    /// File format of the thumbnails.
    #[arg(long, value_enum, default_value_t = ThumbFormat::Jpg)]
    thumb_format: ThumbFormat,
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
    /// Mesh quality, 1 being full detail [default: 1, or 0.6 with --random].
    #[arg(long, conflicts_with = "config")]
    quality: Option<f32>,
    /// Levels of detail per plant; level k is meshed at quality * 0.5^k [default: 1].
    #[arg(long, conflicts_with = "config")]
    lods: Option<u32>,
    /// Surroundings file: envelope, obstacles and neighbours around each plant.
    #[arg(long, value_name = "FILE", conflicts_with = "config")]
    surroundings: Option<PathBuf>,
    /// Write each plant as a growth sequence of K frames instead of one file.
    #[arg(long, value_name = "K")]
    growth_frames: Option<u16>,
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

/// The `--random` options of `arbrume gen`.
#[derive(Debug, Args)]
struct RandomArgs {
    /// Draw each plant from a random preset, with varied parameters and age.
    #[arg(long, conflicts_with_all = ["config", "species", "age", "age_range", "scale"])]
    random: bool,
    /// Triangle budget of a random plant; 0 for none [default: 60000].
    #[arg(long, requires = "random")]
    max_triangles: Option<usize>,
    /// Vertex budget of a random plant; 0 for none [default: 55000].
    #[arg(long, requires = "random")]
    max_vertices: Option<usize>,
}

/// File format of a thumbnail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThumbFormat {
    /// JPEG, quality 88.
    Jpg,
    /// PNG.
    Png,
}

impl ThumbFormat {
    /// File extension, without the dot.
    const fn extension(self) -> &'static str {
        match self {
            Self::Jpg => "jpg",
            Self::Png => "png",
        }
    }
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

/// The batch of `--random`.
fn random_batch(args: &GenArgs) -> Result<Batch, CliError> {
    let mut sampler = RandomSampler::new(args.seed);
    if let Some(quality) = args.quality {
        sampler.quality = quality;
    }
    let limit = |flag: Option<usize>, default: Option<usize>| match flag {
        Some(0) => None,
        Some(limit) => Some(limit),
        None => default,
    };
    sampler.budget = Budget {
        triangles: limit(args.random.max_triangles, sampler.budget.triangles),
        vertices: limit(args.random.max_vertices, sampler.budget.vertices),
    };
    sampler.validate().map_err(CliError::usage)?;
    let lods = args.lods.unwrap_or(1);
    config::check_batch(&[], lods)?;
    Ok(Batch {
        out: None,
        lods,
        manifest: false,
        thumb: None,
        growth_frames: 0,
        entries: Vec::new(),
        random: Some((sampler, args.count)),
    })
}

/// The batch of the `--species` flags.
fn batch_from_flags(args: &GenArgs) -> Result<Batch, CliError> {
    if args.random.random {
        return random_batch(args);
    }
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
    let mut entry = config::entry(&source, &toml::Table::new(), args.count, sampler)?;
    if let Some(path) = &args.surroundings {
        entry.surroundings = Some(config::load_surroundings(path)?);
    }
    let lods = args.lods.unwrap_or(1);
    config::check_batch(std::slice::from_ref(&entry), lods)?;
    Ok(Batch {
        out: None,
        lods,
        manifest: false,
        thumb: None,
        growth_frames: 0,
        entries: vec![entry],
        random: None,
    })
}

/// Runs `arbrume gen`.
pub fn run(args: &GenArgs) -> Result<(), CliError> {
    let mut batch = match &args.config {
        Some(path) => config::load(path)?,
        None => batch_from_flags(args)?,
    };
    if let Some(size) = args.thumb {
        batch.thumb = Some(Thumb {
            size,
            format: args.thumb_format,
        });
    }
    if batch.thumb.is_some_and(|thumb| thumb.size == 0) {
        return Err(CliError::usage("the thumbnail size must be at least 1"));
    }
    if let Some(frames) = args.growth_frames {
        batch.growth_frames = frames;
    }
    if batch.growth_frames > 0 && (batch.lods > 1 || batch.random.is_some()) {
        return Err(CliError::usage(
            "a growth sequence cannot be combined with --lods or --random",
        ));
    }
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

/// One plant to generate: where it comes from in the batch and its index.
#[derive(Debug, Clone, Copy)]
struct Task {
    /// Entry of the batch; `None` for a plant of the random sampler.
    slot: Option<usize>,
    index: u32,
}

/// A plant resolved to what generating it needs.
#[derive(Debug)]
struct Job<'a> {
    /// Label in file names and in the manifest.
    label: Cow<'a, str>,
    species: Cow<'a, Species>,
    params: GenParams,
    base_seed: u64,
    index: u32,
    budget: Budget,
    /// Whether the plant comes from the random sampler.
    random: bool,
}

impl Batch {
    /// Every plant of the batch.
    fn tasks(&self) -> Vec<Task> {
        let mut tasks = Vec::new();
        for (slot, entry) in self.entries.iter().enumerate() {
            let slot = Some(slot);
            tasks.extend((0..entry.count).map(|index| Task { slot, index }));
        }
        if let Some((_, count)) = &self.random {
            tasks.extend((0..*count).map(|index| Task { slot: None, index }));
        }
        tasks
    }

    /// Resolves `task`.
    fn job(&self, task: Task) -> Result<Job<'_>, String> {
        let index = task.index;
        if let Some(slot) = task.slot {
            let entry: &Entry = &self.entries[slot];
            let mut params = entry.sampler.params(index);
            params.surroundings.clone_from(&entry.surroundings);
            return Ok(Job {
                label: Cow::Borrowed(&entry.label),
                species: Cow::Borrowed(&entry.species),
                params,
                base_seed: entry.sampler.seed,
                index,
                budget: Budget::NONE,
                random: false,
            });
        }
        let (sampler, _) = self
            .random
            .as_ref()
            .ok_or("the batch has no random plants")?;
        let pick = sampler
            .pick(index)
            .map_err(|e| format!("random plant {index}: {e}"))?;
        Ok(Job {
            label: Cow::Owned(pick.preset),
            species: Cow::Owned(pick.species),
            params: pick.params,
            base_seed: sampler.seed,
            index,
            budget: pick.budget,
            random: true,
        })
    }
}

/// What the workers share.
#[derive(Debug)]
struct Context<'a> {
    out: &'a Path,
    lods: u32,
    /// Frames of the growth sequence of each plant; 0 writes the plant only.
    frames: u16,
    thumb: Option<Thumb>,
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
    let mut tasks = batch.tasks();
    let total = tasks.len();
    if options.resume {
        let mut pending = Vec::new();
        for task in tasks {
            let job = batch.job(task).map_err(CliError::failed)?;
            let done = (!options.manifest || recorded.contains(&key(&job)))
                && files_exist(out, &job, batch);
            if !done {
                pending.push(task);
            }
        }
        tasks = pending;
    }
    let skipped = total - tasks.len();
    let manifest = options
        .manifest
        .then(|| open_manifest(&manifest_path, options.resume))
        .transpose()?
        .map(Mutex::new);
    let context = Context {
        out,
        lods: batch.lods,
        frames: batch.growth_frames,
        thumb: batch.thumb,
        manifest,
        progress: Progress::new(tasks.len(), skipped, options.quiet),
    };
    let work = || {
        run_batch(tasks.len(), |generator, plant, task| {
            let result = batch
                .job(tasks[task])
                .and_then(|job| make_plant(&context, generator, plant, &job));
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

fn key(job: &Job) -> Key {
    (job.label.to_string(), job.base_seed, job.index)
}

/// Quality of the mesh options `params` was meshed with.
fn quality_of(params: &GenParams) -> f32 {
    (arbrume_mesh_tolerance() / params.mesh.tolerance).sqrt()
}

/// Tolerance of full quality.
fn arbrume_mesh_tolerance() -> f32 {
    GenParams::default().mesh.tolerance
}

/// Quality of level of detail `lod`: each level halves the quality of the previous one.
fn lod_quality(quality: f32, lod: u32) -> f32 {
    quality * 0.5_f32.powi(lod.cast_signed())
}

/// File name of the plant of `job` without its extension.
fn stem(job: &Job) -> String {
    format!("{}_{}_{}", job.label, job.base_seed, job.index)
}

/// File name of level `lod` of the plant of `job`.
fn file_name(job: &Job, lod: u32, lods: u32) -> String {
    let stem = stem(job);
    if lods > 1 {
        format!("{stem}_lod{lod}.glb")
    } else {
        format!("{stem}.glb")
    }
}

/// File name of the thumbnail of the plant of `job`.
fn thumb_name(job: &Job, thumb: Thumb) -> String {
    format!("{}.{}", stem(job), thumb.format.extension())
}

/// File name of frame `frame` of the growth sequence of the plant of `job`.
fn frame_name(job: &Job, frame: usize) -> String {
    format!("{}_frame{frame:03}.glb", stem(job))
}

fn files_exist(out: &Path, job: &Job, batch: &Batch) -> bool {
    let thumb = batch
        .thumb
        .is_none_or(|thumb| out.join(thumb_name(job, thumb)).is_file());
    if batch.growth_frames > 0 {
        let frames = usize::from(batch.growth_frames.min(job.params.age));
        return thumb && (0..frames).all(|frame| out.join(frame_name(job, frame)).is_file());
    }
    thumb && (0..batch.lods).all(|lod| out.join(file_name(job, lod, batch.lods)).is_file())
}

/// One written file, as recorded in the manifest.
#[derive(Debug, Serialize)]
struct FileRecord {
    file: String,
    lod: u32,
    /// Last growth cycle shown, for a frame of a growth sequence.
    #[serde(skip_serializing_if = "Option::is_none")]
    cycle: Option<u16>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    thumbnail: Option<String>,
    stats: Stats,
    timings_ms: TimingsMs,
}

/// What generating one plant produced, for its manifest line.
#[derive(Debug)]
struct Written {
    files: Vec<FileRecord>,
    thumbnail: Option<String>,
    stats: Stats,
    quality: f32,
    timings: TimingsMs,
}

/// Generates the plant of `job`, writes its files, its thumbnail and its manifest line.
fn make_plant(
    context: &Context,
    generator: &mut Generator,
    plant: &mut Plant,
    job: &Job,
) -> Result<(), String> {
    let index = job.index;
    let fail = |e: String| format!("{} plant {index}: {e}", job.label);
    let written = if context.frames > 0 {
        write_sequence(context, generator, plant, job)
    } else {
        write_levels(context, generator, plant, job)
    }
    .map_err(fail)?;
    let Some(manifest) = &context.manifest else {
        return Ok(());
    };
    let record = Record {
        species: &job.label,
        species_name: &job.species.name,
        index,
        seed: job.base_seed,
        plant_seed: job.params.seed,
        age: job.params.age,
        scale: job.params.scale,
        quality: written.quality,
        files: written.files,
        thumbnail: written.thumbnail,
        stats: written.stats,
        timings_ms: written.timings,
    };
    append_line(manifest, &record).map_err(fail)
}

/// Generates the plant of `job` and writes one file per level of detail, with the
/// thumbnail of the first.
fn write_levels(
    context: &Context,
    generator: &mut Generator,
    plant: &mut Plant,
    job: &Job,
) -> Result<Written, String> {
    let timings = generator
        .generate_within(&job.species, &job.params, &job.budget, plant)
        .map_err(|e| e.to_string())?;
    let stats = plant.stats;
    let base_quality = quality_of(&plant.params);
    let mut mesh = timings.mesh;
    let mut export = Duration::ZERO;
    let mut files = Vec::new();
    let mut thumbnail = None;
    for lod in 0..context.lods {
        let quality = lod_quality(base_quality, lod);
        if lod > 0 {
            let options = job.params.clone().with_quality(quality).mesh;
            mesh += generator
                .remesh(plant, &job.species, &options)
                .map_err(|e| e.to_string())?;
        }
        let name = file_name(job, lod, context.lods);
        let start = Instant::now();
        let bytes = write_file(context.out, &name, job, plant, (lod, quality))?;
        if let (0, Some(thumb)) = (lod, context.thumb) {
            let name = thumb_name(job, thumb);
            write_thumb(context.out, &name, plant, thumb)?;
            thumbnail = Some(name);
        }
        export += start.elapsed();
        files.push(FileRecord {
            file: name,
            lod,
            cycle: None,
            quality,
            vertices: plant.stats.vertices,
            triangles: plant.stats.triangles,
            bytes,
        });
    }
    Ok(Written {
        files,
        thumbnail,
        stats,
        quality: base_quality,
        timings: TimingsMs {
            grow: ms(timings.grow),
            mesh: ms(mesh),
            export: ms(export),
        },
    })
}

/// Grows the plant of `job` once and writes one file per frame of its growth sequence,
/// with the thumbnail of the last, which is the full-grown plant.
fn write_sequence(
    context: &Context,
    generator: &mut Generator,
    plant: &mut Plant,
    job: &Job,
) -> Result<Written, String> {
    let timeline = generator
        .timeline(&job.species, &job.params)
        .map_err(|e| e.to_string())?;
    let quality = quality_of(&job.params);
    let (mut mesh, mut export) = (Duration::ZERO, Duration::ZERO);
    let mut files = Vec::new();
    for (frame, cycle) in timeline
        .frame_cycles(context.frames)
        .into_iter()
        .enumerate()
    {
        mesh += generator
            .frame(&job.species, &timeline, cycle, plant)
            .map_err(|e| e.to_string())?;
        let name = frame_name(job, frame);
        let start = Instant::now();
        let bytes = write_file(context.out, &name, job, plant, (0, quality))?;
        export += start.elapsed();
        files.push(FileRecord {
            file: name,
            lod: 0,
            cycle: Some(cycle),
            quality,
            vertices: plant.stats.vertices,
            triangles: plant.stats.triangles,
            bytes,
        });
    }
    let mut thumbnail = None;
    if let (Some(thumb), false) = (context.thumb, files.is_empty()) {
        let name = thumb_name(job, thumb);
        let start = Instant::now();
        write_thumb(context.out, &name, plant, thumb)?;
        export += start.elapsed();
        thumbnail = Some(name);
    }
    Ok(Written {
        files,
        thumbnail,
        stats: plant.stats,
        quality,
        timings: TimingsMs {
            grow: ms(timeline.grow),
            mesh: ms(mesh),
            export: ms(export),
        },
    })
}

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1e3
}

/// Renders the thumbnail of `plant` and writes it to `out/name` through a temporary
/// file.
fn write_thumb(out: &Path, name: &str, plant: &Plant, thumb: Thumb) -> Result<(), String> {
    let options = ThumbnailOptions {
        width: thumb.size,
        height: thumb.size,
        azimuth: 20.0,
        ..ThumbnailOptions::default()
    };
    let image = Renderer::new().thumbnail(&plant.mesh, &options);
    let bytes = match thumb.format {
        ThumbFormat::Jpg => image.to_jpeg(88, [255; 3]),
        ThumbFormat::Png => image.to_png(),
    }
    .map_err(|e| e.to_string())?;
    let path = out.join(name);
    let temp = out.join(format!("{name}.tmp"));
    fs::write(&temp, bytes).map_err(|e| format!("{}: {e}", temp.display()))?;
    fs::rename(&temp, &path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Writes `plant` to `out/name` through a temporary file; returns the file size.
/// `(lod, quality)` go to the metadata, with the label, base seed and index of `job`
/// and, for a random plant, the name of its preset.
fn write_file(
    out: &Path,
    name: &str,
    job: &Job,
    plant: &Plant,
    (lod, quality): (u32, f32),
) -> Result<u64, String> {
    let mut extras = metadata(&job.species, plant);
    if let Value::Object(map) = &mut extras {
        map.insert("label".to_owned(), job.label.to_string().into());
        map.insert("base_seed".to_owned(), job.base_seed.into());
        map.insert("index".to_owned(), job.index.into());
        map.insert("lod".to_owned(), lod.into());
        map.insert("quality".to_owned(), quality.into());
        if job.random {
            map.insert("preset".to_owned(), job.label.to_string().into());
        }
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

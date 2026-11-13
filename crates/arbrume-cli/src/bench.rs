//! `arbrume bench`: time each stage of the pipeline and the batch throughput.
//!
//! For every species and age, one plant (base seed 1, index 0) is generated `repeats`
//! times with a warm [`Generator`]; the table gives the median time of growth, meshing
//! and export to memory, with the size of the `.glb`. The throughput is that of a batch
//! of `batch` plants of the species and age (base seed 1, indices `0..batch`), generated
//! and exported to memory by [`run_batch`], on one thread and on every thread of the
//! global pool. Nothing is written to disk.

use std::time::{Duration, Instant};

use arbrume_core::species::Species;
use arbrume_export::{GlbOptions, write_glb};
use arbrume_gen::{
    Distribution, GENERATOR, Generator, Plant, Sampler, metadata, preset, presets, run_batch,
};
use clap::Args;

use crate::error::CliError;
use crate::source::resolve;

/// Arguments of `arbrume bench`.
#[derive(Debug, Args)]
pub struct BenchArgs {
    /// Species to time, presets or files [default: every preset].
    #[arg(long, value_delimiter = ',')]
    species: Vec<String>,
    /// Ages to time.
    #[arg(long, value_delimiter = ',', default_value = "8,20,40")]
    ages: Vec<u16>,
    /// Repetitions of each stage; the median is reported.
    #[arg(long, default_value_t = 5)]
    repeats: usize,
    /// Plants per throughput batch; 0 skips the throughput columns.
    #[arg(long, default_value_t = 32)]
    batch: u32,
    /// Mesh quality.
    #[arg(long, default_value_t = 1.0)]
    quality: f32,
}

/// One row of the table.
#[derive(Debug)]
struct Row {
    nodes: usize,
    triangles: usize,
    grow: Duration,
    mesh: Duration,
    export: Duration,
    bytes: usize,
}

/// Runs `arbrume bench`.
pub fn run(args: &BenchArgs) -> Result<(), CliError> {
    if args.repeats == 0 {
        return Err(CliError::usage("--repeats must be at least 1"));
    }
    let species = selected_species(&args.species)?;
    let threads = rayon::current_num_threads();
    println!(
        "| species | age | nodes | triangles | grow ms | mesh ms | export ms | file MB \
         | trees/s (1 thread) | trees/s ({threads} threads) |"
    );
    println!("|---|---|---|---|---|---|---|---|---|---|");
    let single = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .map_err(CliError::failed)?;
    for (label, species) in &species {
        for &age in &args.ages {
            let sampler = Sampler {
                seed: 1,
                age: Distribution::Fixed(f32::from(age)),
                scale: Distribution::Fixed(1.0),
                quality: args.quality,
            };
            sampler.validate().map_err(CliError::usage)?;
            let row = time_stages(species, &sampler, args.repeats)?;
            let (one, all) = if args.batch == 0 {
                (String::from("-"), String::from("-"))
            } else {
                let one = single.install(|| throughput(species, &sampler, args.batch))?;
                let all = throughput(species, &sampler, args.batch)?;
                (format!("{one:.1}"), format!("{all:.1}"))
            };
            println!(
                "| {label} | {age} | {} | {} | {:.2} | {:.2} | {:.2} | {:.2} | {one} | {all} |",
                row.nodes,
                row.triangles,
                ms(row.grow),
                ms(row.mesh),
                ms(row.export),
                row.bytes as f64 / 1e6
            );
        }
    }
    Ok(())
}

fn selected_species(specs: &[String]) -> Result<Vec<(String, Species)>, CliError> {
    if specs.is_empty() {
        return Ok(presets()
            .iter()
            .filter_map(|(name, _)| preset(name).map(|s| ((*name).to_owned(), s)))
            .collect());
    }
    specs
        .iter()
        .map(|spec| {
            let source = resolve(spec, std::path::Path::new(""))?;
            let species = Species::from_toml_str(&source.text)
                .map_err(|e| CliError::usage(format!("{spec}: {e}")))?;
            Ok((source.label, species))
        })
        .collect()
}

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1e3
}

/// Writes `plant` as a `.glb` with its metadata into `out`, which is cleared first.
fn export(species: &Species, plant: &Plant, out: &mut Vec<u8>) -> Result<(), CliError> {
    out.clear();
    let options = GlbOptions {
        name: "plant".to_owned(),
        generator: GENERATOR.to_owned(),
        extras: Some(metadata(species, plant)),
    };
    write_glb(&plant.mesh, &options, out).map_err(CliError::failed)
}

fn median(mut times: Vec<Duration>) -> Duration {
    times.sort_unstable();
    times[times.len() / 2]
}

/// Median stage times of plant 0 of `sampler` over `repeats` runs.
fn time_stages(species: &Species, sampler: &Sampler, repeats: usize) -> Result<Row, CliError> {
    let params = sampler.params(0);
    let mut generator = Generator::new();
    let mut plant = Plant::default();
    let mut bytes = Vec::new();
    let (mut grow, mut mesh, mut export_times) = (Vec::new(), Vec::new(), Vec::new());
    for _ in 0..repeats {
        let timings = generator
            .generate_into(species, &params, &mut plant)
            .map_err(CliError::failed)?;
        let start = Instant::now();
        export(species, &plant, &mut bytes)?;
        export_times.push(start.elapsed());
        grow.push(timings.grow);
        mesh.push(timings.mesh);
    }
    Ok(Row {
        nodes: plant.stats.nodes,
        triangles: plant.stats.triangles,
        grow: median(grow),
        mesh: median(mesh),
        export: median(export_times),
        bytes: bytes.len(),
    })
}

thread_local! {
    /// Export buffer of each thread, reused from one plant to the next like the file
    /// buffer of `arbrume gen`.
    static BUFFER: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Plants per second of a batch of `count` plants on the current pool.
fn throughput(species: &Species, sampler: &Sampler, count: u32) -> Result<f64, CliError> {
    let start = Instant::now();
    let results = run_batch(count as usize, |generator, plant, index| {
        let params = sampler.params(index as u32);
        generator
            .generate_into(species, &params, plant)
            .map_err(CliError::failed)?;
        BUFFER.with_borrow_mut(|bytes| export(species, plant, bytes))
    });
    let elapsed = start.elapsed().as_secs_f64();
    results.into_iter().collect::<Result<Vec<()>, _>>()?;
    Ok(f64::from(count) / elapsed)
}

//! `arbrume bench`: time each stage of the pipeline and the batch throughput.
//!
//! For every species and age, one plant (base seed 1, index 0) is generated `repeats`
//! times with a warm [`Generator`]; the table gives the median time of growth, meshing
//! and export to memory, with the size of the `.glb`. The throughput is that of a batch
//! of `batch` plants of the species and age (base seed 1, indices `0..batch`), generated
//! and exported to memory by [`run_batch`], on one thread and on every thread of the
//! global pool. Nothing is written to disk. With `--annotations`, the annotations of
//! that list are computed and serialized to memory too, and their time is part of the
//! export column and of the throughput.
//!
//! `--engine` prints another table instead, for the species in the self-organizing mode:
//! the median growth time of the same plant with the default engine options, with a
//! sparse shadow grid, with the buds sensed on the global pool and with the markers
//! given to buds on one thread, then the throughput of a batch of plants grown one per
//! task with the default options and with nothing parallel inside a plant.
//!
//! `--junctions` prints another table instead: the bark triangles of the same plant and
//! the median time to mesh it (bark and leaves, reusing the working memory), with Tier A
//! and with Tier B junctions (docs/junctions.md).

use std::time::{Duration, Instant};

use arbrume_core::mesh::TreeMesh;
use arbrume_core::species::{GrowthMode, Species};
use arbrume_export::{GlbOptions, write_glb};
use arbrume_gen::{
    Distribution, GENERATOR, Generator, Plant, Sampler, metadata, preset, presets, run_batch,
};
use arbrume_grow::{EngineOptions, GridStorage, GrowParams, Surroundings, grow_in_with};
use arbrume_mesh::{Junctions, MeshOptions, Mesher};
use clap::Args;
use rayon::prelude::*;

use crate::annotate::{self, AnnotationSet};
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
    /// Compare the engine options of the self-organizing mode instead.
    #[arg(long)]
    engine: bool,
    /// Compare Tier A and Tier B junctions instead.
    #[arg(long, conflicts_with = "engine")]
    junctions: bool,
    /// Also compute these annotations, as `gen --annotations`.
    #[arg(long, value_name = "LIST")]
    annotations: Option<AnnotationSet>,
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
    if args.engine {
        engine_table(&species, args);
        return Ok(());
    }
    if args.junctions {
        return junction_table(&species, args);
    }
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
            let set = args.annotations.unwrap_or_default();
            let row = time_stages(species, &sampler, args.repeats, &set)?;
            let (one, all) = if args.batch == 0 {
                (String::from("-"), String::from("-"))
            } else {
                let one = single.install(|| throughput(species, &sampler, args.batch, &set))?;
                let all = throughput(species, &sampler, args.batch, &set)?;
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

/// Prints the bark triangles and the median meshing time of plant 0 of each species and
/// age with Tier A and with Tier B junctions.
fn junction_table(species: &[(String, Species)], args: &BenchArgs) -> Result<(), CliError> {
    println!(
        "| species | age | bark triangles A | bark triangles B | mesh ms A | mesh ms B | B / A |"
    );
    println!("|---|---|---|---|---|---|---|");
    let (mut mesher, mut mesh) = (Mesher::new(), TreeMesh::default());
    for (label, species) in species {
        for &age in &args.ages {
            let sampler = Sampler {
                seed: 1,
                age: Distribution::Fixed(f32::from(age)),
                scale: Distribution::Fixed(1.0),
                quality: args.quality,
            };
            sampler.validate().map_err(CliError::usage)?;
            let params = sampler.params(0);
            let plant = Generator::new()
                .generate(species, &params)
                .map_err(CliError::failed)?;
            let mut columns = Vec::with_capacity(2);
            for junctions in [Junctions::TierA, Junctions::TierB] {
                let options = MeshOptions {
                    junctions,
                    ..params.mesh
                };
                let mut times = Vec::with_capacity(args.repeats);
                for _ in 0..args.repeats {
                    let start = Instant::now();
                    mesher
                        .mesh_into(&plant.graph, species, &options, &mut mesh)
                        .map_err(CliError::failed)?;
                    times.push(start.elapsed());
                }
                let counts = mesher
                    .counts(&plant.graph, species, &options)
                    .map_err(CliError::failed)?;
                columns.push((counts.bark_triangles, median(times)));
            }
            let [(tri_a, time_a), (tri_b, time_b)] = [columns[0], columns[1]];
            println!(
                "| {label} | {age} | {tri_a} | {tri_b} | {:.3} | {:.3} | {:.2} |",
                ms(time_a),
                ms(time_b),
                time_b.as_secs_f64() / time_a.as_secs_f64().max(1e-12)
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

/// Median stage times of plant 0 of `sampler` over `repeats` runs; the annotations of
/// `set` count as export.
fn time_stages(
    species: &Species,
    sampler: &Sampler,
    repeats: usize,
    set: &AnnotationSet,
) -> Result<Row, CliError> {
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
        if set.any() {
            let mut sidecar = Vec::new();
            annotate::in_memory(set, &mut generator, &plant, species, &mut sidecar)
                .map_err(CliError::failed)?;
        }
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

/// Plants per second of a batch of `count` plants on the current pool, each with the
/// annotations of `set`.
fn throughput(
    species: &Species,
    sampler: &Sampler,
    count: u32,
    set: &AnnotationSet,
) -> Result<f64, CliError> {
    let start = Instant::now();
    let results = run_batch(count as usize, |generator, plant, index| {
        let params = sampler.params(index as u32);
        generator
            .generate_into(species, &params, plant)
            .map_err(CliError::failed)?;
        BUFFER.with_borrow_mut(|bytes| export(species, plant, bytes))?;
        if set.any() {
            BUFFER.with_borrow_mut(|bytes| {
                annotate::in_memory(set, generator, plant, species, bytes).map_err(CliError::failed)
            })?;
        }
        Ok(())
    });
    let elapsed = start.elapsed().as_secs_f64();
    results.into_iter().collect::<Result<Vec<()>, _>>()?;
    Ok(f64::from(count) / elapsed)
}

/// Median time of `repeats` growths of the plant of `params` with `options`.
fn time_growth(
    species: &Species,
    params: &GrowParams,
    options: EngineOptions,
    repeats: usize,
) -> f64 {
    let open = Surroundings::none();
    let times = (0..repeats).map(|_| {
        let start = Instant::now();
        std::hint::black_box(grow_in_with(species, params, &open, options));
        start.elapsed()
    });
    ms(median(times.collect()))
}

/// Plants per second of `count` plants of the species grown on the global pool, one
/// plant per task, each sensing its buds as `options` says.
fn growth_throughput(species: &Species, age: u16, options: EngineOptions, count: u32) -> f64 {
    let open = Surroundings::none();
    let start = Instant::now();
    (0..count).into_par_iter().for_each(|index| {
        let params = GrowParams {
            seed: arbrume_gen::plant_seed(1, index),
            age,
            scale: 1.0,
        };
        std::hint::black_box(grow_in_with(species, &params, &open, options));
    });
    f64::from(count) / start.elapsed().as_secs_f64()
}

/// Prints the table of `--engine`.
fn engine_table(species: &[(String, Species)], args: &BenchArgs) {
    let threads = rayon::current_num_threads();
    println!(
        "| species | age | nodes | default ms | sparse grid ms | parallel buds ms \
         | sequential markers ms | trees/s ({threads} threads) | trees/s, sequential plants |"
    );
    println!("|---|---|---|---|---|---|---|---|---|");
    let default = EngineOptions::default();
    let sparse = EngineOptions {
        storage: GridStorage::Sparse,
        ..default
    };
    let buds = EngineOptions {
        parallel_buds: true,
        ..default
    };
    let sequential = EngineOptions {
        parallel_markers: false,
        ..default
    };
    let competing = species
        .iter()
        .filter(|(_, species)| species.growth.mode == GrowthMode::SelfOrganizing);
    for (label, species) in competing {
        for &age in &args.ages {
            let params = GrowParams {
                seed: arbrume_gen::plant_seed(1, 0),
                age,
                scale: 1.0,
            };
            let nodes = grow_in_with(species, &params, &Surroundings::none(), default).node_count();
            let time = |options| time_growth(species, &params, options, args.repeats);
            let (batch, plain) = if args.batch == 0 {
                (String::from("-"), String::from("-"))
            } else {
                let rate = |options| growth_throughput(species, age, options, args.batch);
                (
                    format!("{:.1}", rate(default)),
                    format!("{:.1}", rate(sequential)),
                )
            };
            println!(
                "| {label} | {age} | {nodes} | {:.2} | {:.2} | {:.2} | {:.2} | {batch} | {plain} |",
                time(default),
                time(sparse),
                time(buds),
                time(sequential)
            );
        }
    }
}

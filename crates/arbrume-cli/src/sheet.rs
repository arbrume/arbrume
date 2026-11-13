//! `arbrume sheet`: a contact sheet of thumbnails, for tuning species by eye.
//!
//! The sheet is one PNG holding a grid of thumbnails. Without `--random` there is one
//! row per species, seed and view azimuth and one column per age. With `--random N` the
//! sheet shows plants `offset..offset + N` of the random sampler, row by row.
//! `--view crown` and `--view junction` frame a part of the plant instead of the whole:
//! the middle of the crown, or the trunk and its first branches. One line per plant
//! (height, triangles, leaf sites, time) goes to standard output.

use std::path::PathBuf;
use std::time::Instant;

use arbrume_core::species::Species;
use arbrume_gen::{Budget, GenParams, Generator, Plant, RandomSampler, plant_seed};
use arbrume_raster::{Bounds, Camera, Image, Renderer, ThumbnailOptions};
use clap::{Args, ValueEnum};
use glam::Vec3;
use rayon::prelude::*;

use crate::error::CliError;
use crate::source::resolve;

/// Which part of a plant a thumbnail frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum View {
    /// The whole plant.
    Full,
    /// The middle of the crown, enlarged.
    Crown,
    /// The trunk and its first branches, enlarged.
    Junction,
}

/// Arguments of `arbrume sheet`.
#[derive(Debug, Args)]
pub struct SheetArgs {
    /// Species to show, presets or files.
    #[arg(long, value_delimiter = ',', required_unless_present = "random")]
    species: Vec<String>,
    /// Ages, one column each.
    #[arg(long, value_delimiter = ',', default_value = "8,20,40")]
    ages: Vec<u16>,
    /// Seeds per species, one row each; with --random, the base seed.
    #[arg(long, default_value_t = 2)]
    seeds: u32,
    /// View azimuths in degrees, one row each.
    #[arg(long, value_delimiter = ',', default_value = "20")]
    azimuths: Vec<f32>,
    /// Side of a thumbnail in pixels.
    #[arg(long, default_value_t = 256)]
    size: u32,
    /// Mesh quality [default: 1, or the default of the random sampler].
    #[arg(long)]
    quality: Option<f32>,
    /// Part of the plant to frame.
    #[arg(long, value_enum, default_value_t = View::Full)]
    view: View,
    /// Show this many plants of the random sampler instead of species and ages.
    #[arg(long)]
    random: Option<u32>,
    /// Index of the first random plant.
    #[arg(long, default_value_t = 0)]
    offset: u32,
    /// Thumbnails per row of a random sheet.
    #[arg(long, default_value_t = 8)]
    columns: u32,
    /// PNG file to write.
    #[arg(long)]
    out: PathBuf,
}

/// One thumbnail to make.
#[derive(Debug, Clone)]
struct Cell {
    label: String,
    species: Species,
    params: GenParams,
    budget: Budget,
    azimuth: f32,
}

/// The cells of a species sheet, row by row, and the number of columns.
fn species_cells(args: &SheetArgs) -> Result<(Vec<Cell>, u32), CliError> {
    let mut cells = Vec::new();
    for spec in &args.species {
        let source = resolve(spec, std::path::Path::new(""))?;
        let species = Species::from_toml_str(&source.text).map_err(CliError::usage)?;
        for seed in 0..args.seeds {
            for &azimuth in &args.azimuths {
                for &age in &args.ages {
                    let seed = plant_seed(u64::from(seed) + 1, 0);
                    let quality = args.quality.unwrap_or(1.0);
                    cells.push(Cell {
                        label: format!("{} age {age}", source.label),
                        species: species.clone(),
                        params: GenParams::new(seed, age).with_quality(quality),
                        budget: Budget::NONE,
                        azimuth,
                    });
                }
            }
        }
    }
    Ok((cells, args.ages.len() as u32))
}

/// The cells of a random sheet and the number of columns.
fn random_cells(args: &SheetArgs, count: u32) -> Result<(Vec<Cell>, u32), CliError> {
    let mut sampler = RandomSampler::new(u64::from(args.seeds));
    if let Some(quality) = args.quality {
        sampler.quality = quality;
    }
    let azimuth = args.azimuths.first().copied().unwrap_or(20.0);
    let cells = (args.offset..args.offset + count)
        .map(|index| {
            let pick = sampler.pick(index).map_err(CliError::failed)?;
            Ok(Cell {
                label: format!("{index} {} age {}", pick.preset, pick.params.age),
                species: pick.species,
                params: pick.params,
                budget: pick.budget,
                azimuth,
            })
        })
        .collect::<Result<Vec<_>, CliError>>()?;
    Ok((cells, args.columns.max(1)))
}

/// The camera of `view` on a plant with the bounds `bounds`.
fn camera(view: View, bounds: &Bounds, options: &ThumbnailOptions) -> Option<Camera> {
    let size = bounds.size();
    let (center, half) = match view {
        View::Full => return None,
        View::Crown => (
            bounds.min + size * Vec3::new(0.5, 0.62, 0.5),
            size * Vec3::new(0.22, 0.16, 0.22),
        ),
        View::Junction => (
            Vec3::new(0.0, bounds.min.y + 0.3 * size.y, 0.0),
            Vec3::splat(0.1 * size.y),
        ),
    };
    let part = Bounds {
        min: center - half,
        max: center + half,
    };
    Some(Camera::frame(
        &part,
        options.azimuth,
        options.elevation,
        options.fov,
    ))
}

/// Generates and renders one cell; returns its image and its report line.
fn render(cell: &Cell, args: &SheetArgs) -> Result<(Image, String), CliError> {
    let start = Instant::now();
    let mut plant = Plant::default();
    Generator::new()
        .generate_within(&cell.species, &cell.params, &cell.budget, &mut plant)
        .map_err(|e| CliError::failed(format!("{}: {e}", cell.label)))?;
    let elapsed = start.elapsed();
    let options = ThumbnailOptions {
        width: args.size,
        height: args.size,
        azimuth: cell.azimuth,
        ..ThumbnailOptions::default()
    };
    let mut scene = options.scene(&plant.mesh);
    let bounds = Bounds::of_mesh(&plant.mesh);
    if let Some(camera) = bounds.and_then(|bounds| camera(args.view, &bounds, &options)) {
        scene.camera = camera;
    }
    let image = Renderer::new().render(&plant.mesh, &scene).to_image();
    let line = report(cell, &plant, elapsed);
    Ok((image, line))
}

/// One line on a generated plant: quality reached, size, trunk diameter, triangle and
/// site counts, time.
fn report(cell: &Cell, plant: &Plant, elapsed: std::time::Duration) -> String {
    let stats = plant.stats;
    let bark = plant.mesh.parts.first();
    let bark = bark.map_or(0, |part| part.buffers.triangle_count());
    let full = GenParams::default().mesh.tolerance;
    let quality = (full / plant.params.mesh.tolerance).sqrt();
    let graph = &plant.graph;
    let roots = graph
        .chain_ids()
        .filter_map(|chain| graph.chain_view(chain));
    let trunk = roots
        .filter(|view| view.attach.is_none())
        .filter_map(|view| view.radii.first().copied())
        .fold(0.0, f32::max);
    let size = stats.bounds.map_or([0.0; 3], |bounds| bounds.size());
    format!(
        "{}: quality {quality:.2}, height {:.2} m, width {:.2} m, trunk {:.3} m,          {} triangles ({bark} bark), {} vertices, {} nodes, {} leaf sites, {:.2} ms",
        cell.label,
        stats.height,
        size[0].max(size[2]),
        2.0 * trunk,
        stats.triangles,
        stats.vertices,
        stats.nodes,
        stats.leaf_sites,
        elapsed.as_secs_f64() * 1e3
    )
}

/// Copies `tile` into `sheet` with its top left corner at `(left, top)`.
fn paste(sheet: &mut Image, tile: &Image, left: u32, top: u32) {
    let row_bytes = 4 * tile.width as usize;
    for y in 0..tile.height {
        let from = y as usize * row_bytes;
        let to = 4 * ((top + y) as usize * sheet.width as usize + left as usize);
        sheet.pixels[to..to + row_bytes].copy_from_slice(&tile.pixels[from..from + row_bytes]);
    }
}

/// Runs `arbrume sheet`.
pub fn run(args: &SheetArgs) -> Result<(), CliError> {
    if args.size == 0 {
        return Err(CliError::usage("--size must be at least 1"));
    }
    let (cells, columns) = match args.random {
        Some(count) => random_cells(args, count)?,
        None => species_cells(args)?,
    };
    if cells.is_empty() {
        return Err(CliError::usage("nothing to show"));
    }
    let tiles = cells
        .par_iter()
        .map(|cell| render(cell, args))
        .collect::<Result<Vec<_>, _>>()?;
    let rows = (cells.len() as u32).div_ceil(columns);
    let (width, height) = (columns * args.size, rows * args.size);
    let mut sheet = Image {
        width,
        height,
        pixels: vec![255; 4 * width as usize * height as usize],
    };
    for (index, (tile, line)) in tiles.iter().enumerate() {
        let (column, row) = (index as u32 % columns, index as u32 / columns);
        paste(&mut sheet, tile, column * args.size, row * args.size);
        println!("{line}");
    }
    sheet
        .write_png(&args.out)
        .map_err(|e| CliError::failed(format!("{}: {e}", args.out.display())))
}

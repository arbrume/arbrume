//! Validation runs of the tree distance and the fit, as recorded in `docs/fitting.md`.
//!
//! ```sh
//! cargo run --release -p arbrume-fit --example fit_validate -- separation
//! cargo run --release -p arbrume-fit --example fit_validate -- timing
//! cargo run --release -p arbrume-fit --example fit_validate -- recovery OUT_DIR
//! cargo run --release -p arbrume-fit --example fit_validate -- cross OUT_DIR
//! cargo run --release -p arbrume-fit --example fit_validate -- noisy OUT_DIR
//! ```
//!
//! * `separation`: for some presets and seeds, the mean of every term between plants of
//!   the same preset and between plants of different presets.
//! * `timing`: microseconds to normalise a skeleton and to compare two.
//! * `recovery`: targets grown from presets with four perturbed parameters and a random
//!   age and scale, fitted from the same preset and from another one; the error of the
//!   parameter values and the distances before and after.
//! * `cross`: presets fitted to plants of other presets.
//! * `noisy`: targets with a fifth of their small branches dropped and jittered
//!   positions, fitted as they are and with pruning and smoothing, and the fits measured
//!   against the clean targets.
//!
//! The fit runs write, per fit, the fitted species, the report and a contact sheet to
//! `OUT_DIR`: rows are the target (its bark as the distance sees it, then two more plants
//! of the target species), the base species and the fitted species, bark only, then the
//! target species and the fitted species with foliage; three plant seeds per row; every
//! picture of a sheet is framed by the same camera, so sizes compare.

use std::path::{Path, PathBuf};
use std::time::Instant;

use arbrume_core::graph::PlantGraph;
use arbrume_core::mesh::{PartKind, TreeMesh};
use arbrume_core::rng::{Rng, Stream};
use arbrume_core::species::Species;
use arbrume_fit::{
    Descriptor, DistanceOptions, FitOptions, FitResult, Terms, TreeSkeleton, distance, fit,
    grow_skeleton, normalized_graph, pruned_height, step_for,
};
use arbrume_gen::{GenParams, Generator, plant_seed, preset, preset_toml};
use arbrume_mesh::{MeshOptions, Mesher};
use arbrume_raster::{Camera, Image, Renderer, ThumbnailOptions};
use glam::Vec3;

/// Presets of the separation table.
const SEPARATION: [&str; 8] = [
    "broadleaf",
    "maple_like",
    "oak_like",
    "birch_like",
    "poplar_like",
    "willow_like",
    "pine_like",
    "spruce_like",
];

/// Parameters perturbed in the recovery targets.
const PERTURBED: [&str; 4] = [
    "growth.axis_length",
    "growth.branching_angle",
    "growth.gravitropic_setpoint",
    "growth.branch_probability",
];

fn middle_age(species: &Species) -> u16 {
    let [young, old] = species.variation.age;
    u16::midpoint(young, old)
}

fn grow(name: &str, seed: u64) -> TreeSkeleton {
    let species = preset(name).unwrap();
    grow_skeleton(&species, &GenParams::new(seed, middle_age(&species)))
}

fn separation(options: &DistanceOptions) {
    let seeds = 4;
    let plants: Vec<(usize, TreeSkeleton)> = SEPARATION
        .iter()
        .enumerate()
        .flat_map(|(p, name)| (0..seeds).map(move |s| (p, grow(name, plant_seed(7, s)))))
        .collect();
    let mut within = Vec::new();
    let mut between = Vec::new();
    for (i, (pa, a)) in plants.iter().enumerate() {
        for (pb, b) in &plants[i + 1..] {
            let terms = distance(a, b, options).unwrap();
            if pa == pb {
                within.push(terms);
            } else {
                between.push(terms);
            }
        }
    }
    let (w, b) = (Terms::mean(&within), Terms::mean(&between));
    println!("| term | within a preset | between presets | ratio |");
    println!("|---|---|---|---|");
    for ((name, x), y) in Terms::NAMES.iter().zip(w.values()).zip(b.values()) {
        println!("| {name} | {x:.4} | {y:.4} | {:.2} |", y / x.max(1e-12));
    }
    println!("pairs: {} within, {} between", within.len(), between.len());
}

fn timing() {
    let options = DistanceOptions::default();
    println!("| preset | age | nodes | resampled nodes | normalise us | compare us |");
    println!("|---|---|---|---|---|---|");
    for name in [
        "sapling",
        "broadleaf",
        "maple_like",
        "oak_like",
        "pine_like",
    ] {
        let species = preset(name).unwrap();
        let age = species.variation.age[1];
        let a = grow_skeleton(&species, &GenParams::new(1, age));
        let b = grow_skeleton(&species, &GenParams::new(2, age));
        let step = step_for(pruned_height(&a, 0.0), &options);
        let repeats = 50;
        let clock = Instant::now();
        let mut descriptor = None;
        for _ in 0..repeats {
            descriptor = Some(Descriptor::new(&a, step, &options).unwrap());
        }
        let normalise = clock.elapsed().as_secs_f64() * 1e6 / f64::from(repeats);
        let da = descriptor.unwrap();
        let db = Descriptor::new(&b, step, &options).unwrap();
        let clock = Instant::now();
        let mut total = 0.0;
        for _ in 0..repeats * 20 {
            total += da.compare(&db, &options).total;
        }
        let compare = clock.elapsed().as_secs_f64() * 1e6 / f64::from(repeats * 20);
        assert!(total > 0.0);
        println!(
            "| {name} | {age} | {} | {} | {normalise:.0} | {compare:.1} |",
            a.len(),
            da.nodes
        );
    }
}

/// The bark of `graph` meshed with `species`.
fn bark(graph: &PlantGraph, species: &Species) -> TreeMesh {
    let mut mesh = TreeMesh::default();
    let options = MeshOptions::default();
    Mesher::new()
        .mesh_into(graph, species, &options, &mut mesh)
        .unwrap();
    mesh.parts.retain(|part| part.kind == PartKind::Bark);
    mesh
}

/// The full mesh of the plant of `species` with `params`.
fn full(species: &Species, params: &GenParams) -> TreeMesh {
    Generator::new().generate(species, params).unwrap().mesh
}

/// The bark of the plant of `species` with `params`.
fn bare(species: &Species, params: &GenParams) -> TreeMesh {
    let plant = Generator::new().generate(species, params).unwrap();
    bark(&plant.graph, species)
}

/// Renders `rows` of meshes with one camera that frames them all, and writes the sheet.
fn sheet(rows: &[Vec<TreeMesh>], path: &Path) {
    let size = 256_u32;
    let options = ThumbnailOptions {
        width: size,
        height: size,
        azimuth: 20.0,
        ..ThumbnailOptions::default()
    };
    let points: Vec<Vec3> = rows
        .iter()
        .flatten()
        .flat_map(|mesh| mesh.parts.iter())
        .flat_map(|part| part.buffers.positions.iter())
        .map(|p| Vec3::from_array(*p))
        .collect();
    let camera = Camera::frame_points(
        &points,
        20.0,
        options.elevation,
        options.fov,
        options.margin,
    );
    let columns = rows.iter().map(Vec::len).max().unwrap_or(1) as u32;
    let mut out = Image {
        width: size * columns,
        height: size * rows.len() as u32,
        pixels: vec![255; (size * columns * size * rows.len() as u32 * 4) as usize],
    };
    let mut renderer = Renderer::new();
    for (r, row) in rows.iter().enumerate() {
        for (c, mesh) in row.iter().enumerate() {
            let mut scene = options.scene(mesh);
            scene.camera = camera;
            let image = renderer.render(mesh, &scene).to_image();
            paste(&mut out, &image, c as u32 * size, r as u32 * size);
        }
    }
    out.write_png(path).unwrap();
}

/// Copies `image` into `out` at `(x, y)`.
fn paste(out: &mut Image, image: &Image, x: u32, y: u32) {
    let len = (image.width * 4) as usize;
    for row in 0..image.height {
        let from = (row * image.width * 4) as usize;
        let to = (((y + row) * out.width + x) * 4) as usize;
        out.pixels[to..to + len].copy_from_slice(&image.pixels[from..from + len]);
    }
}

/// A fit target: a species, the parameters of the target plant and its skeleton.
#[derive(Debug, Clone)]
struct Target {
    label: String,
    species: Species,
    params: GenParams,
    skeleton: TreeSkeleton,
}

/// Preset `name` with the [`PERTURBED`] parameters multiplied by factors in
/// `[e^-0.35, e^0.35]`, at a random age of its range and a scale in `[e^-0.2, e^0.2]`.
fn perturbed(name: &str, index: u32) -> Target {
    let base = preset(name).unwrap();
    let mut rng = Rng::new(42, index, Stream::custom(0x300));
    let mut species = base.clone();
    for parameter in PERTURBED {
        let factor = (0.35 * rng.signed()).exp();
        species = species.with_factor(parameter, factor).unwrap();
    }
    let [young, old] = base.variation.age;
    let age = young + (rng.f32() * f32::from(old - young)) as u16;
    let scale = (0.2 * rng.signed()).exp();
    let params = GenParams::new(plant_seed(1000, index), age).with_scale(scale);
    Target {
        label: format!("{name}_{index}"),
        skeleton: grow_skeleton(&species, &params),
        species,
        params,
    }
}

/// An unperturbed plant of preset `name` at the middle of its age range.
fn plain(name: &str, index: u32) -> Target {
    let species = preset(name).unwrap();
    let params = GenParams::new(plant_seed(2000, index), middle_age(&species));
    Target {
        label: format!("{name}_plain{index}"),
        skeleton: grow_skeleton(&species, &params),
        species,
        params,
    }
}

/// Fits `from` to `target`, writes the species, the report and the sheet.
fn run_fit(
    target: &Target,
    from: &str,
    options: &FitOptions,
    out: &Path,
    tag: &str,
) -> (FitResult, f64) {
    let clock = Instant::now();
    let result = fit(&target.skeleton, preset_toml(from).unwrap(), from, options).unwrap();
    let seconds = clock.elapsed().as_secs_f64();
    let stem = format!("{}_from_{from}{tag}", target.label);
    std::fs::write(out.join(format!("{stem}.toml")), &result.toml).unwrap();
    let report = serde_json::to_string_pretty(&result.report).unwrap();
    std::fs::write(out.join(format!("{stem}.report.json")), report).unwrap();
    let base = preset(from).unwrap();
    let seeds = |age: u16, scale: f32| -> Vec<GenParams> {
        (0..3)
            .map(|k| GenParams::new(plant_seed(5, k), age).with_scale(scale))
            .collect()
    };
    let step = step_for(
        target
            .skeleton
            .position
            .iter()
            .map(|p| p[1])
            .fold(0.0, f32::max),
        &options.distance,
    );
    let target_graph = normalized_graph(&target.skeleton, step, &options.distance).unwrap();
    let mut target_row = vec![bark(&target_graph, &target.species)];
    let others = seeds(target.params.age, target.params.scale);
    target_row.extend(others[1..].iter().map(|p| bare(&target.species, p)));
    let fitted = seeds(result.fitted.age, result.fitted.scale);
    let rows = [
        target_row,
        seeds(middle_age(&base), 1.0)
            .iter()
            .map(|p| bare(&base, p))
            .collect(),
        fitted.iter().map(|p| bare(&result.species, p)).collect(),
        others.iter().map(|p| full(&target.species, p)).collect(),
        fitted.iter().map(|p| full(&result.species, p)).collect(),
    ];
    sheet(&rows, &out.join(format!("{stem}.png")));
    (result, seconds)
}

fn holdout(report: &serde_json::Value, key: &str) -> f64 {
    report[key]["holdout"]["total"].as_f64().unwrap()
}

/// Mean of a curve parameter over orders 0 to 2 and positions 0.1 to 0.9.
fn value(species: &Species, name: &str) -> f64 {
    let growth = &species.growth;
    let curve = match name {
        "growth.axis_length" => &growth.axis_length,
        "growth.branching_angle" => &growth.branching_angle,
        "growth.gravitropic_setpoint" => &growth.gravitropic_setpoint,
        _ => &growth.branch_probability,
    };
    let values: Vec<f64> = (0..3)
        .flat_map(|order| (1..10).map(move |k| f64::from(curve.eval(order, k as f32 / 10.0))))
        .collect();
    values.iter().sum::<f64>() / values.len() as f64
}

/// Mean absolute log ratio of the [`PERTURBED`] values of `species` to `truth`, and of
/// the effective length (axis length times scale).
fn parameter_error(species: &Species, scale: f32, truth: &Target) -> (f64, f64) {
    let log_ratio = |a: f64, b: f64| {
        if a > 0.0 && b > 0.0 {
            (a / b).ln().abs()
        } else {
            f64::NAN
        }
    };
    let errors: Vec<f64> = PERTURBED
        .iter()
        .map(|name| log_ratio(value(species, name), value(&truth.species, name)))
        .filter(|e| e.is_finite())
        .collect();
    let length = |s: &Species, scale: f32| value(s, "growth.axis_length") * f64::from(scale);
    let effective = log_ratio(
        length(species, scale),
        length(&truth.species, truth.params.scale),
    );
    (
        errors.iter().sum::<f64>() / errors.len().max(1) as f64,
        effective,
    )
}

fn recovery(out: &Path) {
    let cases = [
        ("broadleaf", "maple_like"),
        ("maple_like", "broadleaf"),
        ("oak_like", "maple_like"),
        ("birch_like", "broadleaf"),
        ("poplar_like", "birch_like"),
        ("willow_like", "maple_like"),
        ("pine_like", "spruce_like"),
        ("spruce_like", "conifer"),
        ("conifer", "spruce_like"),
        ("acacia_like", "oak_like"),
        ("shrub", "upright_shrub"),
        ("sapling", "broadleaf"),
    ];
    println!(
        "| target | from | fitted | before | after | param error before | after | length error before | after | age true/fit | scale true/fit | s |"
    );
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|");
    let mut few: Vec<String> = PERTURBED.iter().map(|p| (*p).to_owned()).collect();
    few.extend(["age".to_owned(), "scale".to_owned()]);
    for (index, (name, other)) in cases.iter().enumerate() {
        let target = perturbed(name, index as u32);
        let runs = [
            (*name, Some(few.clone()), "4+2"),
            (*name, None, "default"),
            (*other, None, "default"),
        ];
        for (from, parameters, set) in runs {
            let options = FitOptions {
                parameters,
                ..FitOptions::default()
            };
            let tag = if set == "4+2" { "_few" } else { "" };
            let (result, seconds) = run_fit(&target, from, &options, out, tag);
            let base = preset(from).unwrap();
            let before = parameter_error(&base, 1.0, &target);
            let after = parameter_error(&result.species, result.fitted.scale, &target);
            println!(
                "| {} | {from} | {set} | {:.4} | {:.4} | {:.3} | {:.3} | {:.3} | {:.3} | {}/{} | {:.2}/{:.2} | {seconds:.1} |",
                target.label,
                holdout(&result.report, "before"),
                holdout(&result.report, "after"),
                before.0,
                after.0,
                before.1,
                after.1,
                target.params.age,
                result.fitted.age,
                target.params.scale,
                result.fitted.scale,
            );
        }
    }
}

fn cross(out: &Path) {
    let cases = [
        ("maple_like", "oak_like"),
        ("oak_like", "maple_like"),
        ("birch_like", "poplar_like"),
        ("willow_like", "broadleaf"),
        ("pine_like", "conifer"),
        ("acacia_like", "maple_like"),
    ];
    println!("| target | from | before | after | after, train | age | scale | candidates | s |");
    println!("|---|---|---|---|---|---|---|---|---|");
    for (index, (name, from)) in cases.iter().enumerate() {
        let target = plain(name, index as u32);
        let (result, seconds) = run_fit(&target, from, &FitOptions::default(), out, "");
        println!(
            "| {} | {from} | {:.4} | {:.4} | {:.4} | {} | {:.2} | {} | {seconds:.1} |",
            target.label,
            holdout(&result.report, "before"),
            holdout(&result.report, "after"),
            result.best.terms.total,
            result.fitted.age,
            result.fitted.scale,
            result.report["evaluations"],
        );
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out = args.get(2).map(PathBuf::from);
    if let Some(out) = &out {
        std::fs::create_dir_all(out).unwrap();
    }
    match args.get(1).map(String::as_str) {
        Some("separation") => {
            for (rows, columns) in [(24, 12), (12, 6), (8, 4)] {
                println!("\nheight_bins {rows}, radial_bins {columns}");
                separation(&DistanceOptions {
                    height_bins: rows,
                    radial_bins: columns,
                    ..DistanceOptions::default()
                });
            }
        }
        Some("timing") => timing(),
        Some("recovery") => recovery(out.as_deref().expect("OUT_DIR")),
        Some("cross") => cross(out.as_deref().expect("OUT_DIR")),
        Some("noisy") => noisy::run(out.as_deref().expect("OUT_DIR")),
        _ => eprintln!("usage: fit_validate separation|timing|recovery|cross|noisy [OUT_DIR]"),
    }
}

/// The noisy-target runs.
mod noisy {
    use super::{FitOptions, Path, Rng, Stream, Target, TreeSkeleton, holdout, perturbed, run_fit};
    use arbrume_fit::{DistanceOptions, distance, grow_skeleton};
    use arbrume_gen::plant_seed;

    /// Amplitude of the uniform position noise, meters.
    const JITTER: f32 = 0.03;

    /// Lateral subtrees of `skeleton`, by their first node, with their total length.
    fn laterals(skeleton: &TreeSkeleton) -> Vec<(usize, f32)> {
        let children = skeleton.children();
        let mut length = vec![0.0_f32; skeleton.len()];
        for node in skeleton.topological_order().into_iter().rev() {
            let own = usize::try_from(skeleton.parent[node]).map_or(0.0, |p| {
                let (a, b) = (skeleton.position[node], skeleton.position[p]);
                ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
            });
            length[node] = own + children[node].iter().map(|&c| length[c]).sum::<f32>();
        }
        let mut out = Vec::new();
        for kids in children.iter().filter(|k| k.len() > 1) {
            let main = kids
                .iter()
                .copied()
                .max_by(|&a, &b| length[a].total_cmp(&length[b]));
            out.extend(
                kids.iter()
                    .filter(|&&k| Some(k) != main)
                    .map(|&k| (k, length[k])),
            );
        }
        out
    }

    /// `skeleton` without a fifth of its lateral subtrees shorter than the median, and
    /// with every position moved by up to [`JITTER`] along each axis.
    pub fn damaged(skeleton: &TreeSkeleton, seed: u32) -> TreeSkeleton {
        let mut small = laterals(skeleton);
        let mut lengths: Vec<f32> = small.iter().map(|s| s.1).collect();
        lengths.sort_by(f32::total_cmp);
        let median = lengths.get(lengths.len() / 2).copied().unwrap_or(0.0);
        small.retain(|s| s.1 < median);
        let mut dropped = vec![false; skeleton.len()];
        for (k, (node, _)) in small.iter().enumerate() {
            let mut rng = Rng::new(u64::from(seed), k as u32, Stream::custom(0x301));
            dropped[*node] = rng.chance(0.2);
        }
        let mut index = vec![-1_i32; skeleton.len()];
        let mut out = TreeSkeleton::default();
        for node in skeleton.topological_order() {
            let parent = skeleton.parent[node];
            let parent_dropped = parent >= 0 && index[parent as usize] < 0;
            if dropped[node] || parent_dropped {
                continue;
            }
            let mut rng = Rng::new(u64::from(seed), node as u32, Stream::custom(0x302));
            let p = skeleton.position[node].map(|c| c + JITTER * rng.signed());
            index[node] = i32::try_from(out.position.len()).unwrap();
            out.position.push(p);
            out.radius.push(skeleton.radius[node]);
            out.parent.push(if parent < 0 {
                -1
            } else {
                index[parent as usize]
            });
        }
        out
    }

    /// Mean distance with default options from plants of the fitted species to `clean`.
    fn to_clean(result: &arbrume_fit::FitResult, clean: &TreeSkeleton) -> f64 {
        let options = DistanceOptions::default();
        let totals: Vec<f64> = (0..8)
            .map(|k| {
                let params = result.fitted.params(plant_seed(77, k));
                let plant = grow_skeleton(&result.species, &params);
                distance(clean, &plant, &options).unwrap().total
            })
            .collect();
        totals.iter().sum::<f64>() / totals.len() as f64
    }

    pub fn run(out: &Path) {
        println!(
            "| target | target nodes | damaged nodes | fit on | before | after | after vs clean | age | scale | s |"
        );
        println!("|---|---|---|---|---|---|---|---|---|---|");
        for (index, name) in [
            "broadleaf",
            "maple_like",
            "oak_like",
            "birch_like",
            "pine_like",
        ]
        .iter()
        .enumerate()
        {
            let clean = perturbed(name, 100 + index as u32);
            let skeleton = damaged(&clean.skeleton, index as u32);
            let mut radii = clean.skeleton.radius.clone();
            radii.sort_by(f32::total_cmp);
            let quartile = radii[radii.len() / 4];
            let variants = [
                ("clean", clean.skeleton.clone(), DistanceOptions::default()),
                ("damaged", skeleton.clone(), DistanceOptions::default()),
                (
                    "damaged, pruned and smoothed",
                    skeleton.clone(),
                    DistanceOptions {
                        min_radius: quartile,
                        smooth: 2,
                        ..DistanceOptions::default()
                    },
                ),
            ];
            for (tag, target_skeleton, distance_options) in variants {
                let target = Target {
                    skeleton: target_skeleton,
                    ..clean.clone()
                };
                let options = FitOptions {
                    distance: distance_options,
                    ..FitOptions::default()
                };
                let suffix = format!("_{}", tag.replace([',', ' '], "_"));
                let (result, seconds) = run_fit(&target, name, &options, out, &suffix);
                println!(
                    "| {} | {} | {} | {tag} | {:.4} | {:.4} | {:.4} | {}/{} | {:.2}/{:.2} | {seconds:.1} |",
                    clean.label,
                    clean.skeleton.len(),
                    target.skeleton.len(),
                    holdout(&result.report, "before"),
                    holdout(&result.report, "after"),
                    to_clean(&result, &clean.skeleton),
                    clean.params.age,
                    result.fitted.age,
                    clean.params.scale,
                    result.fitted.scale,
                );
            }
        }
    }
}

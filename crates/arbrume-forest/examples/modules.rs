//! Measures what branch-module instancing would save on variant libraries.
//!
//! ```text
//! cargo run --release -p arbrume-forest --example modules -- broadleaf:20 excurrent_conifer:25
//! ```
//!
//! Each argument is `preset:age`; four variants of each are grown without textures, and
//! [`arbrume_forest::modules::module_report`] runs on each species alone and on all of
//! them, for the axes of order 1 and 2 and tolerances of 5, 10 and 20 % of a module's
//! length. Prints a Markdown table.

use arbrume_forest::modules::{ModuleReport, module_report};
use arbrume_forest::{LibrarySettings, VariantSpec, build_library};
use arbrume_gen::{Generator, LodOptions, LodSettings, preset};

const TOLERANCES: [f32; 3] = [0.05, 0.1, 0.2];

fn row(label: &str, report: &ModuleReport) {
    println!(
        "| {label} | {} | {:.2} | {} | {} | {:.0} % | {:.0} % | {:.1} kB | {:.3} | {:.3} |",
        report.order,
        report.tolerance,
        report.modules,
        report.prototypes,
        100.0 * report.vertices_in_modules as f64 / report.vertices_total as f64,
        100.0 * report.vertices_shared as f64 / report.vertices_total as f64,
        report.transform_bytes as f64 / 1e3,
        report.mean_error,
        report.max_error,
    );
}

fn main() {
    let specs: Vec<VariantSpec> = std::env::args()
        .skip(1)
        .enumerate()
        .map(|(index, arg)| {
            let (name, age) = arg.split_once(':').expect("preset:age");
            VariantSpec {
                name: name.to_owned(),
                species: preset(name).expect("a preset"),
                age: age.parse().expect("an age"),
                count: 4,
                seed: 100 + index as u64,
            }
        })
        .collect();
    let settings = LibrarySettings {
        lod: LodSettings {
            lod: LodOptions {
                levels: 1,
                ..LodOptions::default()
            },
            ..LodSettings::default()
        },
        textures: false,
        quality: 1.0,
    };
    let library = build_library(&specs, &settings).expect("library");
    let mut generator = Generator::new();
    let mut all = Vec::new();
    for species in &library {
        for variant in &species.variants {
            let provenance = generator
                .provenance(&variant.plant, &variant.individual)
                .expect("provenance");
            all.push((
                species.name.clone(),
                variant.plant.graph.clone(),
                provenance,
            ));
        }
    }
    println!(
        "| plants | order | tolerance | modules | prototypes | vertices in modules | vertices kept | transforms | mean error (m) | max error (m) |"
    );
    println!("|---|---|---|---|---|---|---|---|---|---|");
    let mut groups: Vec<(String, Vec<usize>)> = library
        .iter()
        .map(|s| {
            let indices = (0..all.len()).filter(|&i| all[i].0 == s.name).collect();
            (format!("{} x4", s.name), indices)
        })
        .collect();
    if library.len() > 1 {
        groups.push(("all".to_owned(), (0..all.len()).collect()));
    }
    for (label, indices) in groups {
        let plants: Vec<_> = indices.iter().map(|&i| (&all[i].1, &all[i].2)).collect();
        for order in [1, 2] {
            for report in module_report(&plants, order, &TOLERANCES) {
                row(&label, &report);
            }
        }
    }
}

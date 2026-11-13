//! Growth time of the starter species at a few ages.
//!
//! Each benchmark is named `grow/<species>/<age>`; the node, chain and leaf counts of
//! the plant grown with seed 1 are printed once before it is measured.

use arbrume_core::species::Species;
use arbrume_grow::{GrowParams, grow};
use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use std::path::PathBuf;

const AGES: [u16; 3] = [8, 20, 40];

fn species(name: &str) -> Species {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(format!("{name}.toml"));
    Species::from_path(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn bench_species(criterion: &mut Criterion) {
    for name in ["broadleaf", "conifer", "shrub"] {
        let species = species(name);
        for age in AGES {
            let params = GrowParams {
                seed: 1,
                age,
                scale: 1.0,
            };
            let graph = grow(&species, &params);
            println!(
                "{name} age {age}: {} nodes, {} chains, {} leaf sites",
                graph.node_count(),
                graph.chain_count(),
                graph.organs().len()
            );
            criterion.bench_function(&format!("grow/{name}/{age}"), |bencher| {
                bencher.iter(|| grow(black_box(&species), black_box(&params)));
            });
        }
    }
}

criterion_group!(benches, bench_species);
criterion_main!(benches);

//! Batch determinism, generator reuse and export of every preset.

use arbrume_core::species::Species;
use arbrume_export::{GlbOptions, write_glb};
use arbrume_gen::{
    Distribution, GENERATOR, GenParams, Generator, Plant, Sampler, generate, generate_many,
    metadata, preset, presets, run_batch,
};

fn all_presets() -> Vec<(&'static str, Species)> {
    presets()
        .iter()
        .map(|(name, _)| (*name, preset(name).unwrap()))
        .collect()
}

fn jobs<'a>(species: &'a [(&'static str, Species)]) -> Vec<(&'a Species, GenParams)> {
    let mut jobs = Vec::new();
    for (slot, (_, species)) in species.iter().enumerate() {
        let sampler = Sampler {
            seed: 40 + slot as u64,
            age: Distribution::Uniform { lo: 3.0, hi: 7.0 },
            scale: Distribution::Uniform { lo: 0.8, hi: 1.2 },
            quality: 0.7,
        };
        jobs.extend((0..4).map(|index| (species, sampler.params(index))));
    }
    jobs
}

fn pool(threads: usize) -> rayon::ThreadPool {
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .unwrap()
}

#[test]
fn generate_many_equals_sequential_generate_for_any_thread_count() {
    let species = all_presets();
    let jobs = jobs(&species);
    let sequential: Vec<Plant> = jobs
        .iter()
        .map(|(species, params)| generate(species, params).unwrap())
        .collect();
    for threads in [1, 3, 8] {
        let batch = pool(threads).install(|| generate_many(&jobs));
        assert_eq!(batch.len(), sequential.len());
        for (index, (got, want)) in batch.into_iter().zip(&sequential).enumerate() {
            assert_eq!(&got.unwrap(), want, "job {index} on {threads} threads");
        }
    }
}

#[test]
fn run_batch_returns_results_in_index_order() {
    let order = pool(4).install(|| run_batch(100, |_, _, index| index));
    assert_eq!(order, (0..100).collect::<Vec<_>>());
    assert_eq!(run_batch(0, |_, _, index| index), Vec::<usize>::new());
}

#[test]
fn a_reused_generator_gives_the_same_plants() {
    let species = all_presets();
    let jobs = jobs(&species);
    let mut generator = Generator::new();
    let mut scratch = Plant::default();
    for (species, params) in jobs.iter().rev() {
        generator
            .generate_into(species, params, &mut scratch)
            .unwrap();
        assert_eq!(scratch, generate(species, params).unwrap());
    }
}

#[test]
fn remesh_equals_generating_at_that_quality() {
    let species = preset("broadleaf").unwrap();
    let params = GenParams::new(5, 6);
    let mut generator = Generator::new();
    let mut plant = generator.generate(&species, &params).unwrap();
    let coarse = params.with_quality(0.5);
    generator
        .remesh(&mut plant, &species, &coarse.mesh)
        .unwrap();
    assert_eq!(plant, generate(&species, &coarse).unwrap());
}

#[test]
fn statistics_describe_the_plant() {
    let species = preset("conifer").unwrap();
    let plant = generate(&species, &GenParams::new(2, 6)).unwrap();
    let stats = plant.stats;
    assert_eq!(stats.nodes, plant.graph.node_count());
    assert_eq!(stats.chains, plant.graph.chain_count());
    assert_eq!(stats.triangles, plant.mesh.triangle_count());
    let vertices: usize = plant
        .mesh
        .parts
        .iter()
        .map(|p| p.buffers.positions.len())
        .sum();
    assert_eq!(stats.vertices, vertices);
    let bounds = stats.bounds.unwrap();
    assert!(stats.height > 0.0);
    assert!((bounds.size()[1] - stats.height).abs() < 1e-6);
}

#[test]
fn invalid_parameters_are_errors() {
    let species = preset("shrub").unwrap();
    assert!(generate(&species, &GenParams::new(1, 3).with_scale(0.0)).is_err());
    let mut params = GenParams::new(1, 3);
    params.mesh.leaf_fraction = 2.0;
    assert!(generate(&species, &params).is_err());
    let mut broken = species;
    broken.foliage.width_ratio = -1.0;
    assert!(generate(&broken, &GenParams::new(1, 3)).is_err());
}

/// Checks the container, the JSON and every accessor with the `gltf` crate, and the
/// metadata in the extras.
fn check_glb(bytes: &[u8], plant: &Plant, name: &str) {
    let gltf = gltf::Gltf::from_slice(bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
    let blob = gltf.blob.as_deref().expect("binary chunk");
    for accessor in gltf.accessors() {
        let view = accessor.view().expect("accessor with a buffer view");
        let end = view.offset() + accessor.offset() + accessor.count() * accessor.size();
        assert!(
            end <= blob.len(),
            "{name}: accessor past the end of the buffer"
        );
    }
    let mesh = gltf.meshes().next().expect("one mesh");
    let vertices: usize = mesh
        .primitives()
        .map(|p| p.get(&gltf::Semantic::Positions).unwrap().count())
        .sum();
    assert_eq!(vertices, plant.stats.vertices, "{name}");
    let extras = gltf
        .nodes()
        .next()
        .unwrap()
        .extras()
        .as_ref()
        .expect("extras");
    let extras: serde_json::Value = serde_json::from_str(extras.get()).unwrap();
    assert_eq!(extras["seed"], plant.params.seed, "{name}");
    let species: Species = serde_json::from_value(extras["species"].clone()).unwrap();
    species.validate().unwrap();
}

#[test]
fn every_preset_generates_and_exports() {
    for (name, species) in all_presets() {
        for quality in [1.0, 0.25] {
            let params = GenParams::new(1, 8).with_quality(quality);
            let plant = generate(&species, &params).unwrap();
            assert!(plant.stats.triangles > 0, "{name}");
            let mut bytes = Vec::new();
            let options = GlbOptions {
                name: name.to_owned(),
                generator: GENERATOR.to_owned(),
                extras: Some(metadata(&species, &plant)),
            };
            write_glb(&plant.mesh, &options, &mut bytes).unwrap();
            check_glb(&bytes, &plant, name);
        }
    }
}

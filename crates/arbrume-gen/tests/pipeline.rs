//! Batch determinism, generator reuse and export of every preset.

use arbrume_core::species::Species;
use arbrume_export::{GlbOptions, write_glb};
use arbrume_gen::{
    Budget, Distribution, GENERATOR, GenError, GenParams, Generator, Occluder, Plant,
    RandomSampler, Sampler, Shape, Surroundings, generate, generate_many, metadata, preset,
    presets, run_batch,
};
use glam::Vec3;

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

#[test]
fn every_preset_grows_soundly_over_its_range_of_ages() {
    let names: Vec<&str> = presets().iter().map(|(name, _)| *name).collect();
    assert!(names.len() >= 16, "{names:?}");
    for (name, species) in all_presets() {
        let [young, old] = species.variation.age;
        let mut tallest = 0.0_f32;
        for age in [young, young.midpoint(old), old] {
            let plant = generate(&species, &GenParams::new(11, age).with_quality(0.5)).unwrap();
            let graph = &plant.graph;
            graph.check_invariants().unwrap();
            assert!(graph.is_contiguous(), "{name} at {age}");
            assert!(plant.stats.leaf_sites > 0, "{name} at {age} has no foliage");
            assert_eq!(plant.mesh.parts.len(), 2, "{name} at {age}");
            let finite = |part: &arbrume_core::mesh::MeshPart| {
                let positions = part.buffers.positions.iter();
                positions.flatten().all(|c| c.is_finite())
            };
            assert!(plant.mesh.parts.iter().all(finite), "{name} at {age}");
            let radii = graph.radii().iter();
            assert!(radii.clone().all(|r| *r > 0.0 && r.is_finite()), "{name}");
            assert!(plant.stats.height > 0.05, "{name} at {age}");
            assert!(
                plant.stats.height >= 0.8 * tallest,
                "{name} shrinks at {age}"
            );
            tallest = tallest.max(plant.stats.height);
        }
    }
}

#[test]
fn every_preset_has_distinct_name_and_jitter() {
    let species = all_presets();
    let mut names: Vec<&str> = species.iter().map(|(_, s)| s.name.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), species.len());
    for (name, species) in &species {
        assert!(!species.variation.jitter.is_empty(), "{name}");
        assert_ne!(&species.varied(1, 0), species, "{name}");
    }
}

#[test]
fn random_plants_do_not_depend_on_the_number_of_threads() {
    let sampler = RandomSampler::new(21);
    let make = |generator: &mut Generator, plant: &mut Plant, index: usize| {
        let pick = sampler.pick(index as u32).unwrap();
        pick.generate(generator, plant).unwrap();
        (pick.preset, plant.clone())
    };
    let sequential = pool(1).install(|| run_batch(20, make));
    for threads in [3, 8] {
        let batch = pool(threads).install(|| run_batch(20, make));
        assert_eq!(batch, sequential, "{threads} threads");
    }
    let presets: std::collections::BTreeSet<&str> =
        sequential.iter().map(|(name, _)| name.as_str()).collect();
    assert!(presets.len() > 5);
}

#[test]
fn a_budget_lowers_the_quality_until_the_mesh_fits() {
    let species = preset("maple_like").unwrap();
    let params = GenParams::new(4, 35);
    let full = generate(&species, &params).unwrap();
    let mut generator = Generator::new();
    let mut plant = Plant::default();
    let unlimited = Budget::NONE;
    generator
        .generate_within(&species, &params, &unlimited, &mut plant)
        .unwrap();
    assert_eq!(plant, full);
    let budget = Budget {
        triangles: Some(full.stats.triangles / 4),
        vertices: Some(full.stats.vertices / 3),
    };
    generator
        .generate_within(&species, &params, &budget, &mut plant)
        .unwrap();
    assert!(plant.stats.triangles <= full.stats.triangles / 4);
    assert!(plant.stats.vertices <= full.stats.vertices / 3);
    assert!(plant.stats.triangles > full.stats.triangles / 12);
    assert!(plant.params.mesh.tolerance > params.mesh.tolerance);
    assert_eq!(plant.graph, full.graph);
    let again = generate(&species, &plant.params).unwrap();
    assert_eq!(again.mesh, plant.mesh);
    let roomy = Budget {
        triangles: Some(full.stats.triangles),
        vertices: None,
    };
    generator
        .generate_within(&species, &params, &roomy, &mut plant)
        .unwrap();
    assert_eq!(plant, full);
}

/// The presets in the self-organizing mode.
fn competing_presets() -> Vec<(&'static str, Species)> {
    let competing = |(_, species): &(&str, Species)| {
        species.growth.mode == arbrume_core::species::GrowthMode::SelfOrganizing
    };
    all_presets().into_iter().filter(competing).collect()
}

#[test]
fn self_organizing_presets_do_not_depend_on_the_number_of_threads() {
    let species = competing_presets();
    assert!(species.len() >= 6, "{} presets", species.len());
    for (name, species) in &species {
        let [young, old] = species.variation.age;
        let params = GenParams::new(21, young.midpoint(old).min(16)).with_quality(0.4);
        let reference = generate(species, &params).unwrap();
        assert!(reference.stats.nodes > 50, "{name}");
        for threads in [1, 2, 8] {
            let plant = pool(threads).install(|| generate(species, &params).unwrap());
            assert_eq!(plant, reference, "{name} on {threads} threads");
        }
    }
}

#[test]
fn surroundings_shape_self_organizing_plants_only() {
    let wall = Shape::Box {
        min: Vec3::new(0.6, 0.0, -20.0),
        max: Vec3::new(1.0, 30.0, 20.0),
    };
    let surroundings = Surroundings {
        envelope: None,
        occluders: vec![Occluder::obstacle(wall.clone())],
    };
    let open = GenParams::new(5, 12).with_quality(0.4);
    let walled = open.clone().with_surroundings(surroundings);
    for (name, species) in competing_presets() {
        let (free, held) = (
            generate(&species, &open).unwrap(),
            generate(&species, &walled).unwrap(),
        );
        assert_ne!(free.graph, held.graph, "{name}");
        let inside = held.graph.positions().iter().any(|&p| wall.contains(p));
        assert!(!inside, "{name} grows into the wall");
        let extras = metadata(&species, &held);
        assert_eq!(extras["surroundings"]["occluders"][0]["shape"], "box");
        assert!(metadata(&species, &free)["surroundings"].is_null());
    }
    let parametric = preset("broadleaf").unwrap();
    let (free, held) = (
        generate(&parametric, &open).unwrap(),
        generate(&parametric, &walled).unwrap(),
    );
    assert_eq!(free.graph, held.graph);
    let flat = Shape::Box {
        min: Vec3::ZERO,
        max: Vec3::new(1.0, 0.0, 1.0),
    };
    let empty = Surroundings {
        envelope: Some(flat),
        occluders: Vec::new(),
    };
    let result = generate(&parametric, &open.with_surroundings(empty));
    assert!(matches!(
        result,
        Err(GenError::InvalidParam {
            field: "surroundings",
            ..
        })
    ));
}

#[test]
fn growth_frames_of_self_organizing_presets_grow_into_the_plant() {
    for (name, species) in competing_presets() {
        let [young, old] = species.variation.age;
        let params = GenParams::new(8, young.midpoint(old).min(18)).with_quality(0.4);
        let mut generator = Generator::new();
        let timeline = generator.timeline(&species, &params).unwrap();
        let full = generator.generate(&species, &params).unwrap();
        let mut plant = Plant::default();
        let cycles = timeline.frame_cycles(4);
        let mut nodes = Vec::new();
        for &cycle in &cycles {
            generator
                .frame(&species, &timeline, cycle, &mut plant)
                .unwrap();
            let graph = &plant.graph;
            graph.check_invariants().unwrap();
            let shown = graph.births().iter().zip(graph.flags());
            nodes.push(shown.filter(|(b, f)| **b <= cycle && !f.is_shed()).count());
            assert!(plant.stats.triangles > 0, "{name} at cycle {cycle}");
        }
        let live = full.graph.flags().iter().filter(|f| !f.is_shed()).count();
        assert_eq!(nodes.last(), Some(&live), "{name}");
        assert!(nodes[0] < live, "{name}: {nodes:?}");
        let bark = |plant: &Plant| plant.mesh.parts[0].buffers.positions.len();
        assert_eq!(bark(&plant), bark(&full), "{name}");
    }
}

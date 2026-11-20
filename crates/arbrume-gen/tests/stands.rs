//! Stands generated with `Generator::generate_group`: determinism, order and thread
//! independence, neighbours shading each other, and a stand of one plant.

use arbrume_core::species::Species;
use arbrume_gen::{GenError, GenParams, Generator, GroupEntry, Plant, Surroundings, generate};
use arbrume_gen::{Occluder, Shape, preset};
use glam::Vec3;

/// FNV-1a over the bits of the graph node positions and of the mesh vertices.
fn hash(plant: &Plant) -> u64 {
    let graph = plant.graph.positions().iter().flat_map(Vec3::to_array);
    let mesh = plant
        .mesh
        .parts
        .iter()
        .flat_map(|part| part.buffers.positions.iter().flatten().copied());
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for value in graph.chain(mesh) {
        for byte in value.to_bits().to_le_bytes() {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    hash
}

fn hashes(plants: &[Plant]) -> Vec<u64> {
    plants.iter().map(hash).collect()
}

/// One parametric and two self-organizing presets.
fn three() -> [Species; 3] {
    ["broadleaf", "open_grown_broadleaf", "hedge_shrub"].map(|name| preset(name).unwrap())
}

fn stand(species: &[Species; 3]) -> Vec<GroupEntry<'_>> {
    vec![
        (
            &species[0],
            GenParams::new(1, 8).with_quality(0.4),
            [-3.0, 0.0, 0.0],
        ),
        (
            &species[1],
            GenParams::new(2, 10).with_quality(0.4),
            [0.0, 0.0, 0.5],
        ),
        (
            &species[2],
            GenParams::new(3, 7).with_quality(0.4),
            [1.5, 0.0, -0.5],
        ),
    ]
}

fn group(entries: &[GroupEntry]) -> Vec<Plant> {
    Generator::new().generate_group(entries).unwrap()
}

#[test]
fn a_stand_is_the_same_on_every_run_and_thread_count() {
    let species = three();
    let entries = stand(&species);
    let want = hashes(&group(&entries));
    assert_eq!(hashes(&group(&entries)), want);
    let single = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap();
    assert_eq!(hashes(&single.install(|| group(&entries))), want);
    let mut reversed = entries.clone();
    reversed.reverse();
    let mut got = hashes(&group(&reversed));
    got.reverse();
    assert_eq!(got, want);
}

/// Hashes of two plants of `name` grown together `distance` meters apart, and alone.
fn pair(name: &str, distance: f32) -> ([u64; 2], [u64; 2]) {
    let species = preset(name).unwrap();
    let params = |seed| GenParams::new(seed, 10).with_quality(0.4);
    let half = 0.5 * distance;
    let entries = [
        (&species, params(1), [-half, 0.0, 0.0]),
        (&species, params(2), [half, 0.0, 0.0]),
    ];
    let together = group(&entries);
    let alone = [1, 2].map(|seed| hash(&generate(&species, &params(seed)).unwrap()));
    ([hash(&together[0]), hash(&together[1])], alone)
}

#[test]
fn close_neighbours_shade_each_other_and_far_ones_do_not() {
    for name in ["open_grown_broadleaf", "excurrent_conifer"] {
        let (together, alone) = pair(name, 1.0);
        assert_ne!(together[0], alone[0], "{name}");
        assert_ne!(together[1], alone[1], "{name}");
        // Recorded: 50 m apart the shadows never meet, but the voxels of a plant away
        // from the origin are not aligned with its own, so the plants differ slightly.
        let (together, alone) = pair(name, 50.0);
        assert_ne!(together[0], alone[0], "{name}");
        assert_ne!(together[1], alone[1], "{name}");
    }
}

#[test]
fn a_stand_of_one_plant_at_the_origin_is_the_plant_generated_alone() {
    for (name, age) in [("broadleaf", 8), ("open_grown_broadleaf", 10)] {
        let species = preset(name).unwrap();
        let params = GenParams::new(5, age).with_quality(0.5);
        let alone = generate(&species, &params).unwrap();
        let stand = group(&[(&species, params.clone(), [0.0; 3])]);
        assert_eq!(stand.len(), 1);
        assert_eq!(stand[0], alone, "{name}");
    }
}

#[test]
fn the_plants_of_a_stand_share_one_surroundings() {
    let species = preset("hedge_shrub").unwrap();
    let wall = Surroundings {
        envelope: None,
        occluders: vec![Occluder::obstacle(Shape::Box {
            min: Vec3::new(2.0, 0.0, -3.0),
            max: Vec3::new(2.3, 3.0, 3.0),
        })],
    };
    let params = GenParams::new(1, 6).with_quality(0.4);
    let walled = params.clone().with_surroundings(wall);
    let entries = [
        (&species, walled.clone(), [0.0; 3]),
        (&species, walled, [1.0, 0.0, 0.0]),
    ];
    assert_eq!(group(&entries).len(), 2);
    let mixed = [(&species, params, [0.0; 3]), entries[1].clone()];
    let error = Generator::new().generate_group(&mixed).unwrap_err();
    assert!(matches!(
        error,
        GenError::InvalidParam {
            field: "surroundings",
            ..
        }
    ));
    let far = [(&species, GenParams::new(1, 6), [f32::NAN, 0.0, 0.0])];
    assert!(Generator::new().generate_group(&far).is_err());
    assert_eq!(Generator::new().generate_group(&[]).unwrap().len(), 0);
}

//! Placement: determinism, distances, densities, terrain heights and exclusions.

use arbrume_forest::{
    Exclusion, Exclusions, Hills, Instance, Mode, PlaceSettings, Shape, SpeciesRule, Terrain,
    place, table,
};

fn two_species() -> Vec<SpeciesRule> {
    vec![
        SpeciesRule {
            density: 80.0,
            spacing: 4.0,
            variants: 3,
            lean_deg: 4.0,
            tint: 0.1,
            ..SpeciesRule::default()
        },
        SpeciesRule {
            density: 120.0,
            spacing: 3.0,
            variants: 2,
            ..SpeciesRule::default()
        },
    ]
}

fn hills() -> Terrain {
    let hills = Hills {
        amplitude: 40.0,
        wavelength: 120.0,
        octaves: 4,
        resolution: 2.0,
        seed: 3,
    };
    Terrain::hills([-150.0, -100.0], [300.0, 200.0], &hills).unwrap()
}

fn bits(trees: &[Instance]) -> Vec<u8> {
    let mut out = Vec::new();
    table::write_binary(trees, &mut out).unwrap();
    out
}

#[test]
fn placement_does_not_depend_on_threads_nor_bands() {
    let terrain = hills();
    let rules = two_species();
    let run = |threads: usize, band_rows: usize, mode: Mode| {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        let settings = PlaceSettings {
            seed: 11,
            mode,
            band_rows,
        };
        pool.install(|| place(&terrain, &Exclusions::default(), &rules, &settings))
            .unwrap()
            .0
    };
    for mode in [Mode::Poisson, Mode::Competition] {
        let reference = bits(&run(1, 0, mode));
        assert!(reference.len() > 1000);
        assert_eq!(bits(&run(8, 0, mode)), reference);
        assert_eq!(bits(&run(3, 1, mode)), reference);
        assert_eq!(bits(&run(4, 7, mode)), reference);
    }
    let other = PlaceSettings {
        seed: 12,
        ..PlaceSettings::default()
    };
    let changed = place(&terrain, &Exclusions::default(), &rules, &other)
        .unwrap()
        .0;
    assert_ne!(bits(&changed), bits(&run(1, 0, Mode::Poisson)));
}

/// Smallest distance ratio `d / limit` over every pair of trees, by brute force on a
/// grid of buckets.
fn closest_ratio(trees: &[Instance], limit: impl Fn(&Instance, &Instance) -> f32) -> f32 {
    let bucket = 8.0;
    let key = |t: &Instance| {
        (
            (t.position[0] / bucket).floor() as i64,
            (t.position[2] / bucket).floor() as i64,
        )
    };
    let mut buckets = std::collections::HashMap::<(i64, i64), Vec<usize>>::new();
    for (index, tree) in trees.iter().enumerate() {
        buckets.entry(key(tree)).or_default().push(index);
    }
    let mut ratio = f32::INFINITY;
    for (index, tree) in trees.iter().enumerate() {
        let (bx, bz) = key(tree);
        for dx in -1..=1 {
            for other in buckets
                .get(&(bx + dx, bz - 1))
                .into_iter()
                .chain(buckets.get(&(bx + dx, bz)))
                .chain(buckets.get(&(bx + dx, bz + 1)))
                .flatten()
            {
                if *other == index {
                    continue;
                }
                let o = &trees[*other];
                let d = (o.position[0] - tree.position[0]).hypot(o.position[2] - tree.position[2]);
                ratio = ratio.min(d / limit(tree, o));
            }
        }
    }
    ratio
}

#[test]
fn trees_keep_their_distances() {
    let terrain = Terrain::flat([0.0, 0.0], [200.0, 200.0], 0.0);
    let rules = two_species();
    let (trees, _) = place(
        &terrain,
        &Exclusions::default(),
        &rules,
        &PlaceSettings::default(),
    )
    .unwrap();
    let mean = |a: &Instance, b: &Instance| {
        f32::midpoint(
            rules[a.species as usize].spacing,
            rules[b.species as usize].spacing,
        )
    };
    assert!(closest_ratio(&trees, mean) >= 1.0);
    let competition = PlaceSettings {
        mode: Mode::Competition,
        ..PlaceSettings::default()
    };
    let (trees, _) = place(&terrain, &Exclusions::default(), &rules, &competition).unwrap();
    let scaled = |a: &Instance, b: &Instance| {
        let size = |t: &Instance| {
            let rule = &rules[t.species as usize];
            rule.spacing * t.scale / rule.mean_scale()
        };
        f32::midpoint(size(a), size(b))
    };
    assert!(closest_ratio(&trees, scaled) >= 1.0);
}

/// The densities of the Poisson mode are those asked, within four standard deviations of
/// a Poisson count of the expected mean: the thinned process is more regular than a
/// Poisson process, so its count varies less and the bound is loose.
#[test]
fn densities_are_reached() {
    let terrain = Terrain::flat([0.0, 0.0], [400.0, 400.0], 0.0);
    let rules = two_species();
    for seed in [1, 2, 3] {
        let settings = PlaceSettings {
            seed,
            ..PlaceSettings::default()
        };
        let (trees, report) = place(&terrain, &Exclusions::default(), &rules, &settings).unwrap();
        for (s, rule) in rules.iter().enumerate() {
            let expected = f64::from(rule.density) * 16.0;
            let kept = trees.iter().filter(|t| t.species as usize == s).count() as f64;
            assert!((report.species[s].expected_open - expected).abs() < 1e-6 * expected);
            assert!(
                (kept - expected).abs() <= 4.0 * expected.sqrt(),
                "species {s}, seed {seed}: {kept} trees for {expected}"
            );
        }
    }
}

#[test]
fn unreachable_densities_are_reported() {
    let terrain = Terrain::flat([0.0, 0.0], [100.0, 100.0], 0.0);
    let rule = SpeciesRule {
        density: 1000.0,
        spacing: 4.0,
        ..SpeciesRule::default()
    };
    let (trees, report) = place(
        &terrain,
        &Exclusions::default(),
        &[rule],
        &PlaceSettings::default(),
    )
    .unwrap();
    assert!(report.species[0].saturated_cells > 0);
    let ceiling = 1e4 / (std::f64::consts::PI * 16.0);
    assert!((trees.len() as f64) < ceiling + 4.0 * ceiling.sqrt());
}

#[test]
fn trees_stand_on_the_ground() {
    let heights: Vec<f32> = (0..16)
        .map(|i| (i % 4) as f32 * 3.0 + (i / 4) as f32)
        .collect();
    let terrain = Terrain::from_grid([-30.0, -30.0], [60.0, 60.0], 4, 4, heights).unwrap();
    let rule = SpeciesRule {
        density: 400.0,
        spacing: 2.0,
        ..SpeciesRule::default()
    };
    let (trees, _) = place(
        &terrain,
        &Exclusions::default(),
        std::slice::from_ref(&rule),
        &PlaceSettings::default(),
    )
    .unwrap();
    for tree in &trees {
        let [x, y, z] = tree.position;
        assert!((y - terrain.height(x, z)).abs() < 1e-4);
        let gx = (x + 30.0) / 20.0;
        let gz = (z + 30.0) / 20.0;
        assert!((terrain.height(x, z) - (3.0 * gx + gz)).abs() < 1e-3);
    }
    let sunk = SpeciesRule {
        trunk_radius: 0.5,
        sink: 0.1,
        ..rule
    };
    let (trees, _) = place(
        &terrain,
        &Exclusions::default(),
        &[sunk],
        &PlaceSettings::default(),
    )
    .unwrap();
    for tree in &trees {
        let [x, y, z] = tree.position;
        assert!(y < terrain.height(x, z) - 0.099);
    }
}

#[test]
fn a_sixteen_bit_heightmap_is_read() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("h.png");
    let file = std::fs::File::create(&path).unwrap();
    let mut encoder = png::Encoder::new(file, 3, 2);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Sixteen);
    let mut writer = encoder.write_header().unwrap();
    let values: [u16; 6] = [0, 32768, 65535, 0, 0, 0];
    let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_be_bytes()).collect();
    writer.write_image_data(&bytes).unwrap();
    writer.finish().unwrap();
    let terrain = Terrain::from_png(&path, [0.0, 0.0], [20.0, 10.0], [10.0, 110.0]).unwrap();
    assert_eq!(terrain.samples(), [3, 2]);
    assert!((terrain.height(20.0, 0.0) - 110.0).abs() < 1e-3);
    assert!((terrain.height(10.0, 0.0) - (10.0 + 100.0 * 32768.0 / 65535.0)).abs() < 1e-3);
    assert!((terrain.height(15.0, 5.0) - (10.0 + 0.25 * (50.0 + 100.0) * 1.0)).abs() < 0.1);
}

#[test]
fn exclusions_and_preferences_are_respected() {
    let heights = vec![0.0, 0.0, 40.0, 40.0];
    let terrain = Terrain::from_grid([0.0, 0.0], [200.0, 200.0], 2, 2, heights).unwrap();
    let clearing = Exclusion {
        shape: Shape::Circle {
            center: [60.0, 60.0],
            radius: 25.0,
        },
        falloff: 5.0,
        carve: 0.0,
    };
    let river = Exclusion {
        shape: Shape::Path {
            points: vec![[0.0, 150.0], [100.0, 140.0], [200.0, 160.0]],
            width: 12.0,
        },
        falloff: 0.0,
        carve: 0.0,
    };
    let exclusions =
        Exclusions::new(vec![clearing.clone(), river.clone()], None, Some(2.0)).unwrap();
    let lowland = SpeciesRule {
        density: 250.0,
        spacing: 3.0,
        altitude: [0.0, 25.0],
        ..SpeciesRule::default()
    };
    let (trees, _) = place(&terrain, &exclusions, &[lowland], &PlaceSettings::default()).unwrap();
    assert!(trees.len() > 400);
    for tree in &trees {
        let [x, _, z] = tree.position;
        assert!(clearing.shape.distance(x, z) > 0.0);
        assert!(river.shape.distance(x, z) > 0.0);
        let ground = terrain.height(x, z);
        assert!((2.0..=25.0).contains(&ground), "tree at {ground} m");
    }
}

#[test]
fn the_binary_table_reads_back() {
    let terrain = hills();
    let (trees, _) = place(
        &terrain,
        &Exclusions::default(),
        &two_species(),
        &PlaceSettings::default(),
    )
    .unwrap();
    let bytes = bits(&trees);
    assert_eq!(
        bytes.len(),
        table::HEADER_BYTES + table::RECORD_BYTES * trees.len()
    );
    assert_eq!(table::read_binary(bytes.as_slice()).unwrap(), trees);
}

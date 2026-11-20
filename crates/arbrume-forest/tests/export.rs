//! A small grown forest written to disk: the instanced file, the variant files, the
//! tables, the coupled stand and the manifest.

use std::path::Path;

use arbrume_export::ExportOptions;
use arbrume_forest::{COUPLED_VARIANT, Forest, ForestConfig, table, write_forest};
use serde_json::Value;

const CONFIG: &str = r#"
name = "tiny"
seed = 5

[terrain]
size = [40.0, 40.0]
hills = { amplitude = 6.0, wavelength = 30.0 }

[[species]]
species = "shrub"
density = 250.0
age = 4
variants = 2
tint = 0.1
lean = 3.0

[[species]]
species = "thicket_shrub"
density = 120.0
spacing = 2.5
age = 4
variants = 2

[[exclude]]
circle = { center = [5.0, 5.0], radius = 6.0 }

[library]
lods = 2
billboard = "cross"
atlas = 128
textures = false

[coupled]
region = [-20.0, -20.0, -5.0, -5.0]
tile = 8.0
halo = 3.0
quality = 0.3
"#;

/// The JSON chunk of a glb file.
fn glb_json(path: &Path) -> Value {
    let bytes = std::fs::read(path).unwrap();
    let length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    serde_json::from_slice(&bytes[20..20 + length]).unwrap()
}

/// Instances per group node of an instanced file.
fn instances_per_group(json: &Value) -> Vec<(String, u64)> {
    let nodes = json["nodes"].as_array().unwrap();
    let accessors = json["accessors"].as_array().unwrap();
    nodes[0]["children"]
        .as_array()
        .unwrap()
        .iter()
        .map(|group| {
            let group = &nodes[group.as_u64().unwrap() as usize];
            let count = group["children"].as_array().map_or(0, |children| {
                children
                    .iter()
                    .map(|child| {
                        let node = &nodes[child.as_u64().unwrap() as usize];
                        let attributes =
                            &node["extensions"]["EXT_mesh_gpu_instancing"]["attributes"];
                        let translation = attributes["TRANSLATION"].as_u64().unwrap();
                        let color = attributes["_COLOR_0"].as_u64().unwrap();
                        let count = accessors[translation as usize]["count"].as_u64().unwrap();
                        assert_eq!(accessors[color as usize]["count"].as_u64(), Some(count));
                        assert_eq!(accessors[color as usize]["type"], "VEC3");
                        count
                    })
                    .sum()
            });
            (group["name"].as_str().unwrap().to_owned(), count)
        })
        .collect()
}

#[test]
fn a_grown_forest_is_written() {
    let config = ForestConfig::from_toml_str(CONFIG).unwrap();
    let forest = Forest::build(&config, Path::new("."), true).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let written = write_forest(
        &forest,
        &config.output,
        &ExportOptions::default(),
        dir.path(),
    )
    .unwrap();
    assert!(written.iter().all(|w| w.bytes > 0));
    let coupled = forest.coupled.as_ref().unwrap();
    assert!(!coupled.trees.is_empty());
    for tree in &coupled.trees {
        let [x, _, z] = forest.instances[tree.instance].position;
        assert!((-20.0..-5.0).contains(&x) && (-20.0..-5.0).contains(&z));
    }
    let drawn = (forest.instances.len() - coupled.trees.len()) as u64;

    let instanced = glb_json(&dir.path().join("tiny_instanced.glb"));
    assert_eq!(instanced["extensionsUsed"][0], "EXT_mesh_gpu_instancing");
    let groups = instances_per_group(&instanced);
    let names: Vec<&str> = groups.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["tiny_LOD0", "tiny_LOD1", "tiny_LOD2"]);
    assert!(groups.iter().all(|(_, count)| *count == drawn));

    let variant = glb_json(&dir.path().join("shrub_v1.glb"));
    let root = &variant["nodes"][0];
    assert_eq!(root["children"].as_array().unwrap().len(), 3);

    let stand = glb_json(&dir.path().join("tiny_coupled.glb"));
    assert_eq!(
        stand["nodes"][0]["children"].as_array().unwrap().len(),
        coupled.trees.len()
    );

    let bytes = std::fs::read(dir.path().join("tiny_instances.bin")).unwrap();
    let rows = table::read_binary(bytes.as_slice()).unwrap();
    assert_eq!(rows.len(), forest.instances.len());
    let marked = rows.iter().filter(|r| r.variant == COUPLED_VARIANT).count();
    assert_eq!(marked, coupled.trees.len());
    let csv = std::fs::read_to_string(dir.path().join("tiny_instances.csv")).unwrap();
    assert_eq!(csv.lines().count(), forest.instances.len() + 1);

    let manifest: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.path().join("tiny.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["trees"], forest.instances.len());
    assert_eq!(
        manifest["species"][0]["variants"].as_array().unwrap().len(),
        2
    );
}

#[test]
fn the_grown_forest_does_not_depend_on_threads() {
    let config = ForestConfig::from_toml_str(CONFIG).unwrap();
    let run = |threads: usize| {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        let forest = pool
            .install(|| Forest::build(&config, Path::new("."), true))
            .unwrap();
        let mut bytes = Vec::new();
        table::write_binary(&forest.instances, &mut bytes).unwrap();
        for tree in &forest.coupled.as_ref().unwrap().trees {
            for p in tree
                .plant
                .mesh
                .parts
                .iter()
                .flat_map(|p| &p.buffers.positions)
            {
                bytes.extend(p.iter().flat_map(|v| v.to_le_bytes()));
            }
        }
        for variant in forest.library.iter().flat_map(|s| &s.variants) {
            for p in variant.levels[0]
                .mesh
                .parts
                .iter()
                .flat_map(|p| &p.buffers.positions)
            {
                bytes.extend(p.iter().flat_map(|v| v.to_le_bytes()));
            }
        }
        bytes
    };
    assert_eq!(run(1), run(6));
}

//! Rooted presets through the pipeline: labels, wind, junctions of both tiers, growth
//! sequences and the soil of the parameters.

use arbrume_core::mesh::{PartKind, SemanticClass};
use arbrume_export::annotations::MeshLabels;
use arbrume_gen::{GenParams, Generator, Ground, Junctions, Soil, preset};
use arbrume_mesh::topology::check_surface;

const ROOTED: [&str; 4] = [
    "maple_like_rooted",
    "oak_like_rooted",
    "palm_like_rooted",
    "spruce_like_rooted",
];

#[test]
fn root_vertices_are_labelled_root_and_stay_still_in_the_wind() {
    for name in ROOTED {
        let species = preset(name).unwrap();
        let mut params = GenParams::new(2, 12);
        params.mesh.wind = true;
        let mut generator = Generator::new();
        let plant = generator.generate(&species, &params).unwrap();
        let provenance = generator.provenance(&plant, &species).unwrap();
        let labels = MeshLabels::from_provenance(&provenance);
        let bark = plant
            .mesh
            .parts
            .iter()
            .position(|p| p.kind == PartKind::Bark);
        let bark = bark.unwrap();
        let own = labels.part(bark);
        let root = SemanticClass::Root as u8;
        let wind = &plant.mesh.parts[bark].buffers.wind;
        let mut roots = 0;
        for (vertex, &class) in own.semantic.iter().enumerate() {
            if class == root {
                roots += 1;
                let x = &wind[vertex][..4];
                assert!(
                    x.iter().all(|&v| v < 0.2),
                    "{name}: root vertex moves {x:?}"
                );
            }
        }
        assert!(roots > 100, "{name}: {roots} root vertices");
        assert!(
            own.semantic.contains(&(SemanticClass::Trunk as u8)),
            "{name}"
        );
    }
}

#[test]
fn rooted_presets_mesh_with_both_tiers() {
    for name in ROOTED {
        let species = preset(name).unwrap();
        for junctions in [Junctions::TierA, Junctions::TierB] {
            let mut params = GenParams::new(1, 8);
            params.mesh.junctions = junctions;
            let plant = Generator::new().generate(&species, &params).unwrap();
            let bark = &plant.mesh.parts[0].buffers;
            let report = check_surface(&bark.positions, &bark.indices, false);
            assert_eq!(report.degenerate, 0, "{name} {junctions:?}");
            if junctions == Junctions::TierB {
                assert_eq!(report.boundary_edges, 0, "{name}: {report:?}");
                assert_eq!(report.misoriented_edges, 0, "{name}: {report:?}");
            }
        }
    }
}

#[test]
fn the_last_growth_frame_carries_the_roots() {
    let species = preset("spruce_like_rooted").unwrap();
    let params = GenParams::new(3, 9);
    let mut generator = Generator::new();
    let full = generator.generate(&species, &params).unwrap();
    let timeline = generator.timeline(&species, &params).unwrap();
    let mut plant = arbrume_gen::Plant::default();
    let cycles = timeline.frame_cycles(3);
    generator
        .frame(&species, &timeline, cycles[0], &mut plant)
        .unwrap();
    let young = plant.graph.flags().iter().filter(|f| f.is_root()).count();
    generator
        .frame(&species, &timeline, *cycles.last().unwrap(), &mut plant)
        .unwrap();
    assert_eq!(plant.graph, full.graph);
    assert_eq!(plant.mesh, full.mesh);
    let old = plant.graph.flags().iter().filter(|f| f.is_root()).count();
    assert!(young > 0 && young < old, "{young} {old}");
}

#[test]
fn the_soil_of_the_parameters_shapes_the_roots() {
    let species = preset("maple_like_rooted").unwrap();
    let flat = arbrume_gen::generate(&species, &GenParams::new(4, 10)).unwrap();
    let slope = Soil {
        ground: Ground::Slope { rise: [0.4, 0.0] },
        resources: Vec::new(),
    };
    let params = GenParams::new(4, 10).with_soil(slope);
    let sloped = arbrume_gen::generate(&species, &params).unwrap();
    assert_ne!(flat.graph, sloped.graph);
    let graph = &sloped.graph;
    for node in graph.node_ids() {
        if graph.flags()[node.index()].is_root() && !graph.is_shed(node) {
            let p = graph.positions()[node.index()];
            assert!(p.y + graph.radii()[node.index()] <= 0.4 * p.x + 1e-4);
        }
    }
}

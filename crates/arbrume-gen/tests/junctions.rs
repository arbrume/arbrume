//! Tier B junctions on every preset at two ages: the bark of each stem is a closed
//! manifold. Self-intersections are counted and printed, split between those of two
//! axes that meet at a junction and the others (axes that cross in the grown skeleton);
//! the trees used to choose the method have none at their junctions.

use arbrume_core::graph::{ChainId, PlantGraph};
use arbrume_core::mesh::{PartKind, TreeMesh};
use arbrume_gen::{GenParams, Generator, preset, presets};
use arbrume_mesh::topology::{SurfaceReport, check_surface};
use arbrume_mesh::{Junctions, Mesher};

/// Ages checked for every preset.
const AGES: [u16; 2] = [5, 12];

/// Presets whose junctions must not intersect anything.
const CLEAN: [&str; 4] = ["broadleaf", "oak_like", "pine_like", "open_grown_broadleaf"];

fn bark(mesh: &TreeMesh) -> &arbrume_core::mesh::MeshBuffers {
    &mesh
        .parts
        .iter()
        .find(|part| part.kind == PartKind::Bark)
        .unwrap()
        .buffers
}

/// Whether two chains meet at a junction: the same chain, parent and child, or two
/// children of one node.
fn joined(graph: &PlantGraph, a: u32, b: u32) -> bool {
    let attach = |chain: u32| graph.chain(ChainId(chain)).attach;
    let parent = |chain: u32| {
        let node = attach(chain);
        (!node.is_none()).then(|| graph.chain_of(node).0)
    };
    a == b
        || parent(a) == Some(b)
        || parent(b) == Some(a)
        || (!attach(a).is_none() && attach(a) == attach(b))
}

/// Root chains with a meshed bark.
fn stems(graph: &PlantGraph, chains: &[u32]) -> usize {
    let mut roots: Vec<u32> = chains
        .iter()
        .copied()
        .filter(|&chain| graph.chain(ChainId(chain)).attach.is_none())
        .collect();
    roots.sort_unstable();
    roots.dedup();
    roots.len()
}

fn check(name: &str, age: u16) -> (SurfaceReport, usize, usize) {
    let species = preset(name).unwrap();
    let mut params = GenParams::new(1, age);
    params.mesh.junctions = Junctions::TierB;
    let plant = Generator::new().generate(&species, &params).unwrap();
    let bark = bark(&plant.mesh);
    let report = check_surface(&bark.positions, &bark.indices, true);
    let provenance = Mesher::new()
        .provenance(&plant.graph, &species, &params.mesh)
        .unwrap();
    let runs = &provenance.parts[0].1;
    let chains: Vec<u32> = bark
        .indices
        .chunks(3)
        .map(|triangle| {
            let vertex = triangle[0];
            let run = runs
                .iter()
                .find(|run| vertex >= run.start && vertex < run.start + run.len);
            run.unwrap().instance
        })
        .collect();
    let local = report
        .intersections
        .iter()
        .filter(|&&(a, b)| joined(&plant.graph, chains[a as usize], chains[b as usize]))
        .count();
    (report, local, stems(&plant.graph, &chains))
}

#[test]
fn every_preset_is_closed_per_stem() {
    let mut failures = Vec::new();
    for &(name, _) in presets() {
        if preset(name).unwrap().climbing.is_some() {
            // Stems pressed on a support and wrapped over its edges are not closed by Tier B.
            continue;
        }
        for age in AGES {
            let (report, local, stems) = check(name, age);
            eprintln!(
                "{name} age {age}: {} triangles, {} components for {stems} stems, \
                 {} crossings ({local} at junctions), {} sharp creases",
                report.triangles,
                report.components,
                report.intersections.len(),
                report.creases
            );
            let crossing = local > 0 && CLEAN.contains(&name);
            if !report.is_closed_manifold() || report.components != stems || crossing {
                failures.push(format!("{name} age {age}: {report:?}"));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

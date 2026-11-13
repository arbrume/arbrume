//! Writes the mesh of a hand-built tree as a `.glb` file, to look at the mesher's output.
//!
//! Usage: `cargo run -p arbrume-mesh --example demo_tree -- <out.glb> [quality] [seed] [species.toml]`
//!
//! The skeleton comes from `arbrume_mesh::synthetic`, not from a growth model. `quality`
//! is the level of detail (1 by default), `seed` selects the skeleton, and the optional
//! species file sets bark, foliage and geometry parameters.

use arbrume_core::species::Species;
use arbrume_export::{GlbOptions, write_glb_file};
use arbrume_mesh::synthetic::{SkeletonParams, skeleton};
use arbrume_mesh::{MeshOptions, mesh_tree};
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let usage = "usage: demo_tree <out.glb> [quality] [seed] [species.toml]";
    let path = args.next().ok_or(usage)?;
    let quality: f32 = args.next().map_or(Ok(1.0), |text| text.parse())?;
    let seed: u64 = args.next().map_or(Ok(1), |text| text.parse())?;
    let species = match args.next() {
        Some(file) => Species::from_path(file)?,
        None => Species::default(),
    };
    let graph = skeleton(&SkeletonParams::small(seed));
    let options = MeshOptions {
        seed,
        ..MeshOptions::with_quality(quality)
    };
    let start = Instant::now();
    let mesh = mesh_tree(&graph, &species, &options)?;
    let elapsed = start.elapsed();
    for part in &mesh.parts {
        println!(
            "{:?}: {} vertices, {} triangles",
            part.kind,
            part.buffers.positions.len(),
            part.buffers.triangle_count()
        );
    }
    println!(
        "{} nodes, {} chains, meshed in {elapsed:?}",
        graph.node_count(),
        graph.chain_count()
    );
    let glb = GlbOptions {
        name: "demo_tree".to_owned(),
        ..GlbOptions::default()
    };
    write_glb_file(&mesh, &glb, &path)?;
    println!("wrote {path}");
    Ok(())
}

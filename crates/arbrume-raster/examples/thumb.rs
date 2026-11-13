//! Grows and meshes the three starter species and writes a PNG thumbnail of each.
//!
//! Usage: `cargo run --release -p arbrume-raster --example thumb -- <out_dir> [size] [age] [seed] [azimuth]`
//!
//! Writes `<out_dir>/<species>.png` (and a JPEG beside it) and prints the triangle count
//! and render time of each plant. `size` is the side in pixels (256 by default), `age`
//! the number of growth cycles (15), `seed` the plant seed (1), `azimuth` the view
//! azimuth in degrees (0).

use arbrume_core::species::Species;
use arbrume_grow::{GrowParams, grow};
use arbrume_mesh::{MeshOptions, mesh_tree};
use arbrume_raster::{Renderer, ThumbnailOptions};
use std::path::PathBuf;
use std::time::Instant;

const STARTERS: [&str; 3] = ["broadleaf", "conifer", "shrub"];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let usage = "usage: thumb <out_dir> [size] [age] [seed] [azimuth]";
    let out = PathBuf::from(args.next().ok_or(usage)?);
    let size: u32 = args.next().map_or(Ok(256), |text| text.parse())?;
    let age: u16 = args.next().map_or(Ok(15), |text| text.parse())?;
    let seed: u64 = args.next().map_or(Ok(1), |text| text.parse())?;
    let azimuth: f32 = args.next().map_or(Ok(0.0), |text| text.parse())?;
    std::fs::create_dir_all(&out)?;
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../arbrume-grow/tests/data");
    let mut renderer = Renderer::new();
    for name in STARTERS {
        let species = Species::from_path(data.join(format!("{name}.toml")))?;
        let graph = grow(
            &species,
            &GrowParams {
                seed,
                age,
                scale: 1.0,
            },
        );
        let options = MeshOptions {
            seed,
            ..MeshOptions::default()
        };
        let mesh = mesh_tree(&graph, &species, &options)?;
        let thumbnail = ThumbnailOptions {
            width: size,
            height: size,
            azimuth,
            ..ThumbnailOptions::default()
        };
        let start = Instant::now();
        let image = renderer.thumbnail(&mesh, &thumbnail);
        let elapsed = start.elapsed();
        let png = out.join(format!("{name}.png"));
        image.write_png(&png)?;
        std::fs::write(
            out.join(format!("{name}.jpg")),
            image.to_jpeg(85, [255; 3])?,
        )?;
        println!(
            "{name}: {} triangles, rendered in {elapsed:?}, wrote {}",
            mesh.triangle_count(),
            png.display()
        );
        for part in &mesh.parts {
            println!(
                "  {:?}: {} triangles, base colour {:?}",
                part.kind,
                part.buffers.triangle_count(),
                part.material.base_color
            );
        }
    }
    Ok(())
}

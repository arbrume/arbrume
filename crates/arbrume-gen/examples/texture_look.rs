//! Renders presets with and without procedural textures, for tuning by eye.
//!
//! `cargo run --release -p arbrume-gen --example texture_look -- OUT_DIR [PRESET...]`
//! writes `<preset>_look.png` for each preset: the top row without textures, the bottom
//! row with them, and in each row the whole plant, a close-up of the base of the trunk
//! and a close-up of a twig at the edge of the crown, all rendered with
//! `arbrume-raster`. It also prints the synthesis time of the textures of each preset.

use arbrume_core::mesh::{PartKind, TreeMesh};
use arbrume_gen::{GenParams, Generator, TextureCache, TextureOptions, preset};
use arbrume_raster::{Camera, Image, Renderer, ThumbnailOptions};
use glam::Vec3;
use std::path::PathBuf;
use std::sync::Arc;

/// Side of one view, in pixels.
const SIDE: u32 = 480;

/// Points of the parts of `kind`.
fn points(mesh: &TreeMesh, kind: PartKind) -> Vec<Vec3> {
    mesh.parts
        .iter()
        .filter(|part| part.kind == kind)
        .flat_map(|part| part.buffers.positions.iter().map(|&p| Vec3::from(p)))
        .collect()
}

/// The three views of `mesh`: whole plant, base of the trunk, a twig of the crown edge.
fn views(renderer: &mut Renderer, mesh: &TreeMesh) -> [Image; 3] {
    let options = ThumbnailOptions {
        width: SIDE,
        height: SIDE,
        azimuth: 20.0,
        ..ThumbnailOptions::default()
    };
    let whole = renderer.thumbnail(mesh, &options);
    let bark = points(mesh, PartKind::Bark);
    let low = bark.iter().map(|p| p.y).fold(f32::MAX, f32::min);
    let trunk: Vec<Vec3> = bark
        .iter()
        .copied()
        .filter(|p| p.y < low + 0.5 && p.x.hypot(p.z) < 0.6)
        .collect();
    let leaves = points(mesh, PartKind::Leaf);
    let edge = leaves
        .iter()
        .copied()
        .max_by(|a, b| (a.x + 0.3 * a.y).total_cmp(&(b.x + 0.3 * b.y)))
        .unwrap_or(Vec3::Y);
    let twig: Vec<Vec3> = leaves
        .iter()
        .copied()
        .filter(|p| p.distance(edge) < 0.25)
        .collect();
    let close = |renderer: &mut Renderer, focus: &[Vec3]| {
        let mut scene = options.scene(mesh);
        scene.camera = Camera::frame_points(focus, 35.0, 10.0, 30.0, 0.05);
        scene.ground = None;
        renderer.render(mesh, &scene).to_image()
    };
    let trunk_view = close(renderer, &trunk);
    let twig_view = close(renderer, &twig);
    [whole, trunk_view, twig_view]
}

/// Copies `image` into `sheet` (of width `width`) at `(left, top)`.
fn blit(sheet: &mut [u8], width: u32, image: &Image, (left, top): (u32, u32)) {
    for y in 0..image.height {
        for x in 0..image.width {
            let to = 4 * ((top + y) * width + left + x) as usize;
            sheet[to..to + 4].copy_from_slice(&image.pixel(x, y));
        }
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let out = PathBuf::from(
        args.next()
            .expect("usage: texture_look OUT_DIR [PRESET...]"),
    );
    let mut names: Vec<String> = args.collect();
    if names.is_empty() {
        names = [
            "oak_like",
            "birch_like",
            "pine_like",
            "maple_like",
            "spruce_like",
        ]
        .map(String::from)
        .to_vec();
    }
    std::fs::create_dir_all(&out).expect("output directory");
    let cache = Arc::new(TextureCache::new(TextureOptions::default()));
    let mut renderer = Renderer::new();
    for name in &names {
        let species = preset(name).expect("preset");
        let params = GenParams::new(3, GenParams::default().age);
        let mut plain = Generator::new();
        let mut textured = Generator::new();
        textured.set_textures(Some(Arc::clone(&cache)));
        let start = std::time::Instant::now();
        let synthesized = cache.get(&species).expect("synthesized textures");
        println!(
            "{name}: textures {:.1} ms (bark {:.1}, leaf {:.1}, encode {:.1}), {} KiB",
            start.elapsed().as_secs_f64() * 1e3,
            synthesized.timings.bark.as_secs_f64() * 1e3,
            synthesized.timings.leaf.as_secs_f64() * 1e3,
            synthesized.timings.encode.as_secs_f64() * 1e3,
            synthesized.bytes() / 1024
        );
        let rows = [
            plain.generate(&species, &params).expect("plant").mesh,
            textured.generate(&species, &params).expect("plant").mesh,
        ];
        let width = 3 * SIDE;
        let mut sheet = vec![0_u8; (width * 2 * SIDE * 4) as usize];
        for (row, mesh) in rows.iter().enumerate() {
            for (column, image) in views(&mut renderer, mesh).iter().enumerate() {
                let at = (column as u32 * SIDE, row as u32 * SIDE);
                blit(&mut sheet, width, image, at);
            }
        }
        let image = Image {
            width,
            height: 2 * SIDE,
            pixels: sheet,
        };
        let path = out.join(format!("{name}_look.png"));
        std::fs::write(&path, image.to_png().expect("png")).expect("write");
    }
}

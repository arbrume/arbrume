//! Contact sheet of the levels of detail of a few presets, for judging the switches by
//! eye.
//!
//! ```text
//! cargo run --release -p arbrume-lod --example lod_sheet -- out.png SIZE[@AZ,EL] preset:age ...
//! ```
//!
//! One row per plant; one column per mesh level, then the billboard cloud and the cross
//! billboard, every image `SIZE` pixels square, seen from azimuth `AZ` and elevation
//! `EL` in degrees (20 and 12 by default), framed like level 0 and lit the same way. Prints the triangles, the screen-size hints and the times of each level.

use std::time::Instant;

use arbrume_gen::{
    BillboardMethod, BillboardOptions, GenParams, Generator, LodKind, LodSettings, preset,
};
use arbrume_raster::{Image, Renderer, ThumbnailOptions};

/// Copies `image` into `sheet` (of width `width`) at column `x`, row `y`.
fn blit(sheet: &mut [u8], width: u32, image: &Image, (x, y): (u32, u32)) {
    for row in 0..image.height {
        let from = (row * image.width * 4) as usize;
        let to = (((y + row) * width + x) * 4) as usize;
        let len = (image.width * 4) as usize;
        sheet[to..to + len].copy_from_slice(&image.pixels[from..from + len]);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let out = args
        .next()
        .ok_or("usage: lod_sheet out.png SIZE preset:age ...")?;
    let view = args.next().ok_or("missing SIZE")?;
    let (size, angles) = view.split_once('@').unwrap_or((&view, "20,12"));
    let size: u32 = size.parse()?;
    let (azimuth, elevation) = angles.split_once(',').ok_or("angles are AZ,EL")?;
    let (azimuth, elevation): (f32, f32) = (azimuth.parse()?, elevation.parse()?);
    let plants: Vec<String> = args.collect();
    let columns = 5;
    let width = size * columns;
    let height = size * plants.len() as u32;
    let mut sheet = vec![255_u8; (width * height * 4) as usize];
    let mut generator = Generator::new();
    let mut renderer = Renderer::new();
    for (row, plant) in plants.iter().enumerate() {
        let (name, age) = plant.split_once(':').unwrap_or((plant, "20"));
        let species = preset(name).ok_or("unknown preset")?;
        let grown = generator.generate(&species, &GenParams::new(1, age.parse()?))?;
        let mut settings = LodSettings {
            billboard: Some(BillboardOptions {
                method: BillboardMethod::Cloud,
                ..BillboardOptions::default()
            }),
            billboard_source: 1,
            wind: true,
            ..LodSettings::default()
        };
        let start = Instant::now();
        let mut chain = generator.lod_chain(&grown, &species, &settings)?;
        let elapsed = start.elapsed();
        settings.billboard = Some(BillboardOptions {
            method: BillboardMethod::Cross,
            ..BillboardOptions::default()
        });
        let cross = generator.lod_chain(&grown, &species, &settings)?;
        if let Some(last) = cross
            .into_iter()
            .last()
            .filter(|l| l.kind == LodKind::Billboard)
        {
            chain.push(last);
        }
        let options = ThumbnailOptions {
            width: size,
            height: size,
            azimuth,
            elevation,
            ..ThumbnailOptions::default()
        };
        let scene = options.scene(&chain[0].mesh);
        println!("{name}:{age} chain {elapsed:?}");
        for (column, level) in chain.iter().enumerate().take(columns as usize) {
            println!(
                "  {:?} tris {} budget {} H {:.0}px time {:?}",
                level.kind, level.triangles, level.budget, level.max_screen_height, level.time
            );
            let image = renderer.render(&level.mesh, &scene).to_image();
            blit(
                &mut sheet,
                width,
                &image,
                (column as u32 * size, row as u32 * size),
            );
        }
    }
    let image = Image {
        width,
        height,
        pixels: sheet,
    };
    image.write_png(out)?;
    Ok(())
}

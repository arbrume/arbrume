//! Synthesis time and size of the textures of every preset, as a Markdown table.
//!
//! `RAYON_NUM_THREADS=1 cargo run --release -p arbrume-gen --example texture_bench -- [MAX]`
//! synthesizes the textures of each preset with `max_resolution = MAX` (default: the
//! resolutions of the presets) and prints the time of each step and the encoded size.
//! With one thread the times are the CPU cost a batch pays per species.

use arbrume_gen::{TextureOptions, preset, presets};
use arbrume_texture::synthesize;

fn main() {
    let max = std::env::args()
        .nth(1)
        .map_or(TextureOptions::default().max_resolution, |a| {
            a.parse().expect("MAX is a number")
        });
    let options = TextureOptions {
        seed: 0,
        max_resolution: max,
    };
    println!("| preset | bark px | leaf px | bark ms | leaf ms | encode ms | total ms | KiB |");
    println!("|---|---|---|---|---|---|---|---|");
    let (mut time, mut bytes) = (0.0, 0);
    for &(name, _) in presets() {
        let species = preset(name).expect("preset");
        let textures = synthesize(&species, &options);
        let t = textures.timings;
        let ms = |d: std::time::Duration| d.as_secs_f64() * 1e3;
        let size =
            |m: &Option<arbrume_texture::MaterialTextures>| m.as_ref().map_or(0, |m| m.resolution);
        println!(
            "| {name} | {} | {} | {:.1} | {:.1} | {:.1} | {:.1} | {} |",
            size(&textures.bark),
            size(&textures.leaf),
            ms(t.bark),
            ms(t.leaf),
            ms(t.encode),
            ms(t.total()),
            textures.bytes() / 1024
        );
        time += ms(t.total());
        bytes += textures.bytes();
    }
    println!("\nall presets: {time:.0} ms, {} KiB", bytes / 1024);
}

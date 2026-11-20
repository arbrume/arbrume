//! Contact sheets of the synthesized textures, for tuning by eye.
//!
//! `cargo run --release -p arbrume-texture --example swatches -- OUT_DIR [FILE.toml...]`
//! writes `bark_sheet.png` (each bark kind as a 2 x 2 repetition, so seams would show,
//! lit from the upper left through its normal map, and its albedo alone) and
//! `leaf_sheet.png` (each leaf shape: the atlas lit, over grey), and prints the
//! synthesis time of each texture. Species files given on the command line are added to
//! both sheets.

use arbrume_core::species::Species;
use arbrume_texture::{Image, bark_maps, leaf_maps};
use glam::Vec3;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Bark swatches: label and the `[bark]` table.
const BARKS: &[(&str, &str)] = &[
    (
        "smooth",
        "color = [0.62, 0.6, 0.55]\npattern = { kind = \"smooth\", feature_size = 0.05, stretch = 0.6, coverage = 0.45, contrast = 0.35, color = [0.05, 0.04, 0.035] }",
    ),
    (
        "furrowed",
        "color = [0.14, 0.12, 0.1]\npattern = { kind = \"furrowed\", feature_size = 0.045, stretch = 3.5, coverage = 0.4, relief = 0.7 }",
    ),
    (
        "plated",
        "color = [0.2, 0.12, 0.08]\npattern = { kind = \"plated\", feature_size = 0.08, stretch = 1.6, coverage = 0.3, contrast = 0.5, relief = 0.6 }",
    ),
    (
        "fibrous",
        "color = [0.25, 0.13, 0.08]\npattern = { kind = \"fibrous\", feature_size = 0.02, stretch = 8.0, coverage = 0.5, contrast = 0.4, relief = 0.5 }",
    ),
    (
        "peeling",
        "color = [0.32, 0.3, 0.24]\npattern = { kind = \"peeling\", feature_size = 0.09, stretch = 1.3, coverage = 0.45, contrast = 0.6, color = [0.55, 0.5, 0.3], irregularity = 0.8 }",
    ),
];

/// Leaf swatches: label and the `[foliage]` table.
const LEAVES: &[(&str, &str)] = &[
    (
        "ovate serrate",
        "shape = \"ovate\"\nwidth_ratio = 0.6\nmargin = { kind = \"serrate\", teeth = 18, depth = 0.08 }",
    ),
    (
        "elliptic entire",
        "shape = \"elliptic\"\nwidth_ratio = 0.5\nvenation = { veins = 9 }",
    ),
    (
        "lanceolate",
        "shape = \"lanceolate\"\nwidth_ratio = 0.25\nvenation = { veins = 10, angle_deg = 35.0 }\nmargin = { kind = \"crenate\", teeth = 24, depth = 0.05 }",
    ),
    (
        "obovate lobed",
        "shape = \"obovate\"\nwidth_ratio = 0.6\nvenation = { veins = 5, angle_deg = 50.0 }\nmargin = { kind = \"lobed\", depth = 0.55 }",
    ),
    (
        "palmate lobed",
        "shape = \"ovate\"\nwidth_ratio = 0.95\nvenation = { kind = \"palmate\", veins = 2, angle_deg = 65.0 }\nmargin = { kind = \"lobed\", depth = 0.5 }",
    ),
    (
        "open venation",
        "shape = \"elliptic\"\nwidth_ratio = 0.55\nvenation = { kind = \"open\", width = 0.03, tertiary = 0.0 }",
    ),
    (
        "parallel",
        "shape = \"lanceolate\"\nwidth_ratio = 0.15\nvenation = { kind = \"parallel\", veins = 4 }",
    ),
    (
        "compound",
        "shape = \"lanceolate\"\nwidth_ratio = 0.35\nleaflet_pairs = 4\npetiole_length = 0.1\nmargin = { kind = \"serrate\", teeth = 10, depth = 0.1 }",
    ),
    (
        "needle",
        "shape = \"needle\"\nwidth_ratio = 0.08\nsegments = 1\ntexture_variants = 2",
    ),
    (
        "needle spray",
        "shape = \"needle\"\nwidth_ratio = 0.1\nlength = 0.03\nsegments = 1\ntexture_variants = 1\ntwig = { leaves = 12, length = 0.4, fins = 2, fin_width = 0.04, inclination_deg = 55.0 }",
    ),
];

/// RGBA canvas of the sheets, 8-bit sRGB.
struct Sheet {
    width: usize,
    data: Vec<[u8; 3]>,
}

impl Sheet {
    fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            data: vec![[40, 40, 40]; width * height],
        }
    }

    /// Draws `pixel(x, y)` for a `size`-texel square at `(left, top)`.
    fn tile(
        &mut self,
        left: usize,
        top: usize,
        size: usize,
        pixel: impl Fn(usize, usize) -> [u8; 3],
    ) {
        for y in 0..size {
            for x in 0..size {
                self.data[(top + y) * self.width + left + x] = pixel(x, y);
            }
        }
    }

    fn save(&self, path: &Path) {
        let image = Image {
            width: self.width as u32,
            height: (self.data.len() / self.width) as u32,
            channels: 3,
            srgb: true,
            data: self.data.iter().flatten().copied().collect(),
        };
        std::fs::write(path, image.to_png()).expect("cannot write the sheet");
    }
}

/// The factor the material puts on a texture of mean `mean` for the colour `color`,
/// applied to sRGB codes (approximately, with a 2.2 gamma).
fn factor(color: [f32; 3], mean: [f32; 3]) -> Vec3 {
    Vec3::from_array(std::array::from_fn(|c| {
        (color[c] / mean[c]).min(1.0).powf(1.0 / 2.2)
    }))
}

/// Encoded albedo `albedo` times `tint`, lit through `normal` from the upper left, over
/// `background`.
fn lit(albedo: glam::Vec4, normal: glam::Vec4, background: Vec3, tint: Vec3) -> [u8; 3] {
    let n = (Vec3::new(normal.x, normal.y, normal.z) * 2.0 - 1.0).normalize_or_zero();
    let light = Vec3::new(-0.5, -0.5, 0.7).normalize();
    let shade = 0.35 + 0.75 * n.dot(light).max(0.0);
    let color = Vec3::new(albedo.x, albedo.y, albedo.z) * tint * shade;
    let color = background.lerp(color, albedo.w);
    color
        .to_array()
        .map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn bark_sheet(out: &Path, extra: &[Species]) {
    const SIZE: usize = 256;
    let mut barks: Vec<(String, Species)> = BARKS
        .iter()
        .map(|(label, table)| {
            let species = Species::from_toml_str(&format!("[bark]\n{table}")).expect("bark");
            ((*label).to_owned(), species)
        })
        .collect();
    barks.extend(extra.iter().map(|s| (s.name.clone(), s.clone())));
    let mut sheet = Sheet::new(barks.len() * (2 * SIZE + 8), 4 * SIZE + 8);
    for (index, (label, species)) in barks.iter().enumerate() {
        let start = Instant::now();
        let maps = bark_maps(&species.bark, SIZE as u32, 1);
        println!(
            "bark {label}: {:.1} ms at {SIZE}^2",
            start.elapsed().as_secs_f64() * 1e3
        );
        let left = index * (2 * SIZE + 8);
        let tint = factor(species.bark.material_color(), maps.mean);
        let pixel = |x: usize, y: usize| {
            let (x, y) = ((x % SIZE) as u32, (y % SIZE) as u32);
            lit(
                maps.albedo.texel(x, y),
                maps.normal.texel(x, y),
                Vec3::ZERO,
                tint,
            )
        };
        sheet.tile(left, 0, 2 * SIZE, pixel);
        let albedo = |x: usize, y: usize| {
            let t = maps.albedo.texel((x % SIZE) as u32, (y % SIZE) as u32);
            let t = Vec3::new(t.x, t.y, t.z) * tint;
            t.to_array().map(|c| (c.min(1.0) * 255.0).round() as u8)
        };
        sheet.tile(left, 2 * SIZE + 8, 2 * SIZE, albedo);
    }
    sheet.save(&out.join("bark_sheet.png"));
}

fn leaf_sheet(out: &Path, extra: &[Species]) {
    const SIZE: usize = 512;
    let mut leaves: Vec<(String, Species)> = LEAVES
        .iter()
        .map(|(label, table)| {
            let species = Species::from_toml_str(&format!("[foliage]\n{table}")).expect("leaf");
            ((*label).to_owned(), species)
        })
        .collect();
    leaves.extend(extra.iter().map(|s| (s.name.clone(), s.clone())));
    let columns = 5;
    let rows = leaves.len().div_ceil(columns);
    let mut sheet = Sheet::new(columns * (SIZE + 8), rows * (SIZE + 8));
    let grey = Vec3::splat(0.75);
    for (index, (label, species)) in leaves.iter().enumerate() {
        let start = Instant::now();
        let maps = leaf_maps(&species.foliage, &species.bark, SIZE as u32, 3);
        println!(
            "leaf {label}: {:.1} ms at {SIZE}^2",
            start.elapsed().as_secs_f64() * 1e3
        );
        let (left, top) = (
            (index % columns) * (SIZE + 8),
            (index / columns) * (SIZE + 8),
        );
        let tint = factor(species.foliage.material_color(&species.bark), maps.mean);
        sheet.tile(left, top, SIZE, |x, y| {
            let (x, y) = (x as u32, y as u32);
            lit(maps.albedo.texel(x, y), maps.normal.texel(x, y), grey, tint)
        });
    }
    sheet.save(&out.join("leaf_sheet.png"));
}

fn main() {
    let mut args = std::env::args().skip(1);
    let out = PathBuf::from(args.next().expect("usage: swatches OUT_DIR [FILE.toml...]"));
    let extra: Vec<Species> = args
        .map(|path| Species::from_path(&path).expect("species file"))
        .collect();
    std::fs::create_dir_all(&out).expect("cannot create the output directory");
    bark_sheet(&out, &extra);
    leaf_sheet(&out, &extra);
}

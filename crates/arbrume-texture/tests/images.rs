//! Image textures: files named by a species replace the synthesized bark and leaves.

use arbrume_core::species::{LeafAtlas, Species};
use arbrume_texture::{Image, TextureOptions, decode_image, load_textures, synthesize};

fn solid(side: u32, pixel: [u8; 4]) -> Vec<u8> {
    Image {
        width: side,
        height: side,
        channels: 4,
        srgb: true,
        data: pixel.repeat((side * side) as usize),
    }
    .to_png()
}

/// A leaf image: opaque in its left half, transparent in its right half.
fn half_leaf(side: u32) -> Vec<u8> {
    let mut data = Vec::new();
    for _ in 0..side {
        for x in 0..side {
            let alpha = if x < side / 2 { 255 } else { 0 };
            data.extend([40, 160, 30, alpha]);
        }
    }
    Image {
        width: side,
        height: side,
        channels: 4,
        srgb: true,
        data,
    }
    .to_png()
}

fn species(dir: &std::path::Path) -> Species {
    let mut species = Species::from_toml_str(
        "name = \"Imaged\"\n[bark]\ntexture = \"textures/bark.png\"\n\
         [foliage]\ntexture = \"textures/leaf.png\"\ntexture_resolution = 128",
    )
    .unwrap();
    species.asset_dir = Some(dir.to_path_buf());
    species
}

#[test]
fn bark_and_leaf_images_replace_the_synthesized_maps() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("textures")).unwrap();
    std::fs::write(
        dir.path().join("textures/bark.png"),
        solid(64, [90, 60, 30, 255]),
    )
    .unwrap();
    std::fs::write(dir.path().join("textures/leaf.png"), half_leaf(64)).unwrap();
    let species = species(dir.path());
    let textures = load_textures(&species, &TextureOptions::default()).unwrap();
    let bark = textures.bark.as_ref().unwrap();
    assert_eq!(bark.resolution, 64);
    assert!(bark.roughness.is_none());
    let albedo = decode_image(&bark.base_color).unwrap();
    assert_eq!(&albedo.data[..4], &[90, 60, 30, 255]);
    assert!(bark.mean.iter().all(|c| *c > 0.0 && *c < 1.0));

    let leaf = textures.leaf.as_ref().unwrap();
    let atlas = textures.atlas.unwrap();
    let image = decode_image(&leaf.base_color).unwrap();
    let rect = atlas.cell_rect(0);
    let at = |u: f32, v: f32| {
        let x = ((rect[0] + u * (rect[2] - rect[0])) * image.width as f32) as u32;
        let y = ((rect[1] + v * (rect[3] - rect[1])) * image.height as f32) as u32;
        image.texel(x, y)
    };
    assert!(at(0.25, 0.5).w > 0.99, "left half of the leaf is opaque");
    assert!(at(0.75, 0.5).w < 0.01, "right half is transparent");
    let [u, v] = LeafAtlas::STEM_UV;
    assert!(at(u, v).w > 0.99, "the stalk point stays opaque");

    let synthesized = synthesize(&species, &TextureOptions::default());
    assert_ne!(synthesized.bark, textures.bark);
    let capped = TextureOptions {
        max_resolution: 32,
        ..TextureOptions::default()
    };
    let small = load_textures(&species, &capped).unwrap();
    assert_eq!(small.bark.unwrap().resolution, 32);
}

#[test]
fn missing_and_invalid_images_are_errors_naming_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let species = species(dir.path());
    let error = load_textures(&species, &TextureOptions::default()).unwrap_err();
    assert!(error.path.ends_with("textures/bark.png"), "{error}");
    std::fs::create_dir(dir.path().join("textures")).unwrap();
    std::fs::write(dir.path().join("textures/bark.png"), b"not an image").unwrap();
    let error = load_textures(&species, &TextureOptions::default()).unwrap_err();
    assert!(error.reason.contains("not a PNG or JPEG"), "{error}");
    std::fs::write(dir.path().join("textures/bark.png"), solid(48, [0; 4])).unwrap();
    let error = load_textures(&species, &TextureOptions::default()).unwrap_err();
    assert!(error.to_string().contains("48 x 48"), "{error}");
}

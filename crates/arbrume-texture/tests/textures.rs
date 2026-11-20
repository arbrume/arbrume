//! Tileability, determinism, formats and colour calibration of the synthesized textures.

use arbrume_core::mesh::{Material, MeshBuffers, MeshPart, PartKind, TreeMesh};
use arbrume_core::species::{BarkKind, LeafAtlas, Species};
use arbrume_texture::{
    Image, MaterialTextures, TextureCache, TextureOptions, bark_maps, leaf_maps, synthesize,
    textured_species,
};
use std::sync::Arc;

const KINDS: [BarkKind; 5] = [
    BarkKind::Smooth,
    BarkKind::Furrowed,
    BarkKind::Plated,
    BarkKind::Fibrous,
    BarkKind::Peeling,
];

fn species(kind: BarkKind) -> Species {
    let mut species = Species::default();
    species.bark.pattern.kind = kind;
    species.bark.texture_resolution = 128;
    species.foliage.texture_resolution = 128;
    species
}

/// Mean absolute difference between columns `a` and `b` of `image`, over all channels.
fn column_step(image: &Image, a: u32, b: u32) -> f32 {
    let sum: f32 = (0..image.height)
        .map(|y| (image.texel(a, y) - image.texel(b, y)).abs().element_sum())
        .sum();
    sum / image.height as f32
}

/// Mean absolute difference between rows `a` and `b` of `image`.
fn row_step(image: &Image, a: u32, b: u32) -> f32 {
    let sum: f32 = (0..image.width)
        .map(|x| (image.texel(x, a) - image.texel(x, b)).abs().element_sum())
        .sum();
    sum / image.width as f32
}

/// The seam of a tiled image (last column next to the first, last row next to the
/// first) must look like any boundary between two neighbouring columns or rows inside
/// it: its mean step may not exceed the largest interior step. A texture that does not
/// tile has a seam step of the order of the full contrast of the image, far above that.
fn assert_tiles(image: &Image, label: &str) {
    let (w, h) = (image.width, image.height);
    let inner_x = (0..w - 1)
        .map(|x| column_step(image, x, x + 1))
        .fold(0.0, f32::max);
    let inner_y = (0..h - 1)
        .map(|y| row_step(image, y, y + 1))
        .fold(0.0, f32::max);
    let seam_x = column_step(image, w - 1, 0);
    let seam_y = row_step(image, h - 1, 0);
    assert!(
        seam_x <= inner_x,
        "{label}: seam across {seam_x} > {inner_x}"
    );
    assert!(
        seam_y <= inner_y,
        "{label}: seam along {seam_y} > {inner_y}"
    );
    let mean_x = (0..w - 1)
        .map(|x| column_step(image, x, x + 1))
        .sum::<f32>()
        / (w - 1) as f32;
    let far = (0..w / 2)
        .map(|x| column_step(image, x, x + w / 2))
        .sum::<f32>()
        / (w / 2) as f32;
    assert!(
        mean_x < far,
        "{label}: unrelated columns differ more than neighbours"
    );
}

#[test]
fn every_bark_kind_tiles_in_both_directions() {
    for kind in KINDS {
        let species = species(kind);
        let maps = bark_maps(&species.bark, 128, 5);
        assert_tiles(&maps.albedo, &format!("{kind:?} albedo"));
        assert_tiles(&maps.normal, &format!("{kind:?} normal"));
        assert_tiles(&maps.roughness, &format!("{kind:?} roughness"));
    }
}

#[test]
fn bark_tiles_with_a_rectangular_repeat() {
    let mut species = species(BarkKind::Furrowed);
    species.bark.texture_size = [0.3, 0.9];
    assert_tiles(
        &bark_maps(&species.bark, 128, 2).albedo,
        "rectangular repeat",
    );
}

/// Decodes PNG `bytes` to (width, height, colour type) and 8-bit samples.
fn decode(bytes: &[u8]) -> (u32, u32, png::ColorType, Vec<u8>) {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .unwrap();
    let mut data = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut data).unwrap();
    data.truncate(info.buffer_size());
    (info.width, info.height, info.color_type, data)
}

fn srgb_to_linear(code: u8) -> f32 {
    let encoded = f32::from(code) / 255.0;
    if encoded <= 0.040_45 {
        encoded / 12.92
    } else {
        ((encoded + 0.055) / 1.055).powf(2.4)
    }
}

#[test]
fn maps_have_the_documented_sizes_and_formats() {
    let mut species = species(BarkKind::Plated);
    species.foliage.texture_resolution = 64;
    let textures = synthesize(&species, &TextureOptions::default());
    let bark = textures.bark.as_ref().unwrap();
    let leaf = textures.leaf.as_ref().unwrap();
    let expect = |bytes: &[u8], size, kind| {
        let (width, height, color, _) = decode(bytes);
        assert_eq!((width, height, color), (size, size, kind));
    };
    expect(&bark.base_color, 128, png::ColorType::Rgb);
    expect(&bark.normal, 128, png::ColorType::Rgb);
    expect(
        bark.roughness.as_ref().unwrap(),
        128,
        png::ColorType::Grayscale,
    );
    expect(&leaf.base_color, 64, png::ColorType::Rgba);
    expect(&leaf.normal, 64, png::ColorType::Rgb);
    assert!(leaf.roughness.is_none());
    let capped = synthesize(
        &species,
        &TextureOptions {
            seed: 0,
            max_resolution: 32,
        },
    );
    assert_eq!(capped.bark.unwrap().resolution, 32);
    let bare = Species::from_toml_str("[foliage]\nenabled = false").unwrap();
    assert!(synthesize(&bare, &TextureOptions::default()).leaf.is_none());
}

#[test]
fn recorded_mean_is_the_mean_albedo_and_fits_under_one() {
    for kind in KINDS {
        let species = species(kind);
        let maps = bark_maps(&species.bark, 64, 9);
        let target = maps.mean;
        assert!(target.iter().all(|&m| m > 0.0 && m <= 1.0));
        let mut sum = [0.0_f64; 3];
        for chunk in maps.albedo.data.as_chunks::<3>().0 {
            for channel in 0..3 {
                sum[channel] += f64::from(srgb_to_linear(chunk[channel]));
            }
        }
        for channel in 0..3 {
            let mean = sum[channel] / (64.0 * 64.0);
            let wanted = f64::from(target[channel]);
            let tolerance = 0.03 * wanted + 0.004;
            assert!(
                (mean - wanted).abs() < tolerance,
                "{kind:?} {mean} {wanted}"
            );
        }
    }
}

#[test]
fn leaf_cells_are_transparent_at_their_edges_and_opaque_at_the_stem() {
    for extra in [
        "",
        "twig = { leaves = 6, fins = 2, fin_width = 0.03 }",
        "shape = \"needle\"",
    ] {
        let species = Species::from_toml_str(&format!("[foliage]\n{extra}")).unwrap();
        let maps = leaf_maps(&species.foliage, &species.bark, 256, 4);
        let atlas: LeafAtlas = maps.atlas;
        let size = 256.0;
        let alpha = |uv: [f32; 2]| {
            let x = ((uv[0] * size) as u32).min(255);
            let y = ((uv[1] * size) as u32).min(255);
            maps.albedo.texel(x, y).w
        };
        for cell in 0..atlas.leaf_cells {
            assert!(
                alpha(atlas.map(cell, LeafAtlas::STEM_UV)) > 0.9,
                "{extra}: stem"
            );
        }
        let [u0, v0, u1, v1] = atlas.cell_rect(0);
        let pad = 0.5 * LeafAtlas::PADDING * (u1 - u0).min(v1 - v0);
        assert_eq!(
            alpha([u0 - pad, f32::midpoint(v0, v1)]),
            0.0,
            "{extra}: padding"
        );
        let covered = (0..256)
            .flat_map(|y| (0..256).map(move |x| (x, y)))
            .filter(|&(x, y)| maps.albedo.texel(x, y).w > 0.5)
            .count();
        assert!(covered > 256 * 256 / 20, "{extra}: leaves cover the atlas");
    }
}

#[test]
fn textures_do_not_depend_on_the_thread_count_or_the_run() {
    let species = Species::from_toml_str(
        "[foliage]\nvenation = { kind = \"open\" }\nmargin = { kind = \"serrate\" }\n\
         texture_resolution = 128\n[bark]\ntexture_resolution = 128\n",
    )
    .unwrap();
    let options = TextureOptions::default();
    let on = |threads: usize| {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        pool.install(|| synthesize(&species, &options))
    };
    let (one, four) = (on(1), on(4));
    let images = |t: &arbrume_texture::SpeciesTextures| (t.bark.clone(), t.leaf.clone());
    assert_eq!(images(&one), images(&four));
    assert_eq!(images(&one), images(&synthesize(&species, &options)));
    let other = synthesize(&species, &TextureOptions { seed: 1, ..options });
    assert_ne!(
        images(&one).0,
        images(&other).0,
        "the seed changes the texture"
    );
}

#[test]
fn the_cache_synthesizes_each_species_once_and_apply_sets_the_materials() {
    let cache = Arc::new(TextureCache::new(TextureOptions::default()));
    let species = species(BarkKind::Fibrous);
    let first = cache.get(&species);
    let threads: Vec<_> = (0..4)
        .map(|_| {
            let (cache, species) = (Arc::clone(&cache), species.clone());
            std::thread::spawn(move || cache.get(&species))
        })
        .collect();
    for thread in threads {
        assert!(Arc::ptr_eq(&first, &thread.join().unwrap()));
    }
    assert_eq!(cache.len(), 1);
    let part = |kind| MeshPart {
        kind,
        material: Material {
            name: String::new(),
            base_color: [0.2, 0.3, 0.1, 1.0],
            roughness: 0.6,
            double_sided: false,
            alpha_cutoff: None,
            base_color_texture: None,
            normal_texture: None,
            roughness_texture: None,
        },
        buffers: MeshBuffers::default(),
    };
    let mut mesh = TreeMesh {
        parts: vec![
            part(PartKind::Bark),
            part(PartKind::Leaf),
            part(PartKind::Billboard),
        ],
        wind: None,
    };
    first.apply(&mut mesh, &species);
    let [bark, leaf, billboard] = [0, 1, 2].map(|i| &mesh.parts[i].material);
    let bark_maps: &MaterialTextures = first.bark.as_ref().unwrap();
    let bark_mean = bark_maps.mean;
    for channel in 0..3 {
        let factor = [0.2, 0.3, 0.1][channel] / bark_mean[channel];
        assert!((bark.base_color[channel] - factor.min(1.0)).abs() < 1e-6);
    }
    assert_eq!(
        bark.roughness_texture.as_ref(),
        bark_maps.roughness.as_ref()
    );
    assert_eq!(leaf.alpha_cutoff, Some(0.5));
    assert!(leaf.normal_texture.is_some() && billboard.base_color_texture.is_none());
}

#[test]
fn leaf_coordinates_move_to_the_atlas_of_the_textures() {
    let base = Species::from_toml_str(
        "[foliage]
width_ratio = 0.5
texture_resolution = 64",
    )
    .unwrap();
    let mut wide = base.clone();
    wide.foliage.width_ratio = 2.0;
    let (from, to) = (wide.foliage.atlas(), base.foliage.atlas());
    assert_ne!(from, to, "the test needs atlases that differ");
    let textures = synthesize(&base, &TextureOptions::default());
    let mut uvs: Vec<[f32; 2]> = (0..from.leaf_cells)
        .flat_map(|cell| [from.map(cell, [0.25, 0.5]), from.map(cell, [1.0, 1.0])])
        .collect();
    textures.remap(&wide, &mut uvs);
    for (index, uv) in uvs.iter().enumerate() {
        let cell = index as u32 / 2;
        let local = if index % 2 == 0 {
            [0.25, 0.5]
        } else {
            [1.0, 1.0]
        };
        let expected = to.map(cell % to.leaf_cells, local);
        assert!((uv[0] - expected[0]).abs() < 1e-5 && (uv[1] - expected[1]).abs() < 1e-5);
    }
    let mut same = vec![to.map(1, [0.5, 0.5])];
    textures.remap(&base, &mut same);
    assert_eq!(same[0], to.map(1, [0.5, 0.5]));
}

#[test]
fn textured_species_turns_outline_leaves_into_cards_but_not_needles() {
    let broad = Species::default();
    assert!(textured_species(&broad).foliage.uses_cards());
    let needle = Species::from_toml_str("[foliage]\nshape = \"needle\"").unwrap();
    assert!(matches!(
        textured_species(&needle),
        std::borrow::Cow::Borrowed(_)
    ));
}

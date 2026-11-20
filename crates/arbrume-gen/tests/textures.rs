//! Plants with procedural textures: materials, determinism, billboards and files.
//!
//! When `ARBRUME_GLB_OUT` names a directory, the files written here are also saved there
//! (embedded textures as `*_embed.glb`, external ones as `*_files.glb` next to their
//! PNG files) so `tools/validate_glb.mjs` can run the Khronos validator on them.

use arbrume_core::mesh::{PartKind, TreeMesh};
use arbrume_core::species::LeafGeometry;
use arbrume_export::{
    ExportOptions, GlbLevel, GlbOptions, ImageStorage, image_file_name, write_glb_images,
    write_glb_levels,
};
use arbrume_gen::{
    BillboardMethod, BillboardOptions, GenParams, Generator, LodKind, LodSettings, Plant,
    TextureCache, TextureOptions, preset, run_batch,
};
use arbrume_mesh::LodOptions;
use std::sync::Arc;

/// Small textures keep the tests fast in debug builds.
fn cache() -> Arc<TextureCache> {
    Arc::new(TextureCache::new(TextureOptions {
        seed: 0,
        max_resolution: 64,
    }))
}

fn textured(name: &str, cache: &Arc<TextureCache>) -> Plant {
    let mut generator = Generator::new();
    generator.set_textures(Some(Arc::clone(cache)));
    let params = GenParams::new(2, 6).with_quality(0.5);
    generator.generate(&preset(name).unwrap(), &params).unwrap()
}

fn part(mesh: &TreeMesh, kind: PartKind) -> &arbrume_core::mesh::MeshPart {
    mesh.parts.iter().find(|part| part.kind == kind).unwrap()
}

#[test]
fn textured_plants_carry_textures_and_cut_cards() {
    let cache = cache();
    for name in ["broadleaf", "pine_like"] {
        let plant = textured(name, &cache);
        let bark = &part(&plant.mesh, PartKind::Bark).material;
        let leaf = &part(&plant.mesh, PartKind::Leaf).material;
        assert!(bark.base_color_texture.is_some() && bark.roughness_texture.is_some());
        assert!(leaf.base_color_texture.is_some() && leaf.normal_texture.is_some());
        assert_eq!(leaf.alpha_cutoff, Some(0.5));
        assert!(bark.base_color.iter().all(|c| (0.0..=1.0).contains(c)));
        let uvs = &part(&plant.mesh, PartKind::Leaf).buffers.uvs;
        assert!(
            uvs.iter().flatten().all(|c| (0.0..=1.0).contains(c)),
            "{name}"
        );
    }
    let plain = Generator::new()
        .generate(
            &preset("broadleaf").unwrap(),
            &GenParams::new(2, 6).with_quality(0.5),
        )
        .unwrap();
    let card = textured("broadleaf", &cache);
    assert_eq!(
        part(&plain.mesh, PartKind::Leaf).buffers.indices.len(),
        part(&card.mesh, PartKind::Leaf).buffers.indices.len(),
        "a folded card has the triangles of the outline blade"
    );
    assert_eq!(
        plain.mesh.parts[0].buffers, card.mesh.parts[0].buffers,
        "same bark"
    );
    assert_eq!(
        preset("broadleaf").unwrap().foliage.geometry,
        LeafGeometry::Outline
    );
}

#[test]
fn varied_individuals_share_the_textures_of_their_species() {
    let cache = cache();
    let species = Arc::new(preset("birch_like").unwrap());
    let mut generator = Generator::new();
    generator.set_textures(Some(Arc::clone(&cache)));
    generator.set_texture_base(Some(Arc::clone(&species)));
    let params = GenParams::new(1, 5).with_quality(0.4);
    let meshes: Vec<TreeMesh> = (0..3)
        .map(|index| {
            let individual = species.varied(7, index);
            generator.generate(&individual, &params).unwrap().mesh
        })
        .collect();
    assert_eq!(cache.len(), 1, "one texture set for every individual");
    let leaf = |mesh: &TreeMesh| part(mesh, PartKind::Leaf).material.clone();
    assert_eq!(
        leaf(&meshes[0]).base_color_texture,
        leaf(&meshes[1]).base_color_texture
    );
    assert_ne!(
        leaf(&meshes[0]).base_color,
        leaf(&meshes[1]).base_color,
        "own colours"
    );
    let atlas = cache.get(&species).atlas.unwrap();
    let rects: Vec<[f32; 4]> = (0..atlas.leaf_cells).map(|c| atlas.cell_rect(c)).collect();
    for mesh in &meshes {
        let uvs = &part(mesh, PartKind::Leaf).buffers.uvs;
        let inside = |uv: &[f32; 2]| {
            rects.iter().any(|r| {
                (r[0] - 1e-4..=r[2] + 1e-4).contains(&uv[0])
                    && (r[1] - 1e-4..=r[3] + 1e-4).contains(&uv[1])
            })
        };
        assert!(
            uvs.iter().all(inside),
            "leaves sample the cells of the shared atlas"
        );
    }
}

#[test]
fn textured_batches_do_not_depend_on_the_thread_count() {
    let names = ["broadleaf", "birch_like", "pine_like", "broadleaf"];
    let run = |threads: usize| {
        let cache = cache();
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        pool.install(|| {
            run_batch(names.len(), |generator, _, index| {
                generator.set_textures(Some(Arc::clone(&cache)));
                let params = GenParams::new(index as u64, 5).with_quality(0.4);
                generator
                    .generate(&preset(names[index]).unwrap(), &params)
                    .unwrap()
                    .mesh
            })
        })
    };
    let one = run(1);
    assert_eq!(one, run(3));
    assert_eq!(one[0].parts[1].material, one[3].parts[1].material);
}

#[test]
fn billboards_are_baked_from_the_textured_leaves() {
    let cache = cache();
    let species = preset("broadleaf").unwrap();
    let settings = LodSettings {
        lod: LodOptions {
            levels: 2,
            ..LodOptions::default()
        },
        billboard: Some(BillboardOptions {
            method: BillboardMethod::Cross,
            atlas_size: 128,
            supersampling: 1,
            ..BillboardOptions::default()
        }),
        billboard_source: 1,
        ..LodSettings::default()
    };
    let bake = |textures: Option<Arc<TextureCache>>| {
        let mut generator = Generator::new();
        generator.set_textures(textures);
        let plant = generator
            .generate(&species, &GenParams::new(1, 6).with_quality(0.5))
            .unwrap();
        generator.lod_chain(&plant, &species, &settings).unwrap()
    };
    let chain = bake(Some(Arc::clone(&cache)));
    assert!(
        chain
            .iter()
            .filter(|l| l.kind == LodKind::Mesh)
            .all(|level| {
                level
                    .mesh
                    .parts
                    .iter()
                    .all(|p| p.material.base_color_texture.is_some())
            })
    );
    let last = chain.last().unwrap();
    assert_eq!(last.kind, LodKind::Billboard);
    let plain = bake(None);
    let atlas = |levels: &[arbrume_gen::LodMesh]| {
        levels.last().unwrap().mesh.parts[0]
            .material
            .base_color_texture
            .clone()
    };
    assert_ne!(atlas(&chain), atlas(&plain), "the bake sees the textures");
}

/// Writes `mesh` with `images` and returns the bytes.
fn glb(mesh: &TreeMesh, images: ImageStorage) -> Vec<u8> {
    let mut bytes = Vec::new();
    write_glb_images(mesh, &GlbOptions::default(), &[], images, &mut bytes).unwrap();
    bytes
}

#[test]
fn files_reference_external_images_and_embed_holds_them() {
    let cache = cache();
    let plant = textured("birch_like", &cache);
    let embedded = glb(&plant.mesh, ImageStorage::Embed);
    let external = glb(&plant.mesh, ImageStorage::External);
    let images: Vec<&Vec<u8>> = plant
        .mesh
        .parts
        .iter()
        .flat_map(|p| {
            let m = &p.material;
            [
                &m.base_color_texture,
                &m.normal_texture,
                &m.roughness_texture,
            ]
        })
        .flatten()
        .collect();
    assert_eq!(images.len(), 5);
    let total: usize = images.iter().map(|i| i.len()).sum();
    assert!(embedded.len() >= external.len() + total);
    let gltf = gltf::Gltf::from_slice(&external).unwrap();
    let uris: Vec<String> = gltf
        .images()
        .map(|image| match image.source() {
            gltf::image::Source::Uri { uri, .. } => uri.to_owned(),
            gltf::image::Source::View { .. } => panic!("embedded image"),
        })
        .collect();
    assert_eq!(uris.len(), 5);
    assert!(images.iter().all(|i| uris.contains(&image_file_name(i))));
    let chain_level = GlbLevel {
        mesh: &plant.mesh,
        extras: None,
        extra: &[],
        leaves: None,
    };
    let export = ExportOptions {
        images: ImageStorage::External,
        ..ExportOptions::default()
    };
    let mut levels = Vec::new();
    write_glb_levels(&[chain_level], &GlbOptions::default(), &export, &mut levels).unwrap();
    assert_eq!(gltf::Gltf::from_slice(&levels).unwrap().images().count(), 5);
    if let Some(dir) = std::env::var_os("ARBRUME_GLB_OUT") {
        let dir = std::path::Path::new(&dir);
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("textured_embed.glb"), &embedded).unwrap();
        std::fs::write(dir.join("textured_files.glb"), &external).unwrap();
        for image in images {
            std::fs::write(dir.join(image_file_name(image)), image).unwrap();
        }
    }
}

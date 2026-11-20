//! End-to-end checks of framing, shading, shadows, determinism and thumbnails.

use arbrume_core::mesh::{Material, MeshBuffers, MeshPart, PartKind, TreeMesh};
use arbrume_core::species::Species;
use arbrume_grow::{GrowParams, grow};
use arbrume_mesh::{MeshOptions, mesh_tree};
use arbrume_raster::camera::direction;
use arbrume_raster::{
    Background, Bounds, Camera, Ground, Lighting, Renderer, Scene, ThumbnailOptions,
    render_thumbnail,
};
use glam::{Vec2, Vec3};
use std::path::PathBuf;

fn material(color: [f32; 4], double_sided: bool) -> Material {
    Material {
        name: "test".to_owned(),
        base_color: color,
        roughness: 1.0,
        double_sided,
        alpha_cutoff: None,
        base_color_texture: None,
        normal_texture: None,
        roughness_texture: None,
    }
}

/// A horizontal square of side `size` centred on `center`, facing up.
fn quad(center: Vec3, size: f32, color: [f32; 4], kind: PartKind) -> MeshPart {
    let h = 0.5 * size;
    let corners = [(-h, h), (h, h), (h, -h), (-h, -h)];
    MeshPart {
        kind,
        material: material(color, true),
        buffers: MeshBuffers {
            positions: corners
                .iter()
                .map(|&(x, z)| (center + Vec3::new(x, 0.0, z)).to_array())
                .collect(),
            normals: vec![[0.0, 1.0, 0.0]; 4],
            uvs: Vec::new(),
            colors: Vec::new(),
            wind: Vec::new(),
            indices: vec![0, 1, 2, 0, 2, 3],
        },
    }
}

fn plain_lighting(sun_direction: Vec3) -> Lighting {
    Lighting {
        sun_direction,
        shadow_map_size: 512,
        shadow_softness: 1.0,
        sky_occlusion: 0.0,
        ..Lighting::default()
    }
}

fn starter_mesh(name: &str, age: u16) -> TreeMesh {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../arbrume-grow/tests/data")
        .join(format!("{name}.toml"));
    let species = Species::from_path(&path).unwrap();
    let graph = grow(
        &species,
        &GrowParams {
            seed: 2,
            age,
            scale: 1.0,
        },
    );
    mesh_tree(&graph, &species, &MeshOptions::default()).unwrap()
}

#[test]
fn framing_keeps_every_corner_inside_the_image() {
    let bounds = Bounds {
        min: Vec3::new(-1.5, 0.0, -0.7),
        max: Vec3::new(2.0, 9.0, 1.1),
    };
    for fov in [0.0, 10.0, 35.0, 90.0] {
        for (azimuth, elevation) in [(0.0, 0.0), (37.0, 20.0), (-120.0, 60.0), (10.0, 90.0)] {
            let camera = Camera::frame(&bounds, azimuth, elevation, fov);
            for aspect in [0.5, 1.0, 2.0] {
                let clip = camera.clip_from_world(aspect);
                for corner in bounds.corners() {
                    let ndc = clip.project_point3(corner);
                    let label = format!("fov {fov} view {azimuth}/{elevation} aspect {aspect}");
                    assert!(ndc.x.abs() <= 1.0 && ndc.y.abs() <= 1.0, "{label}: {ndc}");
                    assert!((0.0..=1.0).contains(&ndc.z), "{label}: depth {}", ndc.z);
                }
            }
        }
    }
}

#[test]
fn framing_fills_the_image_up_to_the_margin() {
    let bounds = Bounds {
        min: Vec3::new(-1.0, 0.0, -1.0),
        max: Vec3::new(1.0, 6.0, 1.0),
    };
    let camera = Camera::frame(&bounds, 0.0, 0.0, 0.0);
    let clip = camera.clip_from_world(1.0);
    let top = clip.project_point3(Vec3::new(0.0, 6.0, 0.0)).y;
    let bottom = clip.project_point3(Vec3::ZERO).y;
    let fill = 1.0 - arbrume_raster::camera::DEFAULT_MARGIN;
    assert!((top - fill).abs() < 1e-4 && (bottom + fill).abs() < 1e-4);
}

/// A scene looking straight down at the origin, orthographic, over a large ground.
fn top_down_scene(sun_direction: Vec3) -> Scene {
    let bounds = Bounds {
        min: Vec3::new(-4.0, 0.0, -4.0),
        max: Vec3::new(4.0, 2.0, 4.0),
    };
    Scene {
        camera: Camera::frame(&bounds, 0.0, 90.0, 0.0),
        width: 64,
        height: 64,
        supersampling: 2,
        lighting: plain_lighting(sun_direction),
        background: Background::Solid([0.0, 0.0, 1.0]),
        ground: Some(Ground {
            height: 0.0,
            center: Vec2::ZERO,
            radius: 20.0,
            color: [0.5, 0.5, 0.5],
        }),
    }
}

fn pixel_of(scene: &Scene, point: Vec3) -> (u32, u32) {
    let ndc = scene.camera.clip_from_world(1.0).project_point3(point);
    let x = ((ndc.x * 0.5 + 0.5) * scene.width as f32) as u32;
    let y = ((0.5 - ndc.y * 0.5) * scene.height as f32) as u32;
    (x, y)
}

fn luminance(target: &arbrume_raster::RenderTarget, (x, y): (u32, u32)) -> f32 {
    let c = target.color[(y * target.width + x) as usize];
    c[0] + c[1] + c[2]
}

#[test]
fn shadow_of_a_floating_quad_lands_where_the_sun_projects_it() {
    let height = 2.0;
    let mesh = TreeMesh {
        parts: vec![quad(
            Vec3::new(0.0, height, 0.0),
            1.0,
            [0.8, 0.1, 0.1, 1.0],
            PartKind::Leaf,
        )],
        wind: None,
    };
    let sun = Vec3::new(1.0, 1.0, 0.0).normalize();
    let scene = top_down_scene(sun);
    let target = Renderer::new().render(&mesh, &scene);
    let shadow = luminance(&target, pixel_of(&scene, Vec3::new(-height, 0.0, 0.0)));
    let lit = luminance(&target, pixel_of(&scene, Vec3::new(height, 0.0, 0.0)));
    let beside = luminance(&target, pixel_of(&scene, Vec3::new(-height, 0.0, 1.5)));
    assert!(shadow < 0.6 * lit, "shadow {shadow} lit {lit}");
    assert!(
        (beside - lit).abs() < 0.05 * lit,
        "beside {beside} lit {lit}"
    );
    let quad_pixel = pixel_of(&scene, Vec3::new(0.0, height, 0.0));
    let i = (quad_pixel.1 * target.width + quad_pixel.0) as usize;
    assert!((target.coverage[i] - 1.0).abs() < 1e-6);
    assert!(target.color[i][0] > 2.0 * target.color[i][1]);
}

#[test]
fn nearer_part_hides_farther_part() {
    let mesh = TreeMesh {
        parts: vec![
            quad(
                Vec3::new(0.0, 1.0, 0.0),
                2.0,
                [0.9, 0.0, 0.0, 1.0],
                PartKind::Bark,
            ),
            quad(
                Vec3::new(0.0, 2.0, 0.0),
                1.0,
                [0.0, 0.0, 0.9, 1.0],
                PartKind::Leaf,
            ),
        ],
        wind: None,
    };
    let mut scene = top_down_scene(Vec3::Y);
    scene.ground = None;
    let target = Renderer::new().render(&mesh, &scene);
    let centre = pixel_of(&scene, Vec3::new(0.0, 2.0, 0.0));
    let rim = pixel_of(&scene, Vec3::new(0.8, 1.0, 0.0));
    let at = |(x, y): (u32, u32)| target.color[(y * target.width + x) as usize];
    assert!(at(centre)[2] > at(centre)[0], "{:?}", at(centre));
    assert!(at(rim)[0] > at(rim)[2], "{:?}", at(rim));
}

#[test]
fn single_sided_quad_seen_from_below_is_culled() {
    let mut part = quad(Vec3::ZERO, 2.0, [0.5; 4], PartKind::Bark);
    part.material.double_sided = false;
    let mesh = TreeMesh {
        parts: vec![part],
        wind: None,
    };
    let bounds = Bounds::of_mesh(&mesh).unwrap();
    let mut scene = top_down_scene(Vec3::Y);
    scene.ground = None;
    scene.camera = Camera::frame(&bounds, 0.0, 60.0, 30.0);
    let mut renderer = Renderer::new();
    let above = renderer.render(&mesh, &scene);
    assert!(above.coverage.iter().any(|&c| c > 0.0));
    scene.camera = Camera::frame(&bounds, 0.0, -60.0, 30.0);
    let below = renderer.render(&mesh, &scene);
    assert!(below.coverage.iter().all(|&c| c == 0.0));
}

#[test]
fn camera_inside_the_geometry_does_not_panic() {
    let mesh = starter_mesh("shrub", 6);
    let bounds = Bounds::of_mesh(&mesh).unwrap();
    let mut camera = Camera::frame(&bounds, 30.0, 10.0, 60.0);
    camera.eye = bounds.center();
    camera.target = bounds.center() + direction(200.0, -10.0);
    camera.near = 1e-3;
    let scene = Scene {
        camera,
        width: 48,
        height: 32,
        supersampling: 2,
        lighting: Lighting::default(),
        background: Background::Transparent,
        ground: Some(Ground::under(&bounds, 2.0, [0.3; 3])),
    };
    let target = Renderer::new().render(&mesh, &scene);
    assert!(target.coverage.iter().any(|&c| c > 0.0));
}

#[test]
fn thumbnails_are_deterministic_and_show_the_plant() {
    let mesh = starter_mesh("broadleaf", 10);
    let options = ThumbnailOptions {
        width: 96,
        height: 128,
        ..ThumbnailOptions::default()
    };
    let first = render_thumbnail(&mesh, &options);
    let mut renderer = Renderer::new();
    renderer.thumbnail(&starter_mesh("conifer", 8), &options);
    let again = renderer.thumbnail(&mesh, &options);
    assert_eq!(first, again);
    assert_eq!(first.to_png().unwrap(), again.to_png().unwrap());
    assert!(first.is_opaque());
    let target = renderer.thumbnail_target(&mesh, &options);
    let covered = target.coverage.iter().filter(|&&c| c > 0.0).count();
    assert!(covered > 200, "{covered} covered pixels");
}

#[test]
fn transparent_background_leaves_the_corners_clear() {
    let mesh = starter_mesh("shrub", 8);
    let options = ThumbnailOptions {
        width: 64,
        height: 64,
        background: Background::Transparent,
        ..ThumbnailOptions::default()
    };
    let image = render_thumbnail(&mesh, &options);
    assert_eq!(image.pixel(0, 0)[3], 0);
    assert_eq!(image.pixel(63, 0)[3], 0);
    assert!(image.pixels.as_chunks::<4>().0.iter().any(|p| p[3] == 255));
}

#[test]
fn empty_mesh_gives_the_background() {
    let options = ThumbnailOptions {
        width: 8,
        height: 8,
        background: Background::Solid([1.0, 0.0, 0.0]),
        ..ThumbnailOptions::default()
    };
    let image = render_thumbnail(&TreeMesh::default(), &options);
    assert!(
        image
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| *p == [255, 0, 0, 255])
    );
}

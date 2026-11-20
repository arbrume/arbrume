//! The texture atlas of a billboard and its quads.
//!
//! Each plane gets a rectangle of the atlas covering the projection of the faces it
//! draws, at one texel density for the whole atlas: the largest density at which the
//! rectangles, sorted by height and packed in shelves, fit in the square atlas. Each
//! rectangle is rendered with an orthographic camera looking at the plane along its
//! normal, drawing only the faces of the plane but lighting them with the sky
//! occlusion of the whole plant and no sun, so the colour is the albedo times the
//! ambient occlusion. Two PNG images come out:
//!
//! * base colour: that colour in sRGB, and the coverage of the texel in alpha;
//! * normal: the normal of the surface in the frame of the quad (`x` along the image
//!   right, `y` along the image up, `z` along the plane normal, each mapped from
//!   `[-1, 1]` to `[0, 255]`, glTF convention), and in alpha the height of the surface
//!   above the plane, `128 + 127 h / depth_range`, clamped.
//!
//! Covered texels are spread into their empty neighbours for a few texels (colour and
//! normal, not alpha), so that filtering and mipmaps do not bleed the empty colour into
//! the edges.

use crate::{BillboardOptions, Bounds3, Plane};
use arbrume_core::mesh::{Material, MeshBuffers, TreeMesh};
use arbrume_raster::camera::{Camera, Projection};
use arbrume_raster::target::linear_to_srgb;
use arbrume_raster::{Background, Lighting, RenderTarget, Renderer, Scene};
use glam::Vec3;

/// Texels of padding around each rectangle.
const PAD: u32 = 2;

/// Passes of the spreading of covered texels into empty ones.
const DILATE: usize = 4;

/// Fraction of the atlas area the first packing attempt fills.
const FILL: f32 = 0.8;

/// Factor on the density after a packing attempt that does not fit.
const SHRINK: f32 = 0.93;

/// What [`bake`] returns.
#[derive(Debug, Clone, PartialEq)]
pub struct Baked {
    /// The material, with both textures.
    pub material: Material,
    /// The quads.
    pub buffers: MeshBuffers,
    /// Texels per meter.
    pub texels_per_meter: f32,
    /// Depth range of the normal alpha.
    pub depth_range: f32,
}

/// Extent of a plane in its own coordinates: `[right_min, right_max, up_min, up_max]`.
fn extent(mesh: &TreeMesh, plane: &Plane) -> Option<[f32; 4]> {
    let mut out: Option<[f32; 4]> = None;
    for (index, part) in mesh.parts.iter().enumerate() {
        let mask = plane.masks.get(index).filter(|mask| !mask.is_empty());
        let positions = &part.buffers.positions;
        for (t, tri) in part.buffers.indices.as_chunks::<3>().0.iter().enumerate() {
            if mask.is_some_and(|mask| !mask[t]) {
                continue;
            }
            for &i in tri {
                let p = Vec3::from(positions[i as usize]) - plane.origin;
                let (u, v) = (p.dot(plane.right), p.dot(plane.up));
                out = Some(out.map_or([u, u, v, v], |e| {
                    [e[0].min(u), e[1].max(u), e[2].min(v), e[3].max(v)]
                }));
            }
        }
    }
    out
}

/// A rectangle of the atlas, in texels, padding included.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Tile {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

/// Shelf packing of rectangles of `sizes` texels into a square of side `side`; `None`
/// when they do not fit.
fn pack(sizes: &[(u32, u32)], side: u32) -> Option<Vec<Tile>> {
    let mut order: Vec<usize> = (0..sizes.len()).collect();
    order.sort_by_key(|&i| (std::cmp::Reverse(sizes[i].1), i));
    let mut tiles = vec![
        Tile {
            x: 0,
            y: 0,
            width: 0,
            height: 0
        };
        sizes.len()
    ];
    let (mut x, mut y, mut shelf) = (0, 0, 0);
    for i in order {
        let (width, height) = sizes[i];
        if width > side {
            return None;
        }
        if x + width > side {
            (x, y, shelf) = (0, y + shelf, 0);
        }
        if y + height > side {
            return None;
        }
        tiles[i] = Tile {
            x,
            y,
            width,
            height,
        };
        x += width;
        shelf = shelf.max(height);
    }
    Some(tiles)
}

/// The texel density and tiles of `extents` in an atlas of side `side`.
fn layout(extents: &[[f32; 4]], side: u32) -> (f32, Vec<Tile>) {
    let area: f32 = extents
        .iter()
        .map(|e| ((e[1] - e[0]) * (e[3] - e[2])).max(1e-6))
        .sum();
    let mut density = (FILL * (side * side) as f32 / area).sqrt();
    loop {
        let sizes: Vec<(u32, u32)> = extents
            .iter()
            .map(|e| {
                let size = |span: f32| (span * density).ceil().max(1.0) as u32 + 2 * PAD;
                (size(e[1] - e[0]), size(e[3] - e[2]))
            })
            .collect();
        if let Some(tiles) = pack(&sizes, side) {
            return (density, tiles);
        }
        density *= SHRINK;
    }
}

/// Lighting that renders the albedo times the sky occlusion: no sun, white sky and
/// bounce.
fn bake_lighting(options: &BillboardOptions) -> Lighting {
    Lighting {
        sun_color: [0.0; 3],
        sky_color: [1.0; 3],
        bounce_color: [1.0; 3],
        shadows: false,
        occlusion_cells: if options.bake_occlusion { 32 } else { 0 },
        sky_occlusion: if options.bake_occlusion { 1.0 } else { 0.0 },
        leaf_translucency: 0.0,
        exposure: 1.0,
        ..Lighting::default()
    }
}

/// The scene that renders `tile` of `plane`, whose extent is `e`, at `density`.
fn tile_scene(
    plane: &Plane,
    e: [f32; 4],
    tile: Tile,
    (density, radius): (f32, f32),
    options: &BillboardOptions,
) -> (Scene, f32) {
    let (width_m, height_m) = (tile.width as f32 / density, tile.height as f32 / density);
    let center_u = f32::midpoint(e[0], e[1]);
    let center_v = f32::midpoint(e[2], e[3]);
    let target = plane.origin + plane.right * center_u + plane.up * center_v;
    let distance = 3.0 * radius;
    let camera = Camera {
        eye: target + plane.normal * distance,
        target,
        up: plane.up,
        projection: Projection::Orthographic {
            half_size: 0.5 * width_m.min(height_m),
        },
        near: distance - 2.0 * radius,
        far: distance + 2.0 * radius,
    };
    let scene = Scene {
        camera,
        width: tile.width,
        height: tile.height,
        supersampling: options.supersampling.max(1),
        lighting: bake_lighting(options),
        background: Background::Transparent,
        ground: None,
    };
    (scene, distance)
}

/// The atlas images being filled, linear, `side * side` texels each.
struct Images {
    side: u32,
    color: Vec<[f32; 4]>,
    normal: Vec<[f32; 4]>,
}

impl Images {
    /// Copies the render of `plane` into `tile`.
    fn store(
        &mut self,
        target: &RenderTarget,
        plane: &Plane,
        tile: Tile,
        (scene, distance, range): (&Scene, f32, f32),
    ) {
        let depth_span = scene.camera.far - scene.camera.near;
        for y in 0..tile.height {
            for x in 0..tile.width {
                let source = (y * target.width + x) as usize;
                let alpha = target.color[source][3];
                if alpha <= 0.0 {
                    continue;
                }
                let at = ((tile.y + y) * self.side + tile.x + x) as usize;
                let c = target.color[source];
                self.color[at] = [c[0] / alpha, c[1] / alpha, c[2] / alpha, alpha];
                let n = Vec3::from(target.normal[source]);
                let local = Vec3::new(n.dot(plane.right), n.dot(plane.up), n.dot(plane.normal));
                let height = distance - (scene.camera.near + target.depth[source] * depth_span);
                let depth = 0.5 + 0.5 * (height / range).clamp(-1.0, 1.0);
                self.normal[at] = [
                    local.x * 0.5 + 0.5,
                    local.y * 0.5 + 0.5,
                    local.z * 0.5 + 0.5,
                    depth,
                ];
            }
        }
    }

    /// Spreads colour and normal of covered texels into empty neighbours.
    fn dilate(&mut self) {
        let side = self.side as usize;
        let mut filled: Vec<bool> = self.color.iter().map(|c| c[3] > 0.0).collect();
        for _ in 0..DILATE {
            let before = filled.clone();
            for at in 0..side * side {
                if before[at] {
                    continue;
                }
                if let Some(from) = covered_neighbour(&before, at, side) {
                    let c = self.color[from];
                    self.color[at] = [c[0], c[1], c[2], 0.0];
                    self.normal[at] = self.normal[from];
                    filled[at] = true;
                }
            }
        }
    }
}

/// A neighbour of texel `at` that is `filled`, in a fixed order.
fn covered_neighbour(filled: &[bool], at: usize, side: usize) -> Option<usize> {
    let (x, y) = (at % side, at / side);
    let neighbours = [
        (x.wrapping_sub(1), y),
        (x + 1, y),
        (x, y.wrapping_sub(1)),
        (x, y + 1),
    ];
    neighbours
        .into_iter()
        .filter(|&(nx, ny)| nx < side && ny < side)
        .map(|(nx, ny)| ny * side + nx)
        .find(|&n| filled[n])
}

/// Encodes `texels` (linear, in `[0, 1]`) as an RGBA PNG, colour channels in sRGB when
/// `srgb`.
fn encode(texels: &[[f32; 4]], side: u32, srgb: bool) -> Vec<u8> {
    let code = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    let bytes: Vec<u8> = texels
        .iter()
        .flat_map(|t| {
            let colour = |v: f32| {
                if srgb {
                    code(linear_to_srgb(v.clamp(0.0, 1.0)))
                } else {
                    code(v)
                }
            };
            [colour(t[0]), colour(t[1]), colour(t[2]), code(t[3])]
        })
        .collect();
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, side, side);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    if srgb {
        encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
    }
    let written = encoder
        .write_header()
        .and_then(|mut writer| writer.write_image_data(&bytes));
    written.expect("encoding a PNG into memory cannot fail");
    out
}

/// The quad of `plane` over `tile` (padding excluded), appended to `buffers`.
fn quad(
    buffers: &mut MeshBuffers,
    plane: &Plane,
    e: [f32; 4],
    tile: Tile,
    (density, side): (f32, u32),
) {
    let (inner_w, inner_h) = (
        (tile.width - 2 * PAD) as f32,
        (tile.height - 2 * PAD) as f32,
    );
    let center = plane.origin
        + plane.right * (f32::midpoint(e[0], e[1]))
        + plane.up * (f32::midpoint(e[2], e[3]));
    let (half_w, half_h) = (0.5 * inner_w / density, 0.5 * inner_h / density);
    let s = side as f32;
    let (u0, u1) = (
        (tile.x + PAD) as f32 / s,
        (tile.x + PAD) as f32 / s + inner_w / s,
    );
    let (v0, v1) = (
        (tile.y + PAD) as f32 / s,
        (tile.y + PAD) as f32 / s + inner_h / s,
    );
    let base = buffers.positions.len() as u32;
    for (sx, sy, u, v) in [
        (-1.0, -1.0, u0, v1),
        (1.0, -1.0, u1, v1),
        (1.0, 1.0, u1, v0),
        (-1.0, 1.0, u0, v0),
    ] {
        let p = center + plane.right * (sx * half_w) + plane.up * (sy * half_h);
        buffers.positions.push(p.to_array());
        buffers.normals.push(plane.normal.to_array());
        buffers.uvs.push([u, v]);
    }
    buffers
        .indices
        .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
}

/// Bakes the atlas of `planes` and builds their quads; `None` when no plane draws
/// anything.
#[must_use]
pub fn bake(
    mesh: &TreeMesh,
    planes: &[Plane],
    bounds: &Bounds3,
    options: &BillboardOptions,
) -> Option<Baked> {
    let drawn: Vec<(&Plane, [f32; 4])> = planes
        .iter()
        .filter_map(|p| extent(mesh, p).map(|e| (p, e)))
        .collect();
    if drawn.is_empty() {
        return None;
    }
    let side = options.atlas_size.max(16);
    let extents: Vec<[f32; 4]> = drawn.iter().map(|&(_, e)| e).collect();
    let (density, tiles) = layout(&extents, side);
    let depth_range = drawn
        .iter()
        .map(|(p, _)| p.depth_range)
        .fold(1e-4, f32::max);
    let count = (side * side) as usize;
    let mut images = Images {
        side,
        color: vec![[0.0; 4]; count],
        normal: vec![[0.0; 4]; count],
    };
    let mut renderer = Renderer::new();
    let mut buffers = MeshBuffers::default();
    for (&(plane, e), &tile) in drawn.iter().zip(&tiles) {
        let pad = PAD as f32 / density;
        let padded = [e[0] - pad, e[1] + pad, e[2] - pad, e[3] + pad];
        let (scene, distance) =
            tile_scene(plane, padded, tile, (density, bounds.radius()), options);
        let target = renderer.render_selected(mesh, &scene, &plane.masks);
        images.store(&target, plane, tile, (&scene, distance, depth_range));
        quad(&mut buffers, plane, e, tile, (density, side));
    }
    images.dilate();
    add_wind(mesh, &mut buffers);
    let material = Material {
        name: "billboard".to_owned(),
        base_color: [1.0; 4],
        roughness: 0.9,
        double_sided: true,
        alpha_cutoff: Some(options.alpha_cutoff),
        base_color_texture: Some(encode(&images.color, side, true)),
        normal_texture: Some(encode(&images.normal, side, false)),
        roughness_texture: None,
    };
    Some(Baked {
        material,
        buffers,
        texels_per_meter: density,
        depth_range,
    })
}

/// Gives the quads the wind data of the first root branch of `mesh`, when it has wind
/// data: `x0` by height along it, nothing above level 0.
fn add_wind(mesh: &TreeMesh, buffers: &mut MeshBuffers) {
    let Some(root) = mesh.wind.as_ref().and_then(|table| table.branches.first()) else {
        return;
    };
    let (base, length) = (root.pivot[1], root.length.max(1e-4));
    buffers.wind = buffers
        .positions
        .iter()
        .map(|p| {
            [
                ((p[1] - base) / length).clamp(0.0, 1.0),
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
            ]
        })
        .collect();
}

//! Per-sample shading and the supersampling resolve.
//!
//! Attributes are rebuilt for each visible sample from the triangle id in the visibility
//! buffer. With the clip-space vertices `c_i = (x_i, y_i, z_i, w_i)` of the triangle, the
//! perspective-correct barycentric coordinates of the sample at normalized device
//! coordinates `(u, v)` are `M^-1 (u, v, 1)`, normalized to sum to one, where `M` has the
//! columns `(x_i, y_i, w_i)`. This needs no clipping bookkeeping, holds for perspective
//! and orthographic cameras alike, and the sign of `det M` tells whether the front face
//! is seen. The world position, normal and colour then come from the vertex buffers.
//!
//! Shading is Lambertian: a sun term attenuated by the shadow map, with light crossing
//! leaves lit from behind (`leaf_translucency`), plus a hemisphere ambient (sky above,
//! bounce below, blended by the normal's height) attenuated by the sky occlusion grid.
//! Normals of double-sided parts are flipped towards the viewer. The ground disc is
//! intersected analytically along the pixel ray and shaded the same way with an up
//! normal. The shaded colour is scaled by the exposure and compressed above 0.8 by a soft
//! shoulder before the samples of a pixel are averaged.

use crate::occlusion::OcclusionGrid;
use crate::raster::{NO_TRIANGLE, VisibilityBuffer};
use crate::scene::{Background, Ground, MeshView, PartView, Scene};
use crate::shadow::{DepthMap, sample_rotation};
use crate::target::RenderTarget;
use arbrume_core::mesh::PartKind;
use glam::{Mat3, Mat4, Vec2, Vec3, Vec4};

/// Everything the shading of one render reads.
#[derive(Debug)]
pub(crate) struct Shader<'a> {
    pub view: &'a MeshView<'a>,
    pub clip: &'a [Vec4],
    pub scene: &'a Scene,
    pub clip_from_world: Mat4,
    pub sun: Option<&'a DepthMap>,
    pub occlusion: &'a OcclusionGrid,
    /// Every ray reaches a point above the ground before any point below it, so a plant
    /// point above the ground is always seen in front of the ground.
    pub eye_above_ground: bool,
}

/// How a surface reacts to light.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Finish {
    /// Opaque plant surface.
    Opaque,
    /// Thin double-sided leaf that lets light through from behind.
    Leaf,
    /// The ground disc, which gets the finer shadow filter.
    Ground,
}

/// The plant surface at one sample.
#[derive(Debug, Clone, Copy)]
struct Surface {
    position: Vec3,
    normal: Vec3,
    albedo: Vec3,
    finish: Finish,
    depth: f32,
}

/// The triangle setup reused while consecutive samples hit the same triangle.
#[derive(Debug, Clone, Copy)]
struct TriangleCache {
    id: u32,
    part: usize,
    vertices: [usize; 3],
    inverse: Mat3,
    front: bool,
}

/// Accumulated samples of one output pixel.
#[derive(Debug, Clone, Copy, Default)]
struct Accumulator {
    color: Vec4,
    albedo: Vec3,
    normal: Vec3,
    depth: f32,
    covered: u32,
}

/// Shading key of samples that see the ground.
const GROUND_KEY: u32 = u32::MAX - 1;

/// Most distinct shading keys remembered within one pixel.
const MAX_SHADED: usize = 16;

/// Shaded colours of the distinct surfaces seen within one pixel.
#[derive(Debug, Clone, Copy)]
struct PixelShading {
    keys: [u32; MAX_SHADED],
    colors: [Vec4; MAX_SHADED],
    len: usize,
}

impl PixelShading {
    fn new() -> Self {
        Self {
            keys: [0; MAX_SHADED],
            colors: [Vec4::ZERO; MAX_SHADED],
            len: 0,
        }
    }

    /// The colour stored for `key`, or `shade()` stored when there is room.
    fn get_or(&mut self, key: u32, shade: impl FnOnce() -> Vec4) -> Vec4 {
        if let Some(i) = self.keys[..self.len].iter().position(|&k| k == key) {
            return self.colors[i];
        }
        let color = shade();
        if self.len < MAX_SHADED {
            self.keys[self.len] = key;
            self.colors[self.len] = color;
            self.len += 1;
        }
        color
    }
}

/// Shades every sample of `visibility` and box-filters them into a render target.
///
/// Samples of one pixel that see the same triangle (or the ground, or the background)
/// share one shading computation, made at the first of them, as in multisampling: the
/// supersampling resolves edges and coverage while the lighting is computed about once
/// per surface per pixel. Samples that miss the plant share the ground or background
/// found through the pixel centre; the ground disc fades out at its rim, so it needs no
/// edge antialiasing of its own.
pub(crate) fn resolve(shader: &Shader<'_>, visibility: &VisibilityBuffer) -> RenderTarget {
    let scene = shader.scene;
    let mut target = RenderTarget::new(scene.width, scene.height);
    let inverse = shader.clip_from_world.inverse();
    let mut cache: Option<TriangleCache> = None;
    let samples = scene.supersampling.max(1).pow(2) as f32;
    for py in 0..scene.height {
        for px in 0..scene.width {
            let acc = shade_pixel(shader, visibility, &inverse, (px, py), &mut cache);
            store(&mut target, (px, py), &acc, samples);
        }
    }
    target
}

fn shade_pixel(
    shader: &Shader<'_>,
    visibility: &VisibilityBuffer,
    world_from_clip: &Mat4,
    (px, py): (u32, u32),
    cache: &mut Option<TriangleCache>,
) -> Accumulator {
    let ss = shader.scene.supersampling.max(1);
    let mut acc = Accumulator {
        depth: 1.0,
        ..Accumulator::default()
    };
    let mut shaded = PixelShading::new();
    let rotation = sample_rotation(px, py);
    let row = (py as f32 + 0.5) / shader.scene.height.max(1) as f32;
    let centre = Vec2::new(
        (px as f32 + 0.5) / shader.scene.width.max(1) as f32 * 2.0 - 1.0,
        1.0 - row * 2.0,
    );
    if pixel_is_empty(visibility, (px, py), ss) {
        let hit = shader.classify(None, world_from_clip, centre);
        acc.color = shader.shade(&hit, rotation, row) * (ss * ss) as f32;
        return acc;
    }
    let mut backdrop: Option<Hit> = None;
    for sy in py * ss..(py + 1) * ss {
        for sx in px * ss..(px + 1) * ss {
            let index = sy as usize * visibility.width as usize + sx as usize;
            let ndc = sample_ndc(sx, sy, visibility);
            let id = visibility.id[index];
            let surface = shader.surface(id, ndc, visibility.depth[index], cache);
            let hit = if surface.is_some() {
                shader.classify(surface, world_from_clip, ndc)
            } else {
                *backdrop.get_or_insert_with(|| shader.classify(None, world_from_clip, centre))
            };
            let key = match hit {
                Hit::Plant(_) => id,
                Hit::Ground(_) => GROUND_KEY,
                Hit::Background => NO_TRIANGLE,
            };
            let color = shaded.get_or(key, || shader.shade(&hit, rotation, row));
            accumulate(&mut acc, color, surface);
        }
    }
    acc
}

/// Whether no sample of pixel `(px, py)` sees the plant.
fn pixel_is_empty(visibility: &VisibilityBuffer, (px, py): (u32, u32), ss: u32) -> bool {
    let width = visibility.width as usize;
    let (x0, x1) = ((px * ss) as usize, ((px + 1) * ss) as usize);
    (py * ss..(py + 1) * ss).all(|sy| {
        let row = sy as usize * width;
        visibility.id[row + x0..row + x1]
            .iter()
            .all(|&id| id == NO_TRIANGLE)
    })
}

fn accumulate(acc: &mut Accumulator, color: Vec4, surface: Option<Surface>) {
    acc.color += color;
    if let Some(s) = surface {
        acc.albedo += s.albedo;
        acc.normal += s.normal;
        acc.depth = acc.depth.min(s.depth);
        acc.covered += 1;
    }
}

fn store(target: &mut RenderTarget, (px, py): (u32, u32), acc: &Accumulator, samples: f32) {
    let i = py as usize * target.width as usize + px as usize;
    target.color[i] = (acc.color / samples).to_array();
    if acc.covered > 0 {
        target.albedo[i] = (acc.albedo / acc.covered as f32).to_array();
        target.normal[i] = acc.normal.normalize_or_zero().to_array();
        target.depth[i] = acc.depth;
        target.coverage[i] = acc.covered as f32 / samples;
    }
}

/// Normalized device coordinates of the centre of sample `(x, y)`.
fn sample_ndc(x: u32, y: u32, buffer: &VisibilityBuffer) -> Vec2 {
    Vec2::new(
        (x as f32 + 0.5) / buffer.width as f32 * 2.0 - 1.0,
        1.0 - (y as f32 + 0.5) / buffer.height as f32 * 2.0,
    )
}

/// A world-space ray through a sample, from the near plane.
#[derive(Debug, Clone, Copy)]
struct Ray {
    origin: Vec3,
    direction: Vec3,
}

impl Ray {
    fn through(world_from_clip: &Mat4, ndc: Vec2) -> Self {
        let near_h = world_from_clip.x_axis * ndc.x
            + world_from_clip.y_axis * ndc.y
            + world_from_clip.w_axis;
        let far_h = near_h + world_from_clip.z_axis;
        let near = near_h.truncate() / near_h.w;
        let far = far_h.truncate() / far_h.w;
        Self {
            origin: near,
            direction: (far - near).normalize_or_zero(),
        }
    }
}

/// Soft shoulder: identity below `knee`, then an asymptote at 1.
fn tone(x: f32) -> f32 {
    let knee = 0.8;
    if x <= knee {
        x.max(0.0)
    } else {
        let over = (x - knee) / (1.0 - knee);
        knee + (1.0 - knee) * over / (1.0 + over)
    }
}

fn tone_map(color: Vec3, exposure: f32) -> Vec3 {
    let c = color * exposure;
    Vec3::new(tone(c.x), tone(c.y), tone(c.z))
}

impl Shader<'_> {
    /// The plant surface seen at a sample, `None` for an empty sample.
    fn surface(
        &self,
        id: u32,
        ndc: Vec2,
        depth: f32,
        cache: &mut Option<TriangleCache>,
    ) -> Option<Surface> {
        if id == NO_TRIANGLE {
            return None;
        }
        let tri = match *cache {
            Some(c) if c.id == id => c,
            _ => {
                let fresh = self.setup(id)?;
                *cache = Some(fresh);
                fresh
            }
        };
        let raw = tri.inverse * Vec3::new(ndc.x, ndc.y, 1.0);
        let sum = raw.x + raw.y + raw.z;
        if sum.abs() < f32::MIN_POSITIVE {
            return None;
        }
        let bary = raw / sum;
        let part = &self.view.parts[tri.part];
        Some(interpolate(part, &tri, bary, depth))
    }

    fn setup(&self, id: u32) -> Option<TriangleCache> {
        let part_index = self.view.part_of(id)?;
        let part = &self.view.parts[part_index];
        let first = 3 * (id - part.first_id) as usize;
        let tri = part.indices.get(first..first + 3)?;
        let vertices = [tri[0] as usize, tri[1] as usize, tri[2] as usize];
        let column = |v: usize| {
            let c = self.clip[part.first_vertex + v];
            Vec3::new(c.x, c.y, c.w)
        };
        let m = Mat3::from_cols(
            column(vertices[0]),
            column(vertices[1]),
            column(vertices[2]),
        );
        let det = m.determinant();
        if det == 0.0 || !det.is_finite() {
            return None;
        }
        Some(TriangleCache {
            id,
            part: part_index,
            vertices,
            inverse: m.inverse(),
            front: det > 0.0,
        })
    }

    /// What the sample at `ndc` sees, given the plant surface found there.
    fn classify(&self, surface: Option<Surface>, world_from_clip: &Mat4, ndc: Vec2) -> Hit {
        let Some(ground) = self.scene.ground else {
            return surface.map_or(Hit::Background, Hit::Plant);
        };
        if let Some(s) = surface
            && s.position.y >= ground.height
            && self.eye_above_ground
        {
            return Hit::Plant(s);
        }
        let ray = Ray::through(world_from_clip, ndc);
        match (surface, hit_ground(&ground, ray)) {
            (Some(s), Some(g)) if g.distance < (s.position - ray.origin).dot(ray.direction) => {
                Hit::Ground(g)
            }
            (Some(s), _) => Hit::Plant(s),
            (None, Some(g)) => Hit::Ground(g),
            (None, None) => Hit::Background,
        }
    }

    /// Linear, premultiplied colour of a hit; `row` is the relative image height.
    fn shade(&self, hit: &Hit, rotation: Vec2, row: f32) -> Vec4 {
        let exposure = self.scene.lighting.exposure;
        match hit {
            Hit::Plant(s) => {
                let lit = self.light(s.position, s.normal, s.albedo, s.finish, rotation);
                tone_map(lit, exposure).extend(1.0)
            }
            Hit::Ground(g) => {
                let lit = self.light(g.position, Vec3::Y, g.albedo, Finish::Ground, rotation);
                let back = background(&self.scene.background, row);
                tone_map(lit, exposure).extend(1.0) * g.alpha + back * (1.0 - g.alpha)
            }
            Hit::Background => background(&self.scene.background, row),
        }
    }

    /// Lit linear colour of a surface point.
    fn light(
        &self,
        position: Vec3,
        normal: Vec3,
        albedo: Vec3,
        finish: Finish,
        rotation: Vec2,
    ) -> Vec3 {
        let lighting = &self.scene.lighting;
        let sun_dir = lighting.sun_direction.normalize_or_zero();
        let cosine = normal.dot(sun_dir);
        let mut direct = cosine.max(0.0);
        if finish == Finish::Leaf {
            direct += lighting.leaf_translucency * (-cosine).max(0.0);
        }
        if direct > 0.0
            && let Some(sun) = self.sun
        {
            let fine = finish == Finish::Ground;
            direct *= sun.visibility(position, normal, lighting.shadow_softness, rotation, fine);
        }
        let strength = lighting.sky_occlusion.clamp(0.0, 1.0);
        let ambient_visibility = 1.0 - strength * (1.0 - self.occlusion.visibility(position));
        let up = 0.5 + 0.5 * normal.y;
        let sky = Vec3::from(lighting.sky_color);
        let bounce = Vec3::from(lighting.bounce_color);
        let ambient = (sky * up + bounce * (1.0 - up)) * ambient_visibility;
        albedo * (Vec3::from(lighting.sun_color) * direct + ambient)
    }
}

/// Builds the surface of a sample from its barycentric coordinates.
fn interpolate(part: &PartView<'_>, tri: &TriangleCache, bary: Vec3, depth: f32) -> Surface {
    let [a, b, c] = tri.vertices;
    let pick = |buffer: &[[f32; 3]]| {
        Vec3::from(buffer[a]) * bary.x
            + Vec3::from(buffer[b]) * bary.y
            + Vec3::from(buffer[c]) * bary.z
    };
    let position = pick(part.positions);
    let face = (Vec3::from(part.positions[b]) - Vec3::from(part.positions[a]))
        .cross(Vec3::from(part.positions[c]) - Vec3::from(part.positions[a]));
    let mut normal = if part.normals.is_empty() {
        face
    } else {
        pick(part.normals)
    }
    .normalize_or(face.normalize_or(Vec3::Y));
    if !tri.front {
        normal = -normal;
    }
    let mut albedo = part.base_color;
    if !part.colors.is_empty() {
        let tint = |i: usize| Vec3::new(part.colors[i][0], part.colors[i][1], part.colors[i][2]);
        albedo *= tint(a) * bary.x + tint(b) * bary.y + tint(c) * bary.z;
    }
    Surface {
        position,
        normal,
        albedo: albedo.max(Vec3::ZERO),
        finish: if part.kind == PartKind::Leaf && part.double_sided {
            Finish::Leaf
        } else {
            Finish::Opaque
        },
        depth,
    }
}

/// What one sample sees.
#[derive(Debug, Clone, Copy)]
enum Hit {
    Plant(Surface),
    Ground(GroundHit),
    Background,
}

/// A ray's hit on the ground disc.
#[derive(Debug, Clone, Copy)]
struct GroundHit {
    position: Vec3,
    distance: f32,
    albedo: Vec3,
    alpha: f32,
}

fn hit_ground(ground: &Ground, ray: Ray) -> Option<GroundHit> {
    if ray.direction.y >= -1e-6 {
        return None;
    }
    let distance = (ground.height - ray.origin.y) / ray.direction.y;
    if distance <= 0.0 {
        return None;
    }
    let position = ray.origin + ray.direction * distance;
    let r = (Vec2::new(position.x, position.z) - ground.center).length() / ground.radius.max(1e-6);
    if r >= 1.0 {
        return None;
    }
    let fade = ((r - 0.45) / 0.55).clamp(0.0, 1.0);
    Some(GroundHit {
        position,
        distance,
        albedo: Vec3::from(ground.color),
        alpha: 1.0 - fade * fade * (3.0 - 2.0 * fade),
    })
}

/// Premultiplied background colour at the relative image height `t` (0 top, 1 bottom).
fn background(background: &Background, t: f32) -> Vec4 {
    match *background {
        Background::Solid(c) => Vec3::from(c).extend(1.0),
        Background::Gradient { top, bottom } => {
            Vec3::from(top).lerp(Vec3::from(bottom), t).extend(1.0)
        }
        Background::Transparent => Vec4::ZERO,
    }
}

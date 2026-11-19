//! The sun shadow map.
//!
//! A [`DepthMap`] is the plant rasterized, without culling, by an orthographic camera
//! looking along a light direction and framing the plant. Points outside the map are
//! unoccluded: nothing else in the scene casts shadows.
//!
//! The sun shadow is percentage-closer filtered: [`DepthMap::visibility`] averages the
//! depth test over a disc of taps, rotated per pixel by an integer hash so that the
//! banding of a fixed pattern turns into fine noise in the penumbra. Before the test, the
//! point is pushed along its normal, on the side facing the light, by a few texels (normal
//! offset), and the test tolerates a further depth bias, which keeps thin double-sided
//! leaves from shadowing themselves.

use crate::camera::Camera;
use crate::raster::{DrawCall, VisibilityBuffer, rasterize};
use crate::scene::MeshView;
use glam::{Mat4, Vec2, Vec3, Vec4};

/// Number of taps of the fine percentage-closer filter, used on the ground.
const FINE_TAPS: usize = 16;

/// Number of taps of the coarse filter, used on the plant, where leaves hide its noise.
const COARSE_TAPS: usize = 8;

/// `N` unit-disc tap offsets of the percentage-closer filter, a Vogel (golden angle)
/// spiral.
fn pcf_taps<const N: usize>() -> [Vec2; N] {
    let golden = core::f32::consts::PI * (3.0 - 5.0_f32.sqrt());
    core::array::from_fn(|i| {
        let r = ((i as f32 + 0.5) / N as f32).sqrt();
        let angle = golden * i as f32;
        Vec2::new(r * angle.cos(), r * angle.sin())
    })
}

/// The plant's depth seen along one light direction.
#[derive(Debug, Clone)]
pub struct DepthMap {
    /// World to clip transform of the light camera.
    pub clip_from_world: Mat4,
    /// Depth buffer of the light camera.
    pub buffer: VisibilityBuffer,
    /// World size of one texel, in meters.
    pub texel: f32,
    /// Meters per unit of clip-space depth.
    pub depth_scale: f32,
    /// Unit direction towards the light.
    pub toward_light: Vec3,
    fine: [Vec2; FINE_TAPS],
    coarse: [Vec2; COARSE_TAPS],
}

/// Inputs of a depth map render.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MapRequest<'a> {
    pub mesh: &'a MeshView<'a>,
    pub points: &'a [Vec3],
    pub toward_light: Vec3,
    pub size: u32,
}

impl DepthMap {
    /// An empty map (every point unoccluded).
    #[must_use]
    pub fn empty() -> Self {
        Self {
            clip_from_world: Mat4::IDENTITY,
            buffer: VisibilityBuffer::new(0, 0),
            texel: 1.0,
            depth_scale: 1.0,
            toward_light: Vec3::Y,
            fine: pcf_taps(),
            coarse: pcf_taps(),
        }
    }

    /// Rasterizes the plant into this map, reusing its buffers; `scratch` holds the
    /// transformed vertices.
    pub(crate) fn render(&mut self, request: &MapRequest<'_>, scratch: &mut Vec<Vec4>) {
        let dir = request.toward_light.normalize();
        let azimuth = dir.x.atan2(dir.z).to_degrees();
        let elevation = dir.y.clamp(-1.0, 1.0).asin().to_degrees();
        let camera = Camera::frame_points(request.points, azimuth, elevation, 0.0, 0.02);
        let crate::camera::Projection::Orthographic { half_size } = camera.projection else {
            return;
        };
        self.clip_from_world = camera.clip_from_world(1.0);
        self.texel = 2.0 * half_size / request.size.max(1) as f32;
        self.depth_scale = camera.far - camera.near;
        self.toward_light = dir;
        self.buffer.reset(request.size, request.size);
        for part in &request.mesh.parts {
            scratch.clear();
            scratch.extend(
                part.positions
                    .iter()
                    .map(|&p| self.clip_from_world * Vec3::from(p).extend(1.0)),
            );
            let call = DrawCall {
                clip: scratch,
                indices: part.indices,
                cull_back: false,
                first_id: 0,
                mask: None,
                alpha: part.alpha_test(),
            };
            rasterize(&mut self.buffer, &call);
        }
    }

    /// Texel coordinates and depth of a world point, `None` outside the map.
    fn locate(&self, point: Vec3) -> Option<(f32, f32, f32)> {
        let size = self.buffer.width as f32;
        if self.buffer.width == 0 {
            return None;
        }
        let clip = self.clip_from_world * point.extend(1.0);
        let u = (clip.x * 0.5 + 0.5) * size;
        let v = (0.5 - clip.y * 0.5) * size;
        (u >= 0.0 && v >= 0.0 && u < size && v < size).then_some((u, v, clip.z))
    }

    fn stored(&self, u: f32, v: f32) -> f32 {
        let size = self.buffer.width;
        let x = (u.max(0.0) as u32).min(size - 1);
        let y = (v.max(0.0) as u32).min(size - 1);
        self.buffer.depth[(y * size + x) as usize]
    }

    /// The point pushed off its surface towards the light by `texels` texels.
    fn offset(&self, point: Vec3, normal: Vec3, texels: f32) -> Vec3 {
        let side = if normal.dot(self.toward_light) < 0.0 {
            -normal
        } else {
            normal
        };
        point + side * (texels * self.texel)
    }

    /// Fraction of the filter disc of `radius` texels around `point` that sees the light.
    /// `rotation` is the (cosine, sine) of the per-pixel rotation of the taps; `fine`
    /// selects 16 taps instead of 8.
    #[must_use]
    pub fn visibility(
        &self,
        point: Vec3,
        normal: Vec3,
        radius: f32,
        rotation: Vec2,
        fine: bool,
    ) -> f32 {
        let Some((u, v, depth)) = self.locate(self.offset(point, normal, 1.5)) else {
            return 1.0;
        };
        let bias = 1.5 * self.texel / self.depth_scale;
        let taps: &[Vec2] = if fine { &self.fine } else { &self.coarse };
        let mut lit = 0;
        for tap in taps {
            let t = Vec2::new(
                tap.x * rotation.x - tap.y * rotation.y,
                tap.x * rotation.y + tap.y * rotation.x,
            ) * radius;
            let in_map = |c: f32| c.clamp(0.0, self.buffer.width as f32 - 1.0);
            if depth - bias <= self.stored(in_map(u + t.x), in_map(v + t.y)) {
                lit += 1;
            }
        }
        lit as f32 / taps.len() as f32
    }
}

/// A rotation (cosine, sine) from the integer hash of a pixel position. The angle is
/// that of a point drawn in a square around the origin, which avoids trigonometry and is
/// irregular enough to decorrelate neighbouring pixels.
#[must_use]
pub fn sample_rotation(column: u32, row: u32) -> Vec2 {
    let mut hash = column.wrapping_mul(0x9E37_79B1) ^ row.wrapping_mul(0x85EB_CA77);
    hash ^= hash >> 15;
    hash = hash.wrapping_mul(0x2C1B_3C6D);
    hash ^= hash >> 12;
    let low = (hash & 0xFFFF) as f32 / 32_768.0 - 0.999_98;
    let high = (hash >> 16) as f32 / 32_768.0 - 0.999_98;
    Vec2::new(low, high).normalize_or(Vec2::X)
}

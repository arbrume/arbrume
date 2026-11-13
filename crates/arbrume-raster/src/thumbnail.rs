//! One-call thumbnails.
//!
//! [`render_thumbnail`] frames the whole plant from a view azimuth and elevation, puts a
//! ground disc under it, lights it with a sun placed relative to the view (so every
//! thumbnail of a gallery is lit the same way whatever its view angle) and returns an
//! 8-bit sRGB image. For many thumbnails, keep a [`Renderer`] per thread and call
//! [`Renderer::thumbnail`].

use crate::camera::{Bounds, Camera, direction, mesh_points};
use crate::scene::{Background, Ground, Lighting, Renderer, Scene};
use crate::target::{Image, RenderTarget, srgb_to_linear};
use arbrume_core::mesh::TreeMesh;
use glam::Vec3;

/// Settings of [`render_thumbnail`]. Angles are in degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThumbnailOptions {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// View azimuth (see [`direction`]).
    pub azimuth: f32,
    /// View elevation above the horizon.
    pub elevation: f32,
    /// Field of view across the smaller side; 0 gives an orthographic view.
    pub fov: f32,
    /// Fraction of the half image size left free around the plant.
    pub margin: f32,
    /// Samples per pixel along each axis.
    pub supersampling: u32,
    /// Background.
    pub background: Background,
    /// Draw a ground disc that receives the shadow (not for an empty mesh).
    pub ground: bool,
    /// Sun azimuth relative to the view azimuth: 0 puts the sun behind the camera,
    /// negative values to the left of the image.
    pub sun_azimuth: f32,
    /// Sun elevation above the horizon.
    pub sun_elevation: f32,
    /// Lights and shading controls; `sun_direction` is replaced from the two sun angles.
    pub lighting: Lighting,
}

/// Linear RGB of an 8-bit sRGB colour.
fn srgb(r: u8, g: u8, b: u8) -> [f32; 3] {
    [r, g, b].map(|c| srgb_to_linear(f32::from(c) / 255.0))
}

impl Default for ThumbnailOptions {
    /// A 256 by 256 thumbnail, 3x3 supersampled, seen from 12 degrees above the horizon
    /// with a 30 degree lens, over a pale sky gradient, with a sun 45 degrees to the left
    /// and 45 degrees high.
    fn default() -> Self {
        Self {
            width: 256,
            height: 256,
            azimuth: 0.0,
            elevation: 12.0,
            fov: 30.0,
            margin: 0.06,
            supersampling: 3,
            background: Background::Gradient {
                top: srgb(196, 214, 232),
                bottom: srgb(236, 238, 240),
            },
            ground: true,
            sun_azimuth: -45.0,
            sun_elevation: 45.0,
            lighting: Lighting::default(),
        }
    }
}

impl ThumbnailOptions {
    /// The scene these options describe for `mesh`.
    #[must_use]
    pub fn scene(&self, mesh: &TreeMesh) -> Scene {
        let points: Vec<Vec3> = mesh_points(mesh).filter(|p| p.is_finite()).collect();
        let camera =
            Camera::frame_points(&points, self.azimuth, self.elevation, self.fov, self.margin);
        let ground = Bounds::of_points(points.iter().copied())
            .filter(|_| self.ground)
            .map(|bounds| Ground::under(&bounds, 1.6, srgb(136, 138, 120)));
        let sun_direction = direction(self.azimuth + self.sun_azimuth, self.sun_elevation);
        Scene {
            camera,
            width: self.width,
            height: self.height,
            supersampling: self.supersampling,
            lighting: Lighting {
                sun_direction,
                ..self.lighting
            },
            background: self.background,
            ground,
        }
    }
}

impl Renderer {
    /// Renders a thumbnail of `mesh` into a render target.
    pub fn thumbnail_target(
        &mut self,
        mesh: &TreeMesh,
        options: &ThumbnailOptions,
    ) -> RenderTarget {
        self.render(mesh, &options.scene(mesh))
    }

    /// Renders a thumbnail of `mesh`.
    pub fn thumbnail(&mut self, mesh: &TreeMesh, options: &ThumbnailOptions) -> Image {
        self.thumbnail_target(mesh, options).to_image()
    }
}

/// Renders a thumbnail of `mesh` with a fresh [`Renderer`].
#[must_use]
pub fn render_thumbnail(mesh: &TreeMesh, options: &ThumbnailOptions) -> Image {
    Renderer::new().thumbnail(mesh, options)
}

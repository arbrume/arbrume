//! Scene description and the renderer.
//!
//! A [`Scene`] is a camera, an output size with its supersampling factor, a [`Lighting`]
//! setup, a [`Background`] and an optional analytic [`Ground`] disc. [`Renderer::render`]
//! draws a [`TreeMesh`] in four steps:
//!
//! 1. transform every vertex to clip space and rasterize the parts into a visibility
//!    buffer of `supersampling` times the output size in each direction, culling back
//!    faces of single-sided parts;
//! 2. render the sun shadow map ([`crate::shadow`]) and fill the sky occlusion grid
//!    ([`crate::occlusion`]);
//! 3. shade the samples (see [`crate::shade`]): the attributes of the visible triangle
//!    are rebuilt at each sample from its id, lighting is computed once per surface per
//!    pixel, the ground disc is intersected analytically, and the background fills the
//!    rest;
//! 4. box-filter the samples of each pixel into a [`RenderTarget`].
//!
//! A [`Renderer`] keeps its buffers between renders, so rendering many plants of similar
//! size with one renderer reallocates only its output after the first. It runs on the
//! calling thread; render several plants in parallel with one renderer per thread.

use crate::camera::{Bounds, Camera, Projection};
use crate::occlusion::OcclusionGrid;
use crate::raster::{DrawCall, VisibilityBuffer, rasterize};
use crate::shade::{Shader, resolve};
use crate::shadow::{DepthMap, MapRequest};
use crate::target::RenderTarget;
use arbrume_core::mesh::{PartKind, TreeMesh};
use glam::{Vec2, Vec3, Vec4};

/// What fills the image behind the plant and the ground.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Background {
    /// One linear RGB colour.
    Solid([f32; 3]),
    /// A vertical gradient between two linear RGB colours.
    Gradient {
        /// Colour of the top row.
        top: [f32; 3],
        /// Colour of the bottom row.
        bottom: [f32; 3],
    },
    /// Fully transparent; the ground disc fades to transparent at its rim.
    Transparent,
}

/// A horizontal disc under the plant that receives its shadow.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ground {
    /// Height of the disc (world Y), in meters.
    pub height: f32,
    /// Centre of the disc in the XZ plane.
    pub center: Vec2,
    /// Radius in meters. The disc fades into the background over its outer part.
    pub radius: f32,
    /// Linear RGB albedo.
    pub color: [f32; 3],
}

impl Ground {
    /// A disc at the base of `bounds`, centred under it, with a radius of `scale` times
    /// the larger of the horizontal half-extent and half the height of the box.
    #[must_use]
    pub fn under(bounds: &Bounds, scale: f32, color: [f32; 3]) -> Self {
        let size = bounds.size();
        let reach = (0.5 * size.x.max(size.z)).max(0.5 * size.y).max(1e-3);
        let center = bounds.center();
        Self {
            height: bounds.min.y,
            center: Vec2::new(center.x, center.z),
            radius: scale * reach,
            color,
        }
    }
}

/// Lights and shading controls. Colours are linear RGB; intensities are relative.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lighting {
    /// Unit direction from the scene towards the sun.
    pub sun_direction: Vec3,
    /// Colour and intensity of the sun.
    pub sun_color: [f32; 3],
    /// Ambient light from the upper hemisphere.
    pub sky_color: [f32; 3],
    /// Ambient light bounced from below.
    pub bounce_color: [f32; 3],
    /// Cast sun shadows.
    pub shadows: bool,
    /// Side of the sun shadow map in texels; 0 picks twice the larger side of the
    /// output image, clamped to `[256, 4096]`, so that the penumbra keeps its size in
    /// pixels at every image size.
    pub shadow_map_size: u32,
    /// Radius of the shadow filter in shadow-map texels.
    pub shadow_softness: f32,
    /// Cells of the sky occlusion grid ([`crate::occlusion`]) along the largest side of
    /// the plant; 0 disables sky occlusion.
    pub occlusion_cells: u32,
    /// Strength of sky occlusion in `[0, 1]`: 1 applies the grid's visibility, 0 none.
    pub sky_occlusion: f32,
    /// Fraction of the sun light that crosses a double-sided leaf lit from behind.
    pub leaf_translucency: f32,
    /// Multiplier applied to shaded colours before display.
    pub exposure: f32,
}

impl Default for Lighting {
    /// A warm sun from the upper left-front quarter, a blue sky and a soft bounce.
    /// The values were chosen by eye on the starter species thumbnails.
    fn default() -> Self {
        Self {
            sun_direction: crate::camera::direction(-40.0, 50.0),
            sun_color: [2.2, 2.1, 1.9],
            sky_color: [0.42, 0.5, 0.62],
            bounce_color: [0.16, 0.15, 0.11],
            shadows: true,
            shadow_map_size: 0,
            shadow_softness: 2.5,
            occlusion_cells: 32,
            sky_occlusion: 1.0,
            leaf_translucency: 0.45,
            exposure: 1.0,
        }
    }
}

/// Everything but the mesh that a render needs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scene {
    /// Viewpoint.
    pub camera: Camera,
    /// Output width in pixels.
    pub width: u32,
    /// Output height in pixels.
    pub height: u32,
    /// Samples per pixel along each axis (1 for none, 2 for 2x2, 3 for 3x3, ...).
    pub supersampling: u32,
    /// Lights.
    pub lighting: Lighting,
    /// Background.
    pub background: Background,
    /// Ground disc, if any.
    pub ground: Option<Ground>,
}

/// Whether every ray of `camera` starts above the plane at `height` or heads down: then
/// a ray meets points above the plane before points below it.
fn eye_above(camera: &Camera, height: f32) -> bool {
    match camera.projection {
        Projection::Perspective { .. } => camera.eye.y >= height,
        Projection::Orthographic { .. } => camera.target.y <= camera.eye.y,
    }
}

/// Borrowed buffers of one mesh part, with its place in the renderer's id space.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PartView<'a> {
    pub kind: PartKind,
    pub positions: &'a [[f32; 3]],
    pub normals: &'a [[f32; 3]],
    pub colors: &'a [[f32; 4]],
    pub indices: &'a [u32],
    pub base_color: Vec3,
    pub double_sided: bool,
    /// Id of the first triangle of the part.
    pub first_id: u32,
    /// Index of the first vertex of the part in the concatenated clip buffer.
    pub first_vertex: usize,
}

/// The parts of a mesh, numbered for the visibility buffer.
#[derive(Debug, Clone)]
pub(crate) struct MeshView<'a> {
    pub parts: Vec<PartView<'a>>,
}

impl<'a> MeshView<'a> {
    pub(crate) fn new(mesh: &'a TreeMesh) -> Self {
        let mut first_id = 0_u32;
        let mut first_vertex = 0;
        let mut parts = Vec::with_capacity(mesh.parts.len());
        for part in &mesh.parts {
            let b = &part.buffers;
            let c = part.material.base_color;
            parts.push(PartView {
                kind: part.kind,
                positions: &b.positions,
                normals: if b.normals.len() == b.positions.len() {
                    &b.normals
                } else {
                    &[]
                },
                colors: if b.colors.len() == b.positions.len() {
                    &b.colors
                } else {
                    &[]
                },
                indices: &b.indices,
                base_color: Vec3::new(c[0], c[1], c[2]),
                double_sided: part.material.double_sided,
                first_id,
                first_vertex,
            });
            first_id = first_id.saturating_add(b.triangle_count() as u32);
            first_vertex += b.positions.len();
        }
        Self { parts }
    }

    /// The part holding triangle `id`.
    pub(crate) fn part_of(&self, id: u32) -> Option<usize> {
        self.parts.iter().rposition(|p| p.first_id <= id)
    }
}

/// Renders meshes, keeping its buffers from one render to the next.
#[derive(Debug, Clone)]
pub struct Renderer {
    visibility: VisibilityBuffer,
    clip: Vec<Vec4>,
    scratch: Vec<Vec4>,
    sun: DepthMap,
    occlusion: OcclusionGrid,
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer {
    /// A renderer with empty buffers.
    #[must_use]
    pub fn new() -> Self {
        Self {
            visibility: VisibilityBuffer::new(0, 0),
            clip: Vec::new(),
            scratch: Vec::new(),
            sun: DepthMap::empty(),
            occlusion: OcclusionGrid::new(),
        }
    }

    /// The visibility buffer of the last render, at sample resolution.
    #[must_use]
    pub fn visibility(&self) -> &VisibilityBuffer {
        &self.visibility
    }

    /// Renders `mesh` as seen in `scene`.
    pub fn render(&mut self, mesh: &TreeMesh, scene: &Scene) -> RenderTarget {
        let ss = scene.supersampling.max(1);
        let (width, height) = (scene.width * ss, scene.height * ss);
        let view = MeshView::new(mesh);
        let aspect = scene.width.max(1) as f32 / scene.height.max(1) as f32;
        let clip_from_world = scene.camera.clip_from_world(aspect);
        self.visibility.reset(width, height);
        self.clip.clear();
        for part in &view.parts {
            let transform = |&p: &[f32; 3]| clip_from_world * Vec3::from(p).extend(1.0);
            self.clip.extend(part.positions.iter().map(transform));
        }
        for part in &view.parts {
            let count = part.positions.len();
            let call = DrawCall {
                clip: &self.clip[part.first_vertex..part.first_vertex + count],
                indices: part.indices,
                cull_back: !part.double_sided,
                first_id: part.first_id,
            };
            rasterize(&mut self.visibility, &call);
        }
        let bounds = Bounds::of_mesh(mesh);
        let has_sun = scene.lighting.shadows && bounds.is_some();
        self.render_lights(&view, bounds, scene);
        let shader = Shader {
            view: &view,
            clip: &self.clip,
            scene,
            clip_from_world,
            sun: has_sun.then_some(&self.sun),
            occlusion: &self.occlusion,
            eye_above_ground: scene
                .ground
                .is_some_and(|g| eye_above(&scene.camera, g.height)),
        };
        resolve(&shader, &self.visibility)
    }

    fn render_lights(&mut self, view: &MeshView<'_>, bounds: Option<Bounds>, scene: &Scene) {
        let light = &scene.lighting;
        let Some(bounds) = bounds else {
            self.occlusion.build(
                view,
                &Bounds {
                    min: Vec3::ZERO,
                    max: Vec3::ONE,
                },
                0,
            );
            return;
        };
        if light.shadows {
            let auto = (2 * scene.width.max(scene.height)).clamp(256, 4096);
            let size = if light.shadow_map_size == 0 {
                auto
            } else {
                light.shadow_map_size
            };
            let request = MapRequest {
                mesh: view,
                points: &bounds.corners(),
                toward_light: light.sun_direction,
                size,
            };
            self.sun.render(&request, &mut self.scratch);
        }
        let cells = if light.sky_occlusion > 0.0 {
            light.occlusion_cells
        } else {
            0
        };
        self.occlusion.build(view, &bounds, cells);
    }
}

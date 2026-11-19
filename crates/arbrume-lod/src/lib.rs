//! Far levels of detail: a plant drawn as a few textured planes.
//!
//! [`bake_billboard`] turns a [`TreeMesh`] into one [`PartKind::Billboard`] part: a set
//! of alpha-masked quads and one texture atlas per plant, baked with the CPU rasterizer
//! of `arbrume-raster`. Two ways to choose the planes:
//!
//! * [`BillboardMethod::Cloud`], billboard clouds (Décoret et al. 2003) with the changes
//!   Fuhrmann et al. (2005) made for trees: wood and foliage are simplified separately,
//!   a face counts for a plane by its area times the cosine between their normals, and
//!   planes close to horizontal are penalized. See [`cloud`].
//! * [`BillboardMethod::Cross`], the classic crossed quads: a few vertical planes through
//!   the trunk, each showing the whole plant seen along its normal, and optionally one
//!   horizontal plane through the crown. See [`cross`].
//!
//! The atlas ([`atlas`]) holds, per plane, the albedo of what the plane stands for with
//! its sky occlusion baked in and the coverage in alpha (base colour texture), and the
//! tangent-space normal with the depth relative to the plane in alpha (normal texture).
//! Everything is deterministic: the same mesh and options give the same bytes.

#![forbid(unsafe_code)]

pub mod atlas;
pub mod cloud;
pub mod cross;

use arbrume_core::mesh::{MeshPart, PartKind, TreeMesh};
use glam::Vec3;

/// How the planes of a billboard are chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BillboardMethod {
    /// Billboard clouds with the tree-specific changes of Fuhrmann et al. (2005).
    Cloud,
    /// Vertical quads crossing at the trunk, and optionally a horizontal one.
    Cross,
}

/// Controls of [`bake_billboard`]. The defaults were chosen by eye on the presets; they
/// are starting points, not measured optima.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BillboardOptions {
    /// How the planes are chosen.
    pub method: BillboardMethod,
    /// Side of the square atlas in texels.
    pub atlas_size: u32,
    /// Samples per texel along each axis when baking.
    pub supersampling: u32,
    /// Alpha cutoff of the material, applied to the coverage. Fuhrmann et al. threshold
    /// their coverage at about 0.3.
    pub alpha_cutoff: f32,
    /// Bake the sky occlusion of the whole plant into the albedo.
    pub bake_occlusion: bool,
    /// Cloud: largest distance of a face to its plane, as a fraction of the radius of
    /// the bounding sphere of the plant (Fuhrmann et al. use 6.5 % to 12.5 %).
    pub epsilon: f32,
    /// Cloud: most planes, wood and foliage together.
    pub max_planes: usize,
    /// Cloud: most planes of the wood (bark part).
    pub max_wood_planes: usize,
    /// Cloud: candidate plane normals, spread over a hemisphere.
    pub directions: usize,
    /// Cloud: largest penalty of a horizontal plane, in `[0, 1]` (60 % in Fuhrmann et
    /// al.).
    pub horizontal_penalty: f32,
    /// Cloud: elevation of the plane normal, in degrees, above which the penalty starts
    /// (50 in Fuhrmann et al.).
    pub penalty_cutoff_deg: f32,
    /// Cloud: most faces used to choose the planes; a larger mesh is sampled with a
    /// regular stride. Every face is still drawn.
    pub max_faces: usize,
    /// Cross: number of vertical planes.
    pub cross_planes: u32,
    /// Cross: add a horizontal plane through the middle of the crown.
    pub cross_horizontal: bool,
}

impl Default for BillboardOptions {
    fn default() -> Self {
        Self {
            method: BillboardMethod::Cloud,
            atlas_size: 1024,
            supersampling: 3,
            alpha_cutoff: 0.3,
            bake_occlusion: false,
            epsilon: 0.1,
            max_planes: 16,
            max_wood_planes: 4,
            directions: 192,
            horizontal_penalty: 0.6,
            penalty_cutoff_deg: 50.0,
            max_faces: 60_000,
            cross_planes: 3,
            cross_horizontal: false,
        }
    }
}

/// A plane of a billboard with the faces it stands for.
#[derive(Debug, Clone, PartialEq)]
pub struct Plane {
    /// Unit normal; the quad faces this way (it is drawn from both sides).
    pub normal: Vec3,
    /// A point of the plane.
    pub origin: Vec3,
    /// Unit vector of the plane along increasing `u` of the atlas (image right).
    pub right: Vec3,
    /// Unit vector of the plane towards the top of the atlas image.
    pub up: Vec3,
    /// Per part of the source mesh, whether each triangle is drawn on this plane; an
    /// empty mask draws the whole part.
    pub masks: Vec<Vec<bool>>,
    /// Largest distance of a drawn face to the plane, for the depth encoding.
    pub depth_range: f32,
}

impl Plane {
    /// A plane through `origin` with normal `normal`, its image axes chosen so that `up`
    /// is the world up direction projected on the plane (or `+Z` for a horizontal
    /// plane).
    #[must_use]
    pub fn new(normal: Vec3, origin: Vec3, masks: Vec<Vec<bool>>, depth_range: f32) -> Self {
        let normal = normal.normalize();
        let up = Vec3::Y
            .reject_from_normalized(normal)
            .try_normalize()
            .unwrap_or_else(|| Vec3::NEG_Z.reject_from_normalized(normal).normalize());
        Self {
            normal,
            origin,
            right: up.cross(normal),
            up,
            masks,
            depth_range,
        }
    }
}

/// The result of [`bake_billboard`].
#[derive(Debug, Clone, PartialEq)]
pub struct Billboard {
    /// The billboard part: quads, alpha-masked double-sided material with the atlas as
    /// base colour and normal textures. Carries wind data when the source mesh did.
    pub part: MeshPart,
    /// Number of planes.
    pub planes: usize,
    /// Side of the atlas in texels.
    pub atlas_size: u32,
    /// Texels per meter of the atlas.
    pub texels_per_meter: f32,
    /// Depth range of the normal texture alpha: alpha `a` is at `(2 a - 1) depth_range`
    /// meters in front of its plane.
    pub depth_range: f32,
    /// Height of the plant on screen, in pixels, up to which the atlas is not
    /// magnified: above it, use a mesh level.
    pub max_screen_height: f32,
}

/// Bakes the far level of detail of `mesh`. `None` for a mesh without triangles.
#[must_use]
pub fn bake_billboard(mesh: &TreeMesh, options: &BillboardOptions) -> Option<Billboard> {
    let bounds = Bounds3::of(mesh)?;
    let planes = match options.method {
        BillboardMethod::Cloud => cloud::choose_planes(mesh, &bounds, options),
        BillboardMethod::Cross => cross::planes(&bounds, options),
    };
    let baked = atlas::bake(mesh, &planes, &bounds, options)?;
    let height = bounds.max.y - bounds.min.y;
    Some(Billboard {
        planes: planes.len(),
        atlas_size: options.atlas_size,
        texels_per_meter: baked.texels_per_meter,
        depth_range: baked.depth_range,
        max_screen_height: height * baked.texels_per_meter,
        part: MeshPart {
            kind: PartKind::Billboard,
            material: baked.material,
            buffers: baked.buffers,
        },
    })
}

/// Axis-aligned bounds of every vertex of a mesh.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds3 {
    /// Smallest corner.
    pub min: Vec3,
    /// Largest corner.
    pub max: Vec3,
}

impl Bounds3 {
    /// The bounds of the vertices of the triangles of `mesh`, `None` without triangles.
    #[must_use]
    pub fn of(mesh: &TreeMesh) -> Option<Self> {
        let mut bounds: Option<Self> = None;
        for part in mesh.parts.iter().filter(|p| !p.buffers.indices.is_empty()) {
            for &p in &part.buffers.positions {
                let p = Vec3::from(p);
                bounds = Some(bounds.map_or(Self { min: p, max: p }, |b| Self {
                    min: b.min.min(p),
                    max: b.max.max(p),
                }));
            }
        }
        bounds
    }

    /// Centre of the box.
    #[must_use]
    pub fn center(&self) -> Vec3 {
        0.5 * (self.min + self.max)
    }

    /// Radius of the sphere around the box.
    #[must_use]
    pub fn radius(&self) -> f32 {
        (0.5 * (self.max - self.min).length()).max(1e-4)
    }
}

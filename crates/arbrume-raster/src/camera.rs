//! Cameras and automatic framing.
//!
//! A [`Camera`] is an eye, a target, an up vector and a [`Projection`]. Its field of view
//! (or orthographic half-size) spans the smaller side of the image, so a framed object
//! stays inside the image whatever the aspect ratio.
//!
//! [`Camera::frame`] places a camera on a sphere around a box, looking at it from an
//! azimuth and an elevation, and pulls it back until every corner of the box projects
//! inside the image with a margin. [`Camera::frame_points`] does the same for a point
//! set, which fits a plant more tightly than its bounding box. Clip space follows `glam`'s
//! right-handed, zero-to-one depth convention: depth 0 on the near plane, 1 on the far one.

use arbrume_core::mesh::TreeMesh;
use glam::camera::rh::proj::directx;
use glam::{Mat4, Vec3};

/// Fraction of the half image size left free around a framed object.
pub const DEFAULT_MARGIN: f32 = 0.08;

/// Axis-aligned bounding box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    /// Smallest corner.
    pub min: Vec3,
    /// Largest corner.
    pub max: Vec3,
}

impl Bounds {
    /// The bounds of a point set, `None` when it is empty or holds no finite point.
    #[must_use]
    pub fn of_points(points: impl IntoIterator<Item = Vec3>) -> Option<Self> {
        let mut bounds: Option<Self> = None;
        for point in points.into_iter().filter(|p| p.is_finite()) {
            bounds = Some(match bounds {
                None => Self {
                    min: point,
                    max: point,
                },
                Some(b) => Self {
                    min: b.min.min(point),
                    max: b.max.max(point),
                },
            });
        }
        bounds
    }

    /// The bounds of every vertex of `mesh`, `None` for an empty mesh.
    #[must_use]
    pub fn of_mesh(mesh: &TreeMesh) -> Option<Self> {
        Self::of_points(mesh_points(mesh))
    }

    /// Centre of the box.
    #[must_use]
    pub fn center(&self) -> Vec3 {
        0.5 * (self.min + self.max)
    }

    /// Size of the box along each axis.
    #[must_use]
    pub fn size(&self) -> Vec3 {
        self.max - self.min
    }

    /// The eight corners.
    #[must_use]
    pub fn corners(&self) -> [Vec3; 8] {
        let (a, b) = (self.min, self.max);
        [
            Vec3::new(a.x, a.y, a.z),
            Vec3::new(b.x, a.y, a.z),
            Vec3::new(a.x, b.y, a.z),
            Vec3::new(b.x, b.y, a.z),
            Vec3::new(a.x, a.y, b.z),
            Vec3::new(b.x, a.y, b.z),
            Vec3::new(a.x, b.y, b.z),
            Vec3::new(b.x, b.y, b.z),
        ]
    }
}

/// Every vertex position of `mesh`.
pub(crate) fn mesh_points(mesh: &TreeMesh) -> impl Iterator<Item = Vec3> + '_ {
    mesh.parts
        .iter()
        .flat_map(|part| part.buffers.positions.iter().map(|&p| Vec3::from(p)))
}

/// How a camera projects the scene.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Projection {
    /// Pinhole camera with this field of view, in radians, across the smaller image side.
    Perspective {
        /// Full field of view in radians.
        fov: f32,
    },
    /// Parallel projection showing this half extent, in meters, across the smaller side.
    Orthographic {
        /// Half of the visible extent in meters.
        half_size: f32,
    },
}

/// A viewpoint with its projection and depth range.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// Eye position.
    pub eye: Vec3,
    /// Point looked at; it projects to the image centre.
    pub target: Vec3,
    /// Approximate up direction of the image.
    pub up: Vec3,
    /// Projection.
    pub projection: Projection,
    /// Distance from the eye to the near plane, positive.
    pub near: f32,
    /// Distance from the eye to the far plane, larger than `near`.
    pub far: f32,
}

/// Orthonormal basis of a view direction: `right`, `up`, and `back` pointing to the eye.
#[derive(Debug, Clone, Copy)]
struct ViewBasis {
    right: Vec3,
    up: Vec3,
    back: Vec3,
}

impl ViewBasis {
    fn from_back(back: Vec3) -> Self {
        let back = back.normalize();
        let helper = if back.y.abs() > 0.999 {
            Vec3::Z
        } else {
            Vec3::Y
        };
        let right = helper.cross(back).normalize();
        Self {
            right,
            up: back.cross(right),
            back,
        }
    }
}

/// The unit direction from the origin towards a point seen at `azimuth` and `elevation`,
/// in degrees. Azimuth 0 is the +Z axis and grows towards +X; elevation 90 is +Y.
#[must_use]
pub fn direction(azimuth: f32, elevation: f32) -> Vec3 {
    let (az, el) = (azimuth.to_radians(), elevation.to_radians());
    Vec3::new(el.cos() * az.sin(), el.sin(), el.cos() * az.cos())
}

impl Camera {
    /// A camera looking at `bounds` from `azimuth` and `elevation` (degrees, see
    /// [`direction`]) that shows every corner of the box with [`DEFAULT_MARGIN`].
    ///
    /// `fov` is the field of view in degrees across the smaller image side; 0 or less
    /// gives an orthographic camera.
    #[must_use]
    pub fn frame(bounds: &Bounds, azimuth: f32, elevation: f32, fov: f32) -> Self {
        Self::frame_points(&bounds.corners(), azimuth, elevation, fov, DEFAULT_MARGIN)
    }

    /// Like [`Camera::frame`] for a point set, with an explicit `margin` (fraction of the
    /// half image size left free, in `[0, 1)`). An empty set frames the unit cube.
    #[must_use]
    pub fn frame_points(
        points: &[Vec3],
        azimuth: f32,
        elevation: f32,
        fov: f32,
        margin: f32,
    ) -> Self {
        let basis = ViewBasis::from_back(direction(azimuth, elevation));
        let unit = Bounds {
            min: Vec3::ZERO,
            max: Vec3::ONE,
        };
        let bounds = Bounds::of_points(points.iter().copied()).unwrap_or(unit);
        let fill = (1.0 - margin).clamp(0.05, 1.0);
        if fov > 0.0 {
            fit_perspective(points, &bounds, basis, fov.to_radians(), fill)
        } else {
            fit_orthographic(points, &bounds, basis, fill)
        }
    }

    /// World to view transform.
    #[must_use]
    pub fn view(&self) -> Mat4 {
        glam::camera::rh::view::look_at_mat4(self.eye, self.target, self.up)
    }

    /// View to clip transform for an image of `aspect` = width / height.
    #[must_use]
    pub fn projection_matrix(&self, aspect: f32) -> Mat4 {
        let aspect = if aspect.is_finite() && aspect > 0.0 {
            aspect
        } else {
            1.0
        };
        match self.projection {
            Projection::Perspective { fov } => {
                let half = (0.5 * fov).tan();
                let half_y = if aspect >= 1.0 { half } else { half / aspect };
                directx::perspective(2.0 * half_y.atan(), aspect, self.near, self.far)
            }
            Projection::Orthographic { half_size } => {
                let (hx, hy) = if aspect >= 1.0 {
                    (half_size * aspect, half_size)
                } else {
                    (half_size, half_size / aspect)
                };
                directx::orthographic(-hx, hx, -hy, hy, self.near, self.far)
            }
        }
    }

    /// World to clip transform for an image of `aspect` = width / height.
    #[must_use]
    pub fn clip_from_world(&self, aspect: f32) -> Mat4 {
        self.projection_matrix(aspect) * self.view()
    }
}

/// Coordinates of `p - origin` in `basis`: (right, up, back).
fn local(p: Vec3, origin: Vec3, basis: ViewBasis) -> Vec3 {
    let d = p - origin;
    Vec3::new(d.dot(basis.right), d.dot(basis.up), d.dot(basis.back))
}

fn fit_orthographic(points: &[Vec3], bounds: &Bounds, basis: ViewBasis, fill: f32) -> Camera {
    let center = bounds.center();
    let local_points = Bounds::of_points(points.iter().map(|&p| local(p, center, basis)))
        .unwrap_or(Bounds {
            min: -Vec3::ONE,
            max: Vec3::ONE,
        });
    let mid = local_points.center();
    let half = 0.5 * local_points.size();
    let half_size = (half.x.max(half.y) / fill).max(1e-3);
    let target = center + mid.x * basis.right + mid.y * basis.up;
    let depth = half.z.max(1e-3);
    let distance = mid.z + 2.0 * depth + half_size;
    Camera {
        eye: target + distance * basis.back,
        target,
        up: basis.up,
        projection: Projection::Orthographic { half_size },
        near: distance - mid.z - 1.5 * depth,
        far: distance - mid.z + 1.5 * depth,
    }
}

/// Smallest eye distance (along `back`, from the shifted target) at which every point
/// projects within `tan_half` of the axis, and the depth range of the points.
fn perspective_distance(locals: &[Vec3], shift: (f32, f32), tan_half: f32) -> (f32, f32, f32) {
    let mut distance = f32::MIN;
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for p in locals {
        let spread = (p.x - shift.0).abs().max((p.y - shift.1).abs());
        distance = distance.max(p.z + spread / tan_half);
        lo = lo.min(p.z);
        hi = hi.max(p.z);
    }
    (distance, lo, hi)
}

fn fit_perspective(
    points: &[Vec3],
    bounds: &Bounds,
    basis: ViewBasis,
    fov: f32,
    fill: f32,
) -> Camera {
    let fov = fov.clamp(1e-3, 3.0);
    let tan_half = (0.5 * fov).tan() * fill;
    let center = bounds.center();
    let mut locals: Vec<Vec3> = points.iter().map(|&p| local(p, center, basis)).collect();
    if locals.is_empty() {
        locals = bounds
            .corners()
            .iter()
            .map(|&p| local(p, center, basis))
            .collect();
    }
    let ortho = Bounds::of_points(locals.iter().copied()).unwrap_or(Bounds {
        min: Vec3::ZERO,
        max: Vec3::ZERO,
    });
    let mut shift = (ortho.center().x, ortho.center().y);
    let (mut distance, mut lo, mut hi) = perspective_distance(&locals, shift, tan_half);
    for _ in 0..3 {
        let (mut umin, mut umax, mut vmin, mut vmax) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for p in &locals {
            let depth = (distance - p.z).max(1e-6);
            let (u, v) = ((p.x - shift.0) / depth, (p.y - shift.1) / depth);
            (umin, umax, vmin, vmax) = (umin.min(u), umax.max(u), vmin.min(v), vmax.max(v));
        }
        let depth = distance - f32::midpoint(lo, hi);
        shift.0 += f32::midpoint(umin, umax) * depth;
        shift.1 += f32::midpoint(vmin, vmax) * depth;
        (distance, lo, hi) = perspective_distance(&locals, shift, tan_half);
    }
    let distance = distance.max(hi + 1e-3);
    let target = center + shift.0 * basis.right + shift.1 * basis.up;
    let nearest = distance - hi;
    let farthest = distance - lo;
    Camera {
        eye: target + distance * basis.back,
        target,
        up: basis.up,
        projection: Projection::Perspective { fov },
        near: (0.5 * nearest).max(1e-4 * farthest).max(1e-6),
        far: farthest * 1.5 + 1e-3,
    }
}

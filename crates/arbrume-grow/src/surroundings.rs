//! What stands around a plant: a crown envelope, obstacles and neighbours.
//!
//! [`Surroundings`] is read by the self-organizing mode. Everything is expressed in
//! meters in the frame of the plant: the plant stands at the origin, Y is up.
//!
//! * The **envelope** is the volume the crown may fill. Shoots stop at its surface, and
//!   with space colonization the markers are scattered inside it (Runions et al. 2007).
//! * An **occluder** is a volume that blocks growth, casts shade, or both. A wall or the
//!   clip volume of a hedge (Prusinkiewicz et al. 1994) blocks; the crown of a
//!   neighbouring tree only shades.
//!
//! Volumes are analytic [`Shape`]s or a [`VoxelMask`], which is how an arbitrary mesh
//! is passed in once voxelized.
//!
//! # Shade of an occluder
//!
//! The shadow model of Palubicki et al. (2009) lets every metamer darken a pyramid of
//! voxels below it by `a * b^-q` at depth `q`. A solid volume is treated as one opaque
//! layer per depth: a point receives `shade * a * b^-q` for every `q` in `0..=q_max`
//! such that the volume comes within `q` voxels of the point lifted by `q` voxels. This
//! is a coarse rule of this crate with one strength per occluder, not a light
//! simulation.

use arbrume_core::graph::Aabb;
use glam::Vec3;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// A volume. Analytic shapes are read from configuration files; [`Shape::Voxels`] is
/// built in code.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "shape", rename_all = "snake_case")]
pub enum Shape {
    /// A ball.
    Sphere {
        /// Centre.
        center: Vec3,
        /// Radius.
        radius: f32,
    },
    /// An ellipsoid with its axes along X, Y and Z.
    Ellipsoid {
        /// Centre.
        center: Vec3,
        /// Half-extent along each axis.
        radii: Vec3,
    },
    /// An axis-aligned box.
    Box {
        /// Smallest coordinates.
        min: Vec3,
        /// Largest coordinates.
        max: Vec3,
    },
    /// A vertical cylinder.
    Cylinder {
        /// Centre of the bottom disc.
        base: Vec3,
        /// Radius.
        radius: f32,
        /// Height above `base`.
        height: f32,
    },
    /// A vertical cone, apex up.
    Cone {
        /// Centre of the bottom disc.
        base: Vec3,
        /// Radius of the bottom disc.
        radius: f32,
        /// Height of the apex above `base`.
        height: f32,
    },
    /// A voxel mask.
    #[serde(skip)]
    Voxels(Arc<VoxelMask>),
}

impl Shape {
    /// Whether `point` is inside the volume (boundary included).
    #[must_use]
    pub fn contains(&self, point: Vec3) -> bool {
        match self {
            Self::Voxels(mask) => mask.contains(point),
            _ => self.distance(point) <= 0.0,
        }
    }

    /// Whether the volume comes within `reach` meters of `point`.
    ///
    /// Exact for spheres, boxes and cylinders, a first-order estimate for ellipsoids and
    /// cones. A voxel mask is sampled at `point` and at the eight horizontal offsets of
    /// `reach` around it.
    #[must_use]
    pub fn near(&self, point: Vec3, reach: f32) -> bool {
        match self {
            Self::Voxels(mask) => {
                let offsets = [-reach, 0.0, reach];
                offsets.iter().any(|&x| {
                    offsets
                        .iter()
                        .any(|&z| mask.contains(point + Vec3::new(x, 0.0, z)))
                })
            }
            _ => self.distance(point) <= reach,
        }
    }

    /// Signed distance from `point` to the surface, negative inside. See [`Shape::near`]
    /// for its accuracy. A voxel mask gives minus one voxel edge inside, and outside one
    /// voxel edge more than the distance to its bounding box.
    #[must_use]
    pub fn distance(&self, point: Vec3) -> f32 {
        match *self {
            Self::Sphere { center, radius } => point.distance(center) - radius,
            Self::Ellipsoid { center, radii } => {
                let scaled = ((point - center) / radii).length();
                (scaled - 1.0) * radii.min_element()
            }
            Self::Box { min, max } => {
                let outside = (min - point).max(point - max);
                outside.max(Vec3::ZERO).length() + outside.max_element().min(0.0)
            }
            Self::Cylinder {
                base,
                radius,
                height,
            } => {
                let local = point - base;
                let radial = Vec3::new(local.x, 0.0, local.z).length() - radius;
                let vertical = (-local.y).max(local.y - height);
                radial.max(0.0).hypot(vertical.max(0.0)) + radial.max(vertical).min(0.0)
            }
            Self::Cone {
                base,
                radius,
                height,
            } => {
                let local = point - base;
                let slope = radius.hypot(height);
                let side = Vec3::new(local.x, 0.0, local.z).length() - radius;
                let lateral = (side * height + local.y * radius) / slope;
                lateral.max(-local.y)
            }
            Self::Voxels(ref mask) => {
                if mask.contains(point) {
                    return -mask.cell;
                }
                let bounds = mask.bounds();
                let outside = (bounds.min - point).max(point - bounds.max);
                outside.max(Vec3::ZERO).length() + mask.cell
            }
        }
    }

    /// Bounding box of the volume.
    #[must_use]
    pub fn bounds(&self) -> Aabb {
        match *self {
            Self::Sphere { center, radius } => Aabb {
                min: center - Vec3::splat(radius),
                max: center + Vec3::splat(radius),
            },
            Self::Ellipsoid { center, radii } => Aabb {
                min: center - radii,
                max: center + radii,
            },
            Self::Box { min, max } => Aabb { min, max },
            Self::Cylinder {
                base,
                radius,
                height,
            }
            | Self::Cone {
                base,
                radius,
                height,
            } => Aabb {
                min: base - Vec3::new(radius, 0.0, radius),
                max: base + Vec3::new(radius, height, radius),
            },
            Self::Voxels(ref mask) => mask.bounds(),
        }
    }

    /// Checks that the sizes are positive and every number finite.
    fn validate(&self) -> Result<(), String> {
        let bounds = self.bounds();
        let size = bounds.max - bounds.min;
        if !(bounds.min.is_finite() && bounds.max.is_finite()) {
            return Err(format!("{self:?} has a value that is not finite"));
        }
        if size.min_element() <= 0.0 {
            return Err(format!("{self:?} has no volume"));
        }
        Ok(())
    }
}

/// A volume given as a grid of filled and empty cubic voxels.
#[derive(Debug, Clone, PartialEq)]
pub struct VoxelMask {
    origin: Vec3,
    cell: f32,
    size: [u32; 3],
    bits: Vec<u64>,
}

impl VoxelMask {
    /// An empty mask of `size` voxels of edge `cell` meters along X, Y and Z, whose
    /// corner of smallest coordinates is `origin`.
    #[must_use]
    pub fn new(origin: Vec3, cell: f32, size: [u32; 3]) -> Self {
        let count = size.iter().map(|&n| n as usize).product::<usize>();
        Self {
            origin,
            cell,
            size,
            bits: vec![0; count.div_ceil(64)],
        }
    }

    /// A mask whose voxels are filled where `inside` holds at their centre.
    #[must_use]
    pub fn from_fn(
        origin: Vec3,
        cell: f32,
        size: [u32; 3],
        mut inside: impl FnMut(Vec3) -> bool,
    ) -> Self {
        let mut mask = Self::new(origin, cell, size);
        let [nx, ny, nz] = size;
        let voxels =
            (0..nz).flat_map(|z| (0..ny).flat_map(move |y| (0..nx).map(move |x| [x, y, z])));
        for voxel in voxels {
            let [x, y, z] = voxel.map(|v| v as f32);
            if inside(origin + (Vec3::new(x, y, z) + 0.5) * cell) {
                mask.set(voxel, true);
            }
        }
        mask
    }

    /// Number of voxels along X, Y and Z.
    #[must_use]
    pub const fn size(&self) -> [u32; 3] {
        self.size
    }

    /// Edge of a voxel, in meters.
    #[must_use]
    pub const fn cell(&self) -> f32 {
        self.cell
    }

    /// Bit index of `voxel`, or `None` outside the grid.
    fn index(&self, voxel: [u32; 3]) -> Option<usize> {
        let inside = voxel.iter().zip(&self.size).all(|(v, n)| v < n);
        let [x, y, z] = voxel.map(|v| v as usize);
        let [nx, ny, _] = self.size.map(|n| n as usize);
        inside.then(|| (z * ny + y) * nx + x)
    }

    /// Fills or empties `voxel`. Voxels outside the grid are ignored.
    pub fn set(&mut self, voxel: [u32; 3], filled: bool) {
        if let Some(index) = self.index(voxel) {
            let bit = 1_u64 << (index % 64);
            if filled {
                self.bits[index / 64] |= bit;
            } else {
                self.bits[index / 64] &= !bit;
            }
        }
    }

    /// Whether `voxel` is filled; `false` outside the grid.
    #[must_use]
    pub fn get(&self, voxel: [u32; 3]) -> bool {
        self.index(voxel)
            .is_some_and(|index| self.bits[index / 64] >> (index % 64) & 1 == 1)
    }

    /// The filled voxels, by increasing Z, then Y, then X.
    pub fn filled(&self) -> impl Iterator<Item = [u32; 3]> + '_ {
        let [nx, ny, nz] = self.size;
        let voxels =
            (0..nz).flat_map(move |z| (0..ny).flat_map(move |y| (0..nx).map(move |x| [x, y, z])));
        voxels.filter(|&voxel| self.get(voxel))
    }

    /// Whether `point` lies in a filled voxel.
    #[must_use]
    pub fn contains(&self, point: Vec3) -> bool {
        let local = ((point - self.origin) / self.cell).floor();
        if local.min_element() < 0.0 {
            return false;
        }
        self.get([local.x as u32, local.y as u32, local.z as u32])
    }

    /// Bounding box of the grid.
    #[must_use]
    pub fn bounds(&self) -> Aabb {
        let size = Vec3::new(
            self.size[0] as f32,
            self.size[1] as f32,
            self.size[2] as f32,
        );
        Aabb {
            min: self.origin,
            max: self.origin + size * self.cell,
        }
    }
}

/// A volume that blocks growth, casts shade, or both.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Occluder {
    /// The volume.
    #[serde(flatten)]
    pub shape: Shape,
    /// No shoot enters the volume.
    #[serde(default = "yes")]
    pub blocks: bool,
    /// Strength of the shade the volume casts, as a multiple of the shadow of one
    /// metamer per layer (see the [module documentation](self)). 0 casts none.
    #[serde(default = "one")]
    pub shade: f32,
}

const fn yes() -> bool {
    true
}

const fn one() -> f32 {
    1.0
}

impl Occluder {
    /// A solid obstacle: blocks growth and casts shade of strength 1.
    #[must_use]
    pub const fn obstacle(shape: Shape) -> Self {
        Self {
            shape,
            blocks: true,
            shade: 1.0,
        }
    }

    /// A neighbouring crown: casts shade of strength `shade` and lets shoots through.
    #[must_use]
    pub const fn neighbour(shape: Shape, shade: f32) -> Self {
        Self {
            shape,
            blocks: false,
            shade,
        }
    }
}

/// Envelope and occluders around one plant. See the [module documentation](self).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Surroundings {
    /// Volume the crown may fill; `None` for no limit.
    pub envelope: Option<Shape>,
    /// Volumes that block growth or cast shade.
    pub occluders: Vec<Occluder>,
}

impl Surroundings {
    /// Open ground: no envelope, no occluder.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            envelope: None,
            occluders: Vec::new(),
        }
    }

    /// `true` when there is neither envelope nor occluder.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.envelope.is_none() && self.occluders.is_empty()
    }

    /// Whether `point` is closed to growth: outside the envelope or inside a blocking
    /// occluder.
    #[must_use]
    pub fn blocked(&self, point: Vec3) -> bool {
        let outside = self
            .envelope
            .as_ref()
            .is_some_and(|envelope| !envelope.contains(point));
        outside || self.obstructed(point)
    }

    /// Whether `point` is inside a blocking occluder.
    fn obstructed(&self, point: Vec3) -> bool {
        self.occluders
            .iter()
            .any(|occluder| occluder.blocks && occluder.shape.contains(point))
    }

    /// Whether a shoot may not step from `from` to `to`: `to` is inside a blocking
    /// occluder, or the step leaves the envelope. A step that starts outside the
    /// envelope is allowed as long as it comes closer to it, so a trunk can rise into a
    /// crown envelope that floats above the ground.
    #[must_use]
    pub fn blocks_step(&self, from: Vec3, to: Vec3) -> bool {
        if self.obstructed(to) {
            return true;
        }
        let Some(envelope) = &self.envelope else {
            return false;
        };
        if envelope.contains(to) {
            return false;
        }
        envelope.contains(from) || envelope.distance(to) >= envelope.distance(from)
    }

    /// Shade the occluders cast on `point`, for voxels of edge `cell` and the shadow
    /// `weights[q]` of one metamer at depth `q`.
    #[must_use]
    pub fn shade(&self, point: Vec3, cell: f32, weights: &[f32]) -> f32 {
        let mut shade = 0.0;
        for occluder in self.occluders.iter().filter(|o| o.shade > 0.0) {
            for (depth, weight) in weights.iter().enumerate() {
                let reach = depth as f32 * cell;
                if occluder.shape.near(point + Vec3::Y * reach, reach) {
                    shade += occluder.shade * weight;
                }
            }
        }
        shade
    }

    /// Checks every volume and strength.
    ///
    /// # Errors
    ///
    /// Returns a message naming the first volume without a positive, finite size, or the
    /// first shade that is negative or not finite.
    pub fn validate(&self) -> Result<(), String> {
        if let Some(envelope) = &self.envelope {
            envelope.validate()?;
        }
        for occluder in &self.occluders {
            occluder.shape.validate()?;
            if !(occluder.shade >= 0.0 && occluder.shade.is_finite()) {
                return Err(format!("shade {} is not a positive number", occluder.shade));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Occluder, Shape, Surroundings, VoxelMask};
    use glam::Vec3;
    use std::sync::Arc;

    fn shapes() -> Vec<Shape> {
        vec![
            Shape::Sphere {
                center: Vec3::new(0.0, 2.0, 0.0),
                radius: 1.5,
            },
            Shape::Ellipsoid {
                center: Vec3::new(0.0, 2.0, 0.0),
                radii: Vec3::new(1.0, 2.0, 1.5),
            },
            Shape::Box {
                min: Vec3::new(-1.0, 0.0, -2.0),
                max: Vec3::new(1.0, 3.0, 2.0),
            },
            Shape::Cylinder {
                base: Vec3::ZERO,
                radius: 1.0,
                height: 4.0,
            },
            Shape::Cone {
                base: Vec3::ZERO,
                radius: 2.0,
                height: 4.0,
            },
        ]
    }

    #[test]
    fn shapes_contain_their_centre_and_not_far_points() {
        for shape in shapes() {
            let bounds = shape.bounds();
            let inside = Vec3::new(0.0, 1.5, 0.0);
            assert!(shape.contains(inside), "{shape:?}");
            assert!(!shape.contains(bounds.max + Vec3::ONE), "{shape:?}");
            assert!(!shape.contains(bounds.min - Vec3::ONE), "{shape:?}");
            assert!(shape.near(bounds.max + Vec3::splat(0.1), 5.0));
            assert!(shape.distance(inside) < 0.0);
        }
    }

    #[test]
    fn cone_narrows_to_its_apex() {
        let cone = &shapes()[4];
        assert!(cone.contains(Vec3::new(1.9, 0.05, 0.0)));
        assert!(!cone.contains(Vec3::new(1.9, 1.0, 0.0)));
        assert!(cone.contains(Vec3::new(0.0, 3.9, 0.0)));
        assert!(!cone.contains(Vec3::new(0.0, 4.1, 0.0)));
        assert!(!cone.contains(Vec3::new(0.0, -0.1, 0.0)));
    }

    #[test]
    fn voxel_mask_matches_the_function_it_samples() {
        let ball = |p: Vec3| p.distance(Vec3::splat(1.0)) < 0.8;
        let mask = VoxelMask::from_fn(Vec3::ZERO, 0.1, [20, 20, 20], ball);
        assert!(mask.contains(Vec3::splat(1.0)));
        assert!(!mask.contains(Vec3::splat(0.05)));
        assert!(!mask.contains(Vec3::splat(-0.5)) && !mask.contains(Vec3::splat(2.5)));
        let filled = mask.filled().count();
        assert!(filled > 1900 && filled < 2400, "{filled}");
        assert!(mask.filled().all(|voxel| mask.get(voxel)));
        let shape = Shape::Voxels(Arc::new(mask));
        assert!(shape.contains(Vec3::new(1.0, 1.0, 1.5)));
        assert!(shape.near(Vec3::new(2.0, 1.0, 1.0), 0.4));
        assert!(!shape.near(Vec3::new(3.0, 1.0, 1.0), 0.4));
    }

    #[test]
    fn blocking_and_shade() {
        let wall = Shape::Box {
            min: Vec3::new(1.0, 0.0, -5.0),
            max: Vec3::new(1.5, 4.0, 5.0),
        };
        let surroundings = Surroundings {
            envelope: Some(Shape::Sphere {
                center: Vec3::ZERO,
                radius: 10.0,
            }),
            occluders: vec![Occluder::obstacle(wall)],
        };
        surroundings.validate().unwrap();
        assert!(surroundings.blocked(Vec3::new(1.2, 1.0, 0.0)));
        assert!(surroundings.blocked(Vec3::new(0.0, 11.0, 0.0)));
        assert!(!surroundings.blocked(Vec3::new(0.0, 1.0, 0.0)));
        let (inside, beyond) = (Vec3::new(0.0, 9.5, 0.0), Vec3::new(0.0, 10.5, 0.0));
        assert!(surroundings.blocks_step(inside, beyond));
        assert!(!surroundings.blocks_step(Vec3::new(0.0, 12.0, 0.0), beyond));
        assert!(surroundings.blocks_step(beyond, Vec3::new(0.0, 12.0, 0.0)));
        assert!(surroundings.blocks_step(Vec3::new(0.5, 1.0, 0.0), Vec3::new(1.2, 1.0, 0.0)));
        assert!(!surroundings.blocks_step(Vec3::ZERO, Vec3::Y));
        let weights = [0.2, 0.1, 0.05, 0.025];
        let beside = surroundings.shade(Vec3::new(0.5, 0.5, 0.0), 0.5, &weights);
        let far = surroundings.shade(Vec3::new(-6.0, 0.5, 0.0), 0.5, &weights);
        let above = surroundings.shade(Vec3::new(0.5, 6.0, 0.0), 0.5, &weights);
        assert!(beside > 0.0 && far == 0.0 && above == 0.0);
    }

    #[test]
    fn surroundings_parse_from_toml() {
        let text = r#"
            envelope = { shape = "ellipsoid", center = [0.0, 3.0, 0.0], radii = [2.0, 3.0, 2.0] }
            [[occluders]]
            shape = "box"
            min = [1.0, 0.0, -4.0]
            max = [1.3, 3.0, 4.0]
            [[occluders]]
            shape = "sphere"
            center = [-4.0, 5.0, 0.0]
            radius = 2.5
            blocks = false
            shade = 3.0
        "#;
        let surroundings: Surroundings = toml::from_str(text).unwrap();
        surroundings.validate().unwrap();
        assert_eq!(surroundings.occluders.len(), 2);
        assert!(surroundings.occluders[0].blocks && !surroundings.occluders[1].blocks);
        let empty = Shape::Sphere {
            center: Vec3::ZERO,
            radius: 0.0,
        };
        let bad = Surroundings {
            envelope: Some(empty),
            occluders: Vec::new(),
        };
        assert!(bad.validate().is_err());
    }
}

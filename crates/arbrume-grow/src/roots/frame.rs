//! The frame roots grow in: the frame of the plant turned upside down.
//!
//! The growth cycle was written for shoots: they rise from the ground, which stops a
//! shoot heading down into it. Turned upside down about a horizontal plane, a root
//! system is a plant of the same kind: its axes leave the collar "upwards" into the
//! soil, the gravitropic set-point counts from the downward vertical as it counts from
//! the upward one for a shoot, and the air above the ground is where the turned-over
//! plant could not dig. So the roots are grown by the same stages as the shoots, in the
//! frame [`Mirror`], and turned back afterwards.
//!
//! The plane is that of the highest point of the ground near the plant, so the turned
//! frame keeps the soil above its own ground. Everything the stages read is turned with
//! the plant: the obstacles become a [`Surroundings`] of the turned frame (analytic
//! shapes turned exactly, a cone or a voxel mask by voxels), and an uneven ground adds
//! the air above it as one more obstacle, a [`VoxelMask`].
//! [`SoilView`] answers the questions of the root stages in the turned frame: how far a
//! point is from anything that blocks a root, which way is free, how rich the soil is.

use super::soil::Soil;
use crate::surroundings::{Occluder, Shape, Surroundings, VoxelMask};
use glam::Vec3;
use std::sync::Arc;

/// The reflection `y -> top - y`, its own inverse.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Mirror {
    /// Twice the height of the plane of reflection.
    pub(crate) top: f32,
}

impl Mirror {
    /// `point` in the other frame.
    #[inline]
    pub(crate) fn point(self, point: Vec3) -> Vec3 {
        Vec3::new(point.x, self.top - point.y, point.z)
    }

    /// `direction` in the other frame.
    #[inline]
    pub(crate) fn direction(direction: Vec3) -> Vec3 {
        Vec3::new(direction.x, -direction.y, direction.z)
    }

    /// `shape` in the other frame. A cone, which has no apex-down form, and a voxel mask
    /// are turned voxel by voxel, with voxels of edge `cell` for the cone.
    pub(crate) fn shape(self, shape: &Shape, cell: f32) -> Shape {
        let top = self.top;
        match *shape {
            Shape::Sphere { center, radius } => Shape::Sphere {
                center: self.point(center),
                radius,
            },
            Shape::Ellipsoid { center, radii } => Shape::Ellipsoid {
                center: self.point(center),
                radii,
            },
            Shape::Box { min, max } => Shape::Box {
                min: Vec3::new(min.x, top - max.y, min.z),
                max: Vec3::new(max.x, top - min.y, max.z),
            },
            Shape::Cylinder {
                base,
                radius,
                height,
            } => Shape::Cylinder {
                base: Vec3::new(base.x, top - base.y - height, base.z),
                radius,
                height,
            },
            Shape::Cone { .. } => self.voxelized(shape, cell),
            Shape::Voxels(ref mask) => Shape::Voxels(Arc::new(self.flipped(mask))),
        }
    }

    /// `shape` turned and sampled on voxels of edge `cell`.
    fn voxelized(self, shape: &Shape, cell: f32) -> Shape {
        let bounds = shape.bounds();
        let min = Vec3::new(bounds.min.x, self.top - bounds.max.y, bounds.min.z);
        let size = ((bounds.max - bounds.min) / cell).ceil().max(Vec3::ONE);
        let size = [size.x as u32, size.y as u32, size.z as u32];
        let mask = VoxelMask::from_fn(min, cell, size, |p| shape.contains(self.point(p)));
        Shape::Voxels(Arc::new(mask))
    }

    /// `mask` turned: same voxels, rows taken in the reverse order along Y.
    fn flipped(self, mask: &VoxelMask) -> VoxelMask {
        let bounds = mask.bounds();
        let origin = Vec3::new(bounds.min.x, self.top - bounds.max.y, bounds.min.z);
        let [_, ny, _] = mask.size();
        let mut turned = VoxelMask::new(origin, mask.cell(), mask.size());
        for [x, y, z] in mask.filled() {
            turned.set([x, ny - 1 - y, z], true);
        }
        turned
    }
}

/// What the root stages read about the soil, in the turned frame.
#[derive(Debug, Clone)]
pub(crate) struct SoilView<'a> {
    /// The turned frame.
    pub(crate) mirror: Mirror,
    /// The soil, in the frame of the plant.
    pub(crate) soil: &'a Soil,
    /// The blocking occluders of the surroundings, in the frame of the plant.
    obstacles: Vec<&'a Shape>,
    /// The same obstacles in the turned frame, with the air above an uneven ground.
    pub(crate) turned: Surroundings,
}

impl<'a> SoilView<'a> {
    /// The view of `soil` and of the blocking occluders of `surroundings` for a root
    /// system reaching `reach` meters from the collar in every direction, turned voxels
    /// being of edge `cell`.
    pub(crate) fn new(
        soil: &'a Soil,
        surroundings: &'a Surroundings,
        reach: f32,
        cell: f32,
    ) -> Self {
        let mirror = Mirror {
            top: soil.ground.highest(reach).max(0.0),
        };
        let blocking = surroundings.occluders.iter().filter(|o| o.blocks);
        let obstacles: Vec<&Shape> = blocking.map(|o| &o.shape).collect();
        let mut occluders: Vec<Occluder> = obstacles
            .iter()
            .map(|shape| Occluder::obstacle(mirror.shape(shape, cell)))
            .collect();
        if !soil.ground.is_flat() {
            occluders.push(Occluder::obstacle(Self::air(soil, mirror, reach, cell)));
        }
        for occluder in &mut occluders {
            occluder.shade = 0.0;
        }
        Self {
            mirror,
            soil,
            obstacles,
            turned: Surroundings {
                envelope: None,
                occluders,
            },
        }
    }

    /// The air above an uneven ground, in the turned frame, over the region the roots
    /// can reach.
    fn air(soil: &Soil, mirror: Mirror, reach: f32, cell: f32) -> Shape {
        let min = Vec3::new(-reach, 0.0, -reach);
        let span = Vec3::new(2.0 * reach, mirror.top + reach, 2.0 * reach);
        let size = (span / cell).ceil().max(Vec3::ONE);
        let size = [size.x as u32, size.y as u32, size.z as u32];
        let mask = VoxelMask::from_fn(min, cell, size, |turned| {
            let point = mirror.point(turned);
            point.y > soil.ground.height(point.x, point.z)
        });
        Shape::Voxels(Arc::new(mask))
    }

    /// Distance in meters from `turned` (a point of the turned frame) to the nearest
    /// place a root may not enter: the air above the ground or an obstacle. Negative in
    /// such a place.
    pub(crate) fn clearance(&self, turned: Vec3) -> f32 {
        let point = self.mirror.point(turned);
        let depth = self.soil.ground.height(point.x, point.z) - point.y;
        self.obstacles
            .iter()
            .map(|shape| shape.distance(point))
            .fold(depth, f32::min)
    }

    /// Unit direction in which the clearance grows at `turned`, from central differences
    /// over `step` meters.
    pub(crate) fn free_direction(&self, turned: Vec3, step: f32) -> Vec3 {
        let slope = |axis: Vec3| {
            self.clearance(turned + axis * step) - self.clearance(turned - axis * step)
        };
        Vec3::new(slope(Vec3::X), slope(Vec3::Y), slope(Vec3::Z)).normalize_or_zero()
    }

    /// Resource of the soil at `turned`.
    pub(crate) fn richness(&self, turned: Vec3) -> f32 {
        self.soil.richness(self.mirror.point(turned))
    }

    /// Direction of steepest rise of the resource at `turned`, in the turned frame.
    pub(crate) fn resource_direction(&self, turned: Vec3, step: f32) -> Vec3 {
        Mirror::direction(self.soil.gradient(self.mirror.point(turned), step))
    }
}

#[cfg(test)]
mod tests {
    use super::{Mirror, SoilView};
    use crate::roots::soil::{Ground, Soil};
    use crate::surroundings::{Occluder, Shape, Surroundings};
    use glam::Vec3;

    #[test]
    fn turned_shapes_hold_the_turned_points() {
        let mirror = Mirror { top: 0.6 };
        let shapes = [
            Shape::Sphere {
                center: Vec3::new(1.0, -1.0, 0.0),
                radius: 0.5,
            },
            Shape::Box {
                min: Vec3::new(0.5, -2.0, -1.0),
                max: Vec3::new(1.5, -0.2, 1.0),
            },
            Shape::Cylinder {
                base: Vec3::new(1.0, -2.0, 0.0),
                radius: 0.6,
                height: 1.5,
            },
            Shape::Cone {
                base: Vec3::new(1.0, -2.0, 0.0),
                radius: 0.8,
                height: 1.6,
            },
        ];
        let probes = [Vec3::new(1.0, -1.0, 0.0), Vec3::new(3.0, -1.0, 0.0)];
        for shape in &shapes {
            let turned = mirror.shape(shape, 0.05);
            for probe in probes {
                let inside = shape.contains(probe);
                assert_eq!(turned.contains(mirror.point(probe)), inside, "{shape:?}");
            }
        }
    }

    #[test]
    fn clearance_is_negative_in_the_air_and_in_obstacles() {
        let soil = Soil {
            ground: Ground::Slope { rise: [0.1, 0.0] },
            resources: Vec::new(),
        };
        let rock = Shape::Sphere {
            center: Vec3::new(0.0, -1.0, 1.0),
            radius: 0.3,
        };
        let surroundings = Surroundings {
            envelope: None,
            occluders: vec![Occluder::obstacle(rock)],
        };
        let view = SoilView::new(&soil, &surroundings, 3.0, 0.1);
        let turned = |p: Vec3| view.mirror.point(p);
        assert!(view.clearance(turned(Vec3::new(0.0, 0.5, 0.0))) < 0.0);
        assert!(view.clearance(turned(Vec3::new(0.0, -1.0, 1.0))) < 0.0);
        assert!(view.clearance(turned(Vec3::new(0.0, -0.5, -1.0))) > 0.0);
        assert!(view.turned.blocked(turned(Vec3::new(2.0, 0.25, 0.0))));
        assert!(!view.turned.blocked(turned(Vec3::new(2.0, 0.1, 0.0))));
        let away = view.free_direction(turned(Vec3::new(0.0, -1.0, 0.65)), 0.02);
        assert!(Mirror::direction(away).z < -0.9, "{away}");
    }
}

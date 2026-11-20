//! The surfaces a climbing plant can hold: the blocking occluders of the surroundings
//! and, when the species allows it, the ground.
//!
//! A support is queried for the distance from a point to its surface and the outward
//! normal there, the gradient of that distance taken by central differences. The
//! surface of a voxel mask lies half a voxel from the centres of its filled voxels, so
//! half a voxel is taken off the distance [`Shape::distance`] gives for a mask.
//!
//! Supports whose bounding box is farther than the query range are skipped, so a scene
//! of many supports costs only those near the shoot.

use crate::math::UP;
use crate::surroundings::{Shape, Surroundings};
use arbrume_core::graph::Aabb;
use glam::Vec3;

/// Step of the central differences for analytic shapes, in meters.
const ANALYTIC_STEP: f32 = 1e-3;

/// One support volume.
#[derive(Debug, Clone)]
struct Support<'a> {
    shape: &'a Shape,
    bounds: Aabb,
    /// Distance from the surface given by [`Shape::distance`] to the real surface.
    inset: f32,
    /// Step of the central differences.
    step: f32,
}

impl Support<'_> {
    /// Signed distance from `point` to the surface, negative inside.
    fn distance(&self, point: Vec3) -> f32 {
        self.shape.distance(point) - self.inset
    }

    /// Distance from `point` to the bounding box of the support.
    fn box_distance(&self, point: Vec3) -> f32 {
        let outside = (self.bounds.min - point).max(point - self.bounds.max);
        outside.max(Vec3::ZERO).length()
    }

    /// Outward unit normal of the surface nearest `point`; `fallback` where the
    /// distance has no gradient.
    fn normal(&self, point: Vec3, fallback: Vec3) -> Vec3 {
        let h = self.step;
        let along = |axis: Vec3| self.distance(point + axis * h) - self.distance(point - axis * h);
        Vec3::new(along(Vec3::X), along(Vec3::Y), along(Vec3::Z))
            .try_normalize()
            .unwrap_or(fallback)
    }
}

/// Where a point stands with respect to one support.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Contact {
    /// Distance from the point to the surface, negative inside.
    pub distance: f32,
    /// Outward unit normal of the surface nearest the point.
    pub normal: Vec3,
    /// Index of the support, `None` for the ground.
    pub support: Option<usize>,
}

impl Contact {
    /// The point of the surface nearest `point`, lifted by `offset` along the normal.
    #[must_use]
    pub fn surface_point(&self, point: Vec3, offset: f32) -> Vec3 {
        point - self.normal * (self.distance - offset)
    }
}

/// The supports of one plant, in the frame of the surroundings.
#[derive(Debug, Clone, Default)]
pub struct Supports<'a> {
    supports: Vec<Support<'a>>,
    /// Height of the ground, when it is a support.
    ground: Option<f32>,
}

impl<'a> Supports<'a> {
    /// The blocking occluders of `surroundings`, and the ground at height `ground` when
    /// given.
    #[must_use]
    pub fn new(surroundings: &'a Surroundings, ground: Option<f32>) -> Self {
        let supports = surroundings
            .occluders
            .iter()
            .filter(|occluder| occluder.blocks)
            .map(|occluder| {
                let (inset, step) = match &occluder.shape {
                    Shape::Voxels(mask) => (0.5 * mask.cell(), 0.5 * mask.cell()),
                    _ => (0.0, ANALYTIC_STEP),
                };
                Support {
                    shape: &occluder.shape,
                    bounds: occluder.shape.bounds(),
                    inset,
                    step,
                }
            })
            .collect();
        Self { supports, ground }
    }

    /// Whether there is no support at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.supports.is_empty() && self.ground.is_none()
    }

    /// Height of the ground when it is a support.
    #[must_use]
    pub const fn ground(&self) -> Option<f32> {
        self.ground
    }

    /// The nearest support other than the ground whose surface is within `range` of
    /// `point` and that `accept` admits, with `point` itself.
    pub fn nearest(
        &self,
        point: Vec3,
        range: f32,
        mut accept: impl FnMut(&Contact) -> bool,
    ) -> Option<Contact> {
        let mut best: Option<Contact> = None;
        for (index, support) in self.supports.iter().enumerate() {
            if support.box_distance(point) > range {
                continue;
            }
            let distance = support.distance(point);
            let closer = best.is_none_or(|best| distance < best.distance);
            if distance > range || !closer {
                continue;
            }
            let contact = Contact {
                distance,
                normal: support.normal(point, UP),
                support: Some(index),
            };
            if accept(&contact) {
                best = Some(contact);
            }
        }
        best
    }

    /// The ground seen from `point`, when it is a support.
    #[must_use]
    pub fn ground_contact(&self, point: Vec3) -> Option<Contact> {
        self.ground.map(|height| Contact {
            distance: point.y - height,
            normal: UP,
            support: None,
        })
    }

    /// `point` seen from the support of `contact`: its distance and normal there.
    #[must_use]
    pub fn contact_with(&self, contact: &Contact, point: Vec3) -> Contact {
        match contact.support {
            Some(index) => {
                let support = &self.supports[index];
                Contact {
                    distance: support.distance(point),
                    normal: support.normal(point, contact.normal),
                    support: Some(index),
                }
            }
            None => self.ground_contact(point).unwrap_or(*contact),
        }
    }

    /// Whether the support of `contact`, seen from `point`, is thinner than
    /// `thickness` there: the point `thickness` beyond its surface along the normal
    /// lies outside it again. The ground is never thin.
    #[must_use]
    pub fn is_thin(&self, contact: &Contact, point: Vec3, thickness: f32) -> bool {
        contact.support.is_some_and(|index| {
            let probe = contact.surface_point(point, 0.0) - contact.normal * thickness;
            self.supports[index].distance(probe) > 0.0
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Supports;
    use crate::surroundings::{Occluder, Shape, Surroundings, VoxelMask};
    use glam::Vec3;
    use std::sync::Arc;

    fn wall() -> Shape {
        Shape::Box {
            min: Vec3::new(-2.0, 0.0, 0.3),
            max: Vec3::new(2.0, 3.0, 0.6),
        }
    }

    #[test]
    fn distance_and_normal_of_a_box() {
        let surroundings = Surroundings {
            envelope: None,
            occluders: vec![Occluder::obstacle(wall())],
            water: None,
        };
        let supports = Supports::new(&surroundings, Some(0.0));
        let point = Vec3::new(0.0, 1.0, 0.2);
        let contact = supports.nearest(point, 0.5, |_| true).unwrap();
        assert!((contact.distance - 0.1).abs() < 1e-5);
        assert!(contact.normal.distance(-Vec3::Z) < 1e-3);
        assert!(
            supports
                .nearest(Vec3::new(0.0, 1.0, -1.0), 0.5, |_| true)
                .is_none()
        );
        assert!(!supports.is_thin(&contact, point, 0.2));
        assert!(supports.is_thin(&contact, point, 0.4));
        let ground = supports.ground_contact(point).unwrap();
        assert!((ground.distance - 1.0).abs() < 1e-6 && ground.support.is_none());
    }

    #[test]
    fn voxel_surface_is_half_a_voxel_from_the_filled_centres() {
        let cell = 0.05;
        let mask = VoxelMask::from_fn(Vec3::new(-1.0, 0.0, 0.0), cell, [40, 40, 20], |p| {
            p.z > 0.3 && p.z < 0.6
        });
        let surroundings = Surroundings {
            envelope: None,
            occluders: vec![Occluder::obstacle(Shape::Voxels(Arc::new(mask)))],
            water: None,
        };
        let supports = Supports::new(&surroundings, None);
        let point = Vec3::new(0.01, 1.01, 0.2);
        let contact = supports.nearest(point, 0.5, |_| true).unwrap();
        assert!((contact.distance - 0.1).abs() < 0.2 * cell, "{contact:?}");
        assert!(contact.normal.distance(-Vec3::Z) < 0.05, "{contact:?}");
    }
}

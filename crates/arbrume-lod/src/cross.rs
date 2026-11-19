//! Crossed quads: `cross_planes` vertical planes through the vertical axis of the plant,
//! turned evenly over half a turn, each showing the whole plant projected along its
//! normal, and optionally one horizontal plane through the middle of its height.
//!
//! This is the impostor of most real-time engines. It reads well from the side at any
//! azimuth, since some plane always faces the viewer, but every plane shows the whole
//! crown, so a plant seen between two planes looks denser, and from above it shows its
//! planes edge on unless the horizontal plane is added.

use crate::{BillboardOptions, Bounds3, Plane};
use glam::Vec3;
use std::f32::consts::PI;

/// The planes of a cross billboard of `mesh`.
#[must_use]
pub fn planes(bounds: &Bounds3, options: &BillboardOptions) -> Vec<Plane> {
    let center = bounds.center();
    let axis = center;
    let count = options.cross_planes.max(1);
    let radius = bounds.radius();
    let mut planes: Vec<Plane> = (0..count)
        .map(|i| {
            let angle = PI * i as f32 / count as f32;
            let normal = Vec3::new(angle.sin(), 0.0, angle.cos());
            Plane::new(normal, axis, Vec::new(), radius)
        })
        .collect();
    if options.cross_horizontal {
        planes.push(Plane::new(Vec3::Y, axis, Vec::new(), radius));
    }
    planes
}

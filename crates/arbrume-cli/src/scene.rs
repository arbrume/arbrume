//! Geometry of the surroundings, to show obstacles next to a plant in a contact sheet.

use arbrume_core::mesh::{Material, MeshBuffers, MeshPart, PartKind, TreeMesh};
use arbrume_gen::{Shape, Surroundings};
use glam::Vec3;
use std::f32::consts::{PI, TAU};

/// Sides of a round shape.
const SIDES: u32 = 24;
/// Largest number of voxels drawn one by one; a larger mask is drawn as its box.
const MAX_VOXELS: usize = 20_000;

/// Appends the triangle `a, b, c` with a flat normal.
fn triangle(buffers: &mut MeshBuffers, [a, b, c]: [Vec3; 3]) {
    let normal = (b - a).cross(c - a).normalize_or_zero();
    let first = buffers.positions.len() as u32;
    for corner in [a, b, c] {
        buffers.positions.push(corner.to_array());
        buffers.normals.push(normal.to_array());
    }
    buffers.indices.extend([first, first + 1, first + 2]);
}

/// Appends the quad `a, b, c, d`.
fn quad(buffers: &mut MeshBuffers, [a, b, c, d]: [Vec3; 4]) {
    triangle(buffers, [a, b, c]);
    triangle(buffers, [a, c, d]);
}

/// Appends the box from `min` to `max`.
fn cuboid(buffers: &mut MeshBuffers, min: Vec3, max: Vec3) {
    let corner = |x: bool, y: bool, z: bool| {
        Vec3::new(
            if x { max.x } else { min.x },
            if y { max.y } else { min.y },
            if z { max.z } else { min.z },
        )
    };
    for axis in 0..3 {
        for high in [false, true] {
            let at = |u: bool, v: bool| match axis {
                0 => corner(high, u, v),
                1 => corner(v, high, u),
                _ => corner(u, v, high),
            };
            quad(
                buffers,
                [
                    at(false, false),
                    at(true, false),
                    at(true, true),
                    at(false, true),
                ],
            );
        }
    }
}

/// Appends the surface of revolution about the vertical through `center` whose profile
/// is `profile(t)` for `t` in `[0, 1]`: a radius and a height above `center`, scaled
/// horizontally by `stretch`.
fn revolution(
    buffers: &mut MeshBuffers,
    center: Vec3,
    stretch: Vec3,
    rings: u32,
    profile: impl Fn(f32) -> (f32, f32),
) {
    let point = |ring: u32, side: u32| {
        let (radius, height) = profile(ring as f32 / rings as f32);
        let angle = TAU * side as f32 / SIDES as f32;
        let offset = Vec3::new(radius * angle.cos(), height, radius * angle.sin());
        center + offset * stretch
    };
    for ring in 0..rings {
        for side in 0..SIDES {
            quad(
                buffers,
                [
                    point(ring, side),
                    point(ring + 1, side),
                    point(ring + 1, side + 1),
                    point(ring, side + 1),
                ],
            );
        }
    }
}

/// Appends the surface of `shape`.
fn surface(buffers: &mut MeshBuffers, shape: &Shape) {
    let ball = |t: f32| ((PI * t).sin(), -(PI * t).cos());
    match *shape {
        Shape::Sphere { center, radius } => {
            revolution(buffers, center, Vec3::splat(radius), SIDES / 2, ball);
        }
        Shape::Ellipsoid { center, radii } => revolution(buffers, center, radii, SIDES / 2, ball),
        Shape::Box { min, max } => cuboid(buffers, min, max),
        Shape::Cylinder {
            base,
            radius,
            height,
        } => {
            let wall = |t: f32| match t {
                t if t < 0.3 => (radius * t / 0.3, 0.0),
                t if t > 0.7 => (radius * (1.0 - t) / 0.3, height),
                t => (radius, height * (t - 0.3) / 0.4),
            };
            revolution(buffers, base, Vec3::ONE, 10, wall);
        }
        Shape::Cone {
            base,
            radius,
            height,
        } => {
            let wall = |t: f32| {
                if t < 0.5 {
                    (radius * 2.0 * t, 0.0)
                } else {
                    (radius * 2.0 * (1.0 - t), height * (2.0 * t - 1.0))
                }
            };
            revolution(buffers, base, Vec3::ONE, 2, wall);
        }
        Shape::Voxels(ref mask) => {
            let [nx, ny, nz] = mask.size();
            let bounds = mask.bounds();
            if nx as usize * ny as usize * nz as usize > MAX_VOXELS {
                cuboid(buffers, bounds.min, bounds.max);
                return;
            }
            for voxel in mask.filled() {
                let [x, y, z] = voxel.map(|v| v as f32);
                let min = bounds.min + Vec3::new(x, y, z) * mask.cell();
                cuboid(buffers, min, min + Vec3::splat(mask.cell()));
            }
        }
    }
}

/// Adds to `mesh` one part showing the blocking occluders of `surroundings`, when there
/// are any.
pub fn add_occluders(mesh: &mut TreeMesh, surroundings: &Surroundings) {
    let mut buffers = MeshBuffers::default();
    for occluder in surroundings.occluders.iter().filter(|o| o.blocks) {
        surface(&mut buffers, &occluder.shape);
    }
    if buffers.indices.is_empty() {
        return;
    }
    mesh.parts.push(MeshPart {
        kind: PartKind::Bark,
        material: Material {
            name: "surroundings".to_owned(),
            base_color: [0.55, 0.52, 0.5, 1.0],
            roughness: 0.9,
            double_sided: true,
            alpha_cutoff: None,
            base_color_texture: None,
            normal_texture: None,
            roughness_texture: None,
        },
        buffers,
    });
}

/// Appends `other`, moved by `offset`, to `mesh`: part by part where the materials
/// match, as new parts otherwise.
pub fn merge(mesh: &mut TreeMesh, other: &TreeMesh, offset: Vec3) {
    for part in &other.parts {
        let target = mesh
            .parts
            .iter_mut()
            .find(|candidate| candidate.kind == part.kind && candidate.material == part.material);
        let Some(target) = target else {
            let mut moved = part.clone();
            for position in &mut moved.buffers.positions {
                *position = (Vec3::from(*position) + offset).to_array();
            }
            mesh.parts.push(moved);
            continue;
        };
        let first = target.buffers.positions.len() as u32;
        let (into, from) = (&mut target.buffers, &part.buffers);
        let moved = from
            .positions
            .iter()
            .map(|p| (Vec3::from(*p) + offset).to_array());
        into.positions.extend(moved);
        into.normals.extend_from_slice(&from.normals);
        into.uvs.extend_from_slice(&from.uvs);
        into.colors.extend_from_slice(&from.colors);
        into.indices.extend(from.indices.iter().map(|i| i + first));
    }
}

//! Gas floats (pneumatocysts) at the base of the blades of a kelp.
//!
//! With `aquatic.blade_float > 0` every leaf template of a species without twigs gets a
//! small closed ellipsoid at the base of its blade: `blade_float` leaf lengths long
//! along the leaf, `FLOAT_WIDTH` of that across, centred on the petiole when the
//! petiole is long enough to hold it, otherwise just inside the base of the blade. It
//! is a UV sphere of [`RINGS`] rings of [`SIDES`] sides, sampled at the opaque stalk
//! texel of its atlas cell (`LeafAtlas::STEM_UV`), and tinted with `aquatic.float_color`
//! relative to the base colour of the leaf material (at most 1 per channel). Floats
//! follow their leaf, so they take its wind data and its flutter.

use crate::twig::{TemplateSet, flutter_of};
use arbrume_core::species::{LeafAtlas, Species};
use glam::Vec3;
use std::f32::consts::{PI, TAU};

/// Ratio of the width of a float to its length.
const FLOAT_WIDTH: f32 = 0.6;
/// Rings of latitude of a float, poles excluded.
const RINGS: u32 = 4;
/// Sides of a ring of a float.
const SIDES: u32 = 8;

/// Adds a float to every variant of `set`, for `species`. Does nothing for a species
/// without blade floats or with twigs.
pub(crate) fn add_blade_floats(set: &mut TemplateSet, species: &Species) {
    let Some(aquatic) = &species.aquatic else {
        return;
    };
    let foliage = &species.foliage;
    if aquatic.blade_float <= 0.0 || foliage.twig.enabled() {
        return;
    }
    let length = aquatic.blade_float;
    let petiole = foliage.leaf_layout().petiole;
    let center = Vec3::X * (petiole - 0.5 * length).max(0.5 * length);
    let radii = Vec3::new(
        0.5 * length,
        0.5 * FLOAT_WIDTH * length,
        0.5 * FLOAT_WIDTH * length,
    );
    let tint: [f32; 3] = std::array::from_fn(|channel| {
        let base = set.base_color[channel].max(f32::MIN_POSITIVE);
        (aquatic.float_color[channel] / base).min(1.0)
    });
    let atlas = foliage.atlas();
    for (cell, variant) in set.variants.iter_mut().enumerate() {
        let uv = atlas.map(cell as u32 % atlas.leaf_cells.max(1), LeafAtlas::STEM_UV);
        let first = variant.positions.len() as u32;
        for (position, normal) in ellipsoid(center, radii) {
            variant.positions.push(position);
            variant.normals.push(normal);
            variant.uvs.push(uv);
            variant.tints.push(tint);
        }
        variant
            .indices
            .extend(ellipsoid_indices().map(|index| first + index));
    }
    set.flutter = set.variants.iter().map(flutter_of).collect();
}

/// Points and unit normals of an ellipsoid of `radii` about `center`, its axis along
/// `x`: the pole at `-x`, the rings, then the pole at `+x`.
fn ellipsoid(center: Vec3, radii: Vec3) -> Vec<(Vec3, Vec3)> {
    let mut points = Vec::with_capacity((RINGS * SIDES + 2) as usize);
    let mut push = |unit: Vec3| {
        let normal = (unit / radii).normalize();
        points.push((center + unit * radii, normal));
    };
    push(-Vec3::X);
    for ring in 1..=RINGS {
        let polar = PI * ring as f32 / (RINGS + 1) as f32;
        for side in 0..SIDES {
            let azimuth = TAU * side as f32 / SIDES as f32;
            let (sin, cos) = polar.sin_cos();
            push(Vec3::new(-cos, sin * azimuth.cos(), sin * azimuth.sin()));
        }
    }
    push(Vec3::X);
    points
}

/// Triangles of [`ellipsoid`], counter-clockwise seen from outside.
fn ellipsoid_indices() -> impl Iterator<Item = u32> {
    let last = RINGS * SIDES + 1;
    let ring = |r: u32, s: u32| 1 + r * SIDES + s % SIDES;
    let caps = (0..SIDES).flat_map(move |s| {
        [
            0,
            ring(0, s + 1),
            ring(0, s),
            last,
            ring(RINGS - 1, s),
            ring(RINGS - 1, s + 1),
        ]
    });
    let bands = (0..RINGS - 1).flat_map(move |r| {
        (0..SIDES).flat_map(move |s| {
            let (a, b) = (ring(r, s), ring(r, s + 1));
            let (c, d) = (ring(r + 1, s), ring(r + 1, s + 1));
            [a, b, d, a, d, c]
        })
    });
    caps.chain(bands)
}

#[cfg(test)]
mod tests {
    use super::{RINGS, SIDES, ellipsoid, ellipsoid_indices};
    use glam::Vec3;

    #[test]
    fn the_float_is_closed_and_faces_out() {
        let center = Vec3::new(0.1, 0.0, 0.0);
        let points = ellipsoid(center, Vec3::new(0.05, 0.03, 0.03));
        let indices: Vec<u32> = ellipsoid_indices().collect();
        assert_eq!(points.len() as u32, RINGS * SIDES + 2);
        let mut edges = std::collections::HashMap::new();
        for triangle in indices.chunks(3) {
            let [a, b, c] = [0, 1, 2].map(|k| points[triangle[k] as usize].0);
            let outward = (a + b + c) / 3.0 - center;
            assert!((b - a).cross(c - a).dot(outward) > 0.0, "{triangle:?}");
            for k in 0..3 {
                let edge = (triangle[k], triangle[(k + 1) % 3]);
                *edges.entry(edge).or_insert(0) += 1;
            }
        }
        assert!(edges.keys().all(|&(a, b)| edges.get(&(b, a)) == Some(&1)));
    }
}

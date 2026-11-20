//! Floating pads: round leaves with a notch, centred on their petiole.
//!
//! A pad (`BladeShape::Pad`) is a flat disc of diameter one leaf length (times
//! `width_ratio` across), centred on the organ site where the petiole meets it, with a
//! narrow notch from the centre to the edge on the side of the base of the leaf: the
//! sinus of a water lily leaf. It is a fan of triangles from the centre to a ring of
//! vertices that skips the notch. The ring has the side count that keeps the polygon
//! within the tolerance of its circle. Texture coordinates are those of the rectangle
//! of the leaf (`LeafLayout`) with the disc in its middle, so a leaf texture drawn for
//! the same outline fits. A pad is never meshed as a card: the fan follows the outline
//! itself, notch included.

use crate::leaf::LeafTemplate;
use arbrume_core::species::{Foliage, PAD_NOTCH_HALF_ANGLE as NOTCH_HALF_ANGLE};
use glam::{Vec2, Vec3};
use std::f32::consts::{PI, TAU};

/// Fewest and most sides of the ring of a pad.
const SIDES: (u32, u32) = (10, 48);

/// The template of the pads of `foliage` at `tolerance`.
pub(crate) fn template(foliage: &Foliage, tolerance: f32) -> LeafTemplate {
    let layout = foliage.leaf_layout();
    let radius = 0.5 * foliage.length * foliage.width_ratio.max(1.0);
    let ideal = (PI * (radius / (2.0 * tolerance.max(f32::MIN_POSITIVE))).sqrt()).ceil();
    let sides = (ideal as u32).clamp(SIDES.0, SIDES.1);
    let uv = |flat: Vec2| {
        [
            0.5 + 0.5 * flat.y / layout.half_width,
            (flat.x + 0.5) / (layout.petiole + 1.0),
        ]
    };
    let mut template = LeafTemplate::default();
    template.positions.push(Vec3::ZERO);
    template.uvs.push(uv(Vec2::ZERO));
    let span = TAU - 2.0 * NOTCH_HALF_ANGLE;
    for index in 0..=sides {
        let angle = NOTCH_HALF_ANGLE + span * index as f32 / sides as f32;
        let flat = Vec2::new(-0.5 * angle.cos(), 0.5 * foliage.width_ratio * angle.sin());
        template.positions.push(flat.extend(0.0));
        template.uvs.push(uv(flat));
    }
    template.normals = vec![Vec3::Z; template.positions.len()];
    for index in 1..=sides {
        template.indices.extend([0, index + 1, index]);
    }
    template.tints = vec![[1.0; 3]; template.positions.len()];
    template
}

#[cfg(test)]
mod tests {
    use super::template;
    use arbrume_core::species::{BladeShape, Foliage, PAD_NOTCH_HALF_ANGLE as NOTCH_HALF_ANGLE};
    use std::f32::consts::PI;

    #[test]
    fn a_pad_is_a_notched_disc_facing_up() {
        let foliage = Foliage {
            shape: BladeShape::Pad,
            width_ratio: 1.0,
            petiole_length: 0.0,
            length: 0.3,
            ..Foliage::default()
        };
        let pad = template(&foliage, 0.002);
        let area = pad.area();
        let disc = PI * 0.25 * (1.0 - NOTCH_HALF_ANGLE / PI);
        assert!((area - disc).abs() < 0.02 * disc, "{area} against {disc}");
        for triangle in pad.indices.chunks(3) {
            let [a, b, c] = [0, 1, 2].map(|k| pad.positions[triangle[k] as usize]);
            assert!((b - a).cross(c - a).z > 0.0);
        }
        assert!(
            pad.uvs
                .iter()
                .flatten()
                .all(|c| (-1e-4..=1.0001).contains(c))
        );
    }
}

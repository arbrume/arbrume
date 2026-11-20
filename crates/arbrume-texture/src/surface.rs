//! Steps shared by the bark and leaf synthesizers: normals from a height field and
//! calibration of the mean colour.

use glam::Vec3;
use rayon::prelude::*;

/// Tangent-space normals of the height field `height` (row-major, `width` texels per
/// row, the first row at the top), with `+x` towards increasing column (increasing `u`)
/// and `+y` towards the top of the image (decreasing `v`), the glTF convention. `spacing` is the size of a texel along each axis and `heights` the factor that
/// turns a height value into the same unit. With `wrap` the field tiles; otherwise the
/// edges are clamped.
pub(crate) fn height_normals(
    height: &[f32],
    width: usize,
    spacing: [f32; 2],
    heights: f32,
    wrap: bool,
) -> Vec<Vec3> {
    let rows = height.len() / width;
    let at = |x: isize, y: isize| {
        let (x, y) = if wrap {
            (
                x.rem_euclid(width.cast_signed()) as usize,
                y.rem_euclid(rows.cast_signed()) as usize,
            )
        } else {
            (
                x.clamp(0, width.cast_signed() - 1) as usize,
                y.clamp(0, rows.cast_signed() - 1) as usize,
            )
        };
        height[y * width + x]
    };
    let mut normals = vec![Vec3::Z; height.len()];
    normals
        .par_chunks_mut(width)
        .enumerate()
        .for_each(|(y, row)| {
            let y = y.cast_signed();
            for (x, normal) in row.iter_mut().enumerate() {
                let x = x.cast_signed();
                let dx = (at(x + 1, y) - at(x - 1, y)) * heights / (2.0 * spacing[0]);
                let dy = (at(x, y + 1) - at(x, y - 1)) * heights / (2.0 * spacing[1]);
                *normal = Vec3::new(-dx, dy, 1.0).normalize();
            }
        });
    normals
}

/// Divides each channel of `colors` by its mean, weighted by `weights` when given:
/// the colours become ratios to their mean.
pub(crate) fn to_ratios(colors: &mut [Vec3], weights: Option<&[f32]>) {
    let (mut sum, mut total) = (glam::DVec3::ZERO, 0.0_f64);
    for (index, color) in colors.iter().enumerate() {
        let weight = f64::from(weights.map_or(1.0, |w| w[index]));
        sum += color.as_dvec3() * weight;
        total += weight;
    }
    if total <= 0.0 {
        return;
    }
    let mean = (sum / total).as_vec3().max(Vec3::splat(1e-6));
    for color in colors.iter_mut() {
        *color /= mean;
    }
}

/// Fraction of texels allowed to clip at 1 when ratios are scaled by [`level`].
const CLIPPED: f32 = 0.002;

/// Scales ratios to their mean (see [`to_ratios`]) by the largest level per channel,
/// at most 1, that leaves no more than [`CLIPPED`] of the texels whose weight is at
/// least one half above 1; clamps to `[0, 1]` and returns the level, which is the mean
/// colour of the result (but for the clipped texels).
pub(crate) fn level(groups: &mut [(&mut [Vec3], Option<&[f32]>)]) -> Vec3 {
    let mut channels: [Vec<f32>; 3] = Default::default();
    for (colors, weights) in groups.iter() {
        for (index, color) in colors.iter().enumerate() {
            if weights.is_none_or(|w| w[index] >= 0.5) {
                for (channel, values) in channels.iter_mut().enumerate() {
                    values.push(color[channel]);
                }
            }
        }
    }
    let level = Vec3::from_array(channels.map(|mut values| {
        if values.is_empty() {
            return 1.0;
        }
        values.sort_by(f32::total_cmp);
        let at = ((1.0 - CLIPPED) * (values.len() - 1) as f32).ceil() as usize;
        (1.0 / values[at].max(1e-6)).min(1.0)
    }));
    for (colors, _) in groups.iter_mut() {
        for color in colors.iter_mut() {
            *color = (*color * level).clamp(Vec3::ZERO, Vec3::ONE);
        }
    }
    level
}

#[cfg(test)]
mod tests {
    use super::{height_normals, level, to_ratios};
    use glam::Vec3;

    #[test]
    fn a_slope_tilts_the_normals_against_it() {
        let height: Vec<f32> = (0..16).map(|i| (i % 4) as f32).collect();
        let normals = height_normals(&height, 4, [1.0, 1.0], 1.0, false);
        assert!(normals[5].x < -0.5 && normals[5].y.abs() < 1e-6);
        let rows: Vec<f32> = (0..16).map(|i| (i / 4) as f32).collect();
        let normals = height_normals(&rows, 4, [1.0, 1.0], 1.0, false);
        assert!(
            normals[5].y > 0.5,
            "a field rising downwards tilts the normal up the image"
        );
    }

    #[test]
    fn levelled_ratios_have_the_level_as_mean_and_stay_below_one() {
        let mut colors = vec![Vec3::new(0.2, 0.4, 0.1), Vec3::new(0.4, 0.2, 0.3)];
        to_ratios(&mut colors, None);
        let mean = (colors[0] + colors[1]) * 0.5;
        assert!((mean - Vec3::ONE).length() < 1e-5);
        let level = level(&mut [(&mut colors, None)]);
        assert!(colors.iter().all(|c| c.max_element() <= 1.0));
        assert!(
            (level - Vec3::new(0.75, 0.75, 0.25 / 0.375)).length() < 1e-5,
            "{level}"
        );
    }
}

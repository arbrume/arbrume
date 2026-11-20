//! The leaf atlas of a species: its cells drawn, calibrated and assembled.
//!
//! Cells are drawn in parallel, each into its inner rectangle of
//! `LeafAtlas::cell_rect` rounded to texels. The colours of the leaf cells, together,
//! and of the spray cell, alone, are divided by their mean over opaque texels, then all
//! scaled by one level (`surface::level`): the base colour factor of the material, its
//! colour divided by that level, gives both the mean colour of the species, as the
//! tints in the vertex colours expect. Transparent texels take the mean colour of the
//! leaves, so that filtering does not darken the edges, and a flat normal.

use crate::asset::LeafImage;
use crate::image::Image;
use crate::leaf::{CellImage, Painter};
use crate::spray::spray_cell;
use crate::surface::level;
use arbrume_core::species::{Bark, Foliage, LeafAtlas};
use glam::Vec3;
use rayon::prelude::*;

/// The two maps of a leaf atlas.
#[derive(Debug, Clone, PartialEq)]
pub struct LeafMaps {
    /// sRGB albedo with the coverage in alpha, RGBA.
    pub albedo: Image,
    /// Tangent-space normals, RGB, `+y` towards decreasing `v` (up the image, from the
    /// tip of the leaf to its base).
    pub normal: Image,
    /// Where the cells are.
    pub atlas: LeafAtlas,
    /// Mean linear colour of the opaque texels (see the module documentation).
    pub mean: [f32; 3],
}

/// Texel rectangle `[x0, y0, x1, y1)` of the inside of `cell`.
fn texels(atlas: &LeafAtlas, cell: u32, resolution: usize) -> [usize; 4] {
    let rect = atlas.cell_rect(cell);
    let edge = |value: f32| (value * resolution as f32).round() as usize;
    let [x0, y0] = [edge(rect[0]), edge(rect[1])];
    [x0, y0, edge(rect[2]).max(x0 + 1), edge(rect[3]).max(y0 + 1)]
}

/// Mean colour of `cells` weighted by their alpha.
fn mean(cells: &[&CellImage]) -> Vec3 {
    let (mut sum, mut total) = (glam::DVec3::ZERO, 0.0_f64);
    for cell in cells {
        for (color, &alpha) in cell.colors.iter().zip(&cell.alpha) {
            sum += color.as_dvec3() * f64::from(alpha);
            total += f64::from(alpha);
        }
    }
    if total > 0.0 {
        (sum / total).as_vec3()
    } else {
        Vec3::ONE
    }
}

/// Divides the colours of `cells` by their common mean.
fn to_ratios(cells: &mut [CellImage]) {
    let current = mean(&cells.iter().collect::<Vec<_>>()).max(Vec3::splat(1e-6));
    for cell in cells {
        for color in &mut cell.colors {
            *color /= current;
        }
    }
}

/// Synthesizes the leaf atlas of `foliage`, `resolution` texels square, from `seed`;
/// `bark` gives the colour of the twig stems.
#[must_use]
pub fn leaf_maps(foliage: &Foliage, bark: &Bark, resolution: u32, seed: u64) -> LeafMaps {
    leaf_maps_with(foliage, bark, resolution, seed, None)
}

/// [`leaf_maps`], with the leaf cells filled from `image` when given.
pub(crate) fn leaf_maps_with(
    foliage: &Foliage,
    bark: &Bark,
    resolution: u32,
    seed: u64,
    image: Option<LeafImage<'_>>,
) -> LeafMaps {
    let atlas = foliage.atlas();
    let side = resolution as usize;
    let spray = atlas.spray_cell();
    let cells = atlas.leaf_cells + u32::from(spray.is_some());
    let mut images: Vec<CellImage> = (0..cells)
        .into_par_iter()
        .map(|cell| {
            let [x0, y0, x1, y1] = texels(&atlas, cell, side);
            let size = [x1 - x0, y1 - y0];
            if Some(cell) == spray {
                spray_cell(foliage, bark, size, seed ^ 0x5EED)
            } else if let Some(image) = &image {
                image.cell(size)
            } else {
                Painter::new(foliage, size, seed).cell(cell)
            }
        })
        .collect();
    let (leaves, rest) = images.split_at_mut(atlas.leaf_cells as usize);
    to_ratios(leaves);
    to_ratios(rest);
    let mut groups: Vec<_> = images
        .iter_mut()
        .map(|cell| (cell.colors.as_mut_slice(), Some(cell.alpha.as_slice())))
        .collect();
    let level = level(&mut groups);
    let fill = mean(
        &images[..atlas.leaf_cells as usize]
            .iter()
            .collect::<Vec<_>>(),
    );
    let mut colors = vec![fill; side * side];
    let mut alpha = vec![0.0_f32; side * side];
    let mut normals = vec![Vec3::Z; side * side];
    for (cell, image) in images.iter().enumerate() {
        let [x0, y0, x1, _] = texels(&atlas, cell as u32, side);
        let width = x1 - x0;
        let rows = image
            .colors
            .chunks_exact(width)
            .zip(image.alpha.chunks_exact(width))
            .zip(image.normals.chunks_exact(width));
        for (((colors_in, alpha_in), normals_in), y) in rows.zip(y0..) {
            let at = y * side + x0;
            for (x, &a) in alpha_in.iter().enumerate() {
                if a > 0.0 {
                    colors[at + x] = colors_in[x];
                    normals[at + x] = normals_in[x];
                }
                alpha[at + x] = a;
            }
        }
    }
    LeafMaps {
        albedo: Image::albedo(resolution, &colors, Some(&alpha)),
        normal: Image::normals(resolution, &normals),
        atlas,
        mean: level.to_array(),
    }
}

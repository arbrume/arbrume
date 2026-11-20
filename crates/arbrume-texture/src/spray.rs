//! The needle spray of a conifer twig, drawn on its fins.
//!
//! A fin spans `[-fin_width, fin_width]` across the twig (`u` from 0 to 1) and the length
//! of the twig along it (`v` from 0 to 1). The cell shows the twig stem along its middle
//! and, on both sides, needles leaving it at the inclination of the twig leaves, spaced
//! by about five times their width, with jittered angle and length, as long as the needles of the
//! species but no longer than the fin is wide, about one needle in seven left out so
//! that the alpha cut shows the gaps of a real spray. Needles are drawn from the base of the
//! twig to its tip, each over the previous ones, rounded across their width, brighter
//! towards their tip and darker near the stem, where a real spray is self-shadowed.
//! Everything is measured in meters so that the texture keeps the proportions of the
//! twig whatever the shape of its cell.

use crate::leaf::CellImage;
use crate::noise::{random, smoothstep};
use crate::surface::height_normals;
use arbrume_core::species::{Bark, Foliage};
use glam::{Vec2, Vec3};

/// One needle: base, tip and half-width in meters, and a brightness factor.
struct Needle {
    base: Vec2,
    tip: Vec2,
    half: f32,
    shade: f32,
}

/// The needles of a spray, from the base of the twig to its tip.
fn needles(foliage: &Foliage, texel: f32, seed: u64) -> Vec<Needle> {
    let twig = &foliage.twig;
    let half_width = foliage.length * foliage.width_ratio * 0.5;
    let half = half_width.max(0.75 * texel);
    let spacing = (5.0 * half).max(twig.length / 120.0);
    let per_side = ((twig.length / spacing) as u32).clamp(4, 160);
    let angle = twig.inclination_deg.to_radians().clamp(0.15, 1.45);
    let mut out = Vec::with_capacity(2 * per_side as usize);
    for i in 0..per_side {
        for (side_index, side) in [1.0_f32, -1.0].into_iter().enumerate() {
            let jitter = |salt: u64| random(seed, i, side_index as u32, salt) - 0.5;
            if jitter(0xC6) < -0.35 {
                continue;
            }
            let along = twig.length * (i as f32 + 0.5 + 0.8 * jitter(0xC1)) / per_side as f32;
            let turn = angle + 0.25 * jitter(0xC2);
            let direction = Vec2::new(turn.cos(), side * turn.sin());
            let reach = twig.fin_width / turn.sin().max(0.2);
            let mut length = foliage.length * (1.0 + 0.3 * jitter(0xC3));
            if twig.radial {
                length *= 0.55 + 0.45 * random(seed, i, side_index as u32, 0xC4);
            }
            let length = length.min(0.97 * reach);
            let base = Vec2::new(along, 0.0);
            out.push(Needle {
                base,
                tip: base + direction * length,
                half,
                shade: 0.85 + 0.3 * random(seed, i, side_index as u32, 0xC5),
            });
        }
    }
    out
}

/// The spray cell, `size` texels, of the twig of `foliage`.
pub(crate) fn spray_cell(foliage: &Foliage, bark: &Bark, size: [usize; 2], seed: u64) -> CellImage {
    let twig = &foliage.twig;
    let (width, height) = (size[0], size[1]);
    let texel = Vec2::new(
        2.0 * twig.fin_width / width as f32,
        twig.length / height as f32,
    );
    let point = |x: usize, y: usize| {
        Vec2::new(
            (y as f32 + 0.5) * texel.y,
            ((x as f32 + 0.5) / width as f32 * 2.0 - 1.0) * twig.fin_width,
        )
    };
    let count = width * height;
    let leaf = Vec3::from(foliage.color);
    let stem_color = Vec3::from(bark.color_at(0.0));
    let mut image = CellImage {
        width,
        colors: vec![leaf; count],
        alpha: vec![0.0; count],
        normals: Vec::new(),
    };
    let mut heights = vec![0.0_f32; count];
    let aa = texel.max_element();
    let stem_half = twig.stem_radius.max(0.8 * aa);
    for y in 0..height {
        for x in 0..width {
            let p = point(x, y);
            let distance = p.y.abs();
            let coverage = (0.5 + (stem_half - distance) / aa).clamp(0.0, 1.0);
            if coverage > 0.0 {
                let index = y * width + x;
                image.alpha[index] = coverage;
                image.colors[index] = stem_color;
                heights[index] = 0.5 * (1.0 - (distance / stem_half).min(1.0));
            }
        }
    }
    for needle in needles(foliage, aa, seed) {
        draw(
            &mut image,
            &mut heights,
            &needle,
            (texel, twig.fin_width),
            leaf,
        );
    }
    let amount = foliage.length * foliage.width_ratio * 0.5;
    image.normals = height_normals(&heights, width, [texel.x, texel.y], amount, false);
    image
}

/// Draws `needle` over the cell; `frame` is the texel size and the fin half-width.
fn draw(
    image: &mut CellImage,
    heights: &mut [f32],
    needle: &Needle,
    frame: (Vec2, f32),
    leaf: Vec3,
) {
    let (texel, fin) = frame;
    let width = image.width;
    let rows = image.alpha.len() / width;
    let aa = texel.max_element();
    let to_texel = |p: Vec2| Vec2::new((p.y + fin) / texel.x, p.x / texel.y);
    let (a, b) = (to_texel(needle.base), to_texel(needle.tip));
    let pad = Vec2::new(needle.half / texel.x, needle.half / texel.y) + 2.0;
    let low = (a.min(b) - pad).floor().max(Vec2::ZERO);
    let high = (a.max(b) + pad)
        .ceil()
        .min(Vec2::new(width as f32, rows as f32));
    let segment = needle.tip - needle.base;
    let length = segment.length_squared().max(1e-12);
    for y in low.y as usize..high.y as usize {
        for x in low.x as usize..high.x as usize {
            let p = Vec2::new((y as f32 + 0.5) * texel.y, (x as f32 + 0.5) * texel.x - fin);
            let f = ((p - needle.base).dot(segment) / length).clamp(0.0, 1.0);
            let half = needle.half * (1.0 - 0.6 * smoothstep(0.7, 1.0, f));
            let distance = p.distance(needle.base + segment * f);
            let coverage = (0.5 + (half - distance) / aa).clamp(0.0, 1.0);
            if coverage <= 0.0 {
                continue;
            }
            let index = y * width + x;
            let round = (1.0 - (distance / half).min(1.0).powi(2)).sqrt();
            let depth = 0.7 + 0.3 * (p.y.abs() / fin).min(1.0);
            let color = leaf * needle.shade * depth * (0.8 + 0.2 * round + 0.15 * f);
            image.colors[index] = image.colors[index].lerp(color, coverage);
            image.alpha[index] = image.alpha[index].max(coverage);
            heights[index] = heights[index].max(coverage * (0.4 + 0.6 * round));
        }
    }
}

//! Twig templates: what is instanced at an organ site.
//!
//! A [`TemplateSet`] holds the variants of the foliage template of one mesh. Without a
//! twig (`foliage.twig.leaves = 0`) it is the single leaf of the species. With a twig,
//! each variant is a thin stem carrying several copies of that leaf, baked into one
//! template, so that instancing a twig costs one transform per vertex, as a leaf does.
//!
//! **Frame and unit.** Like a leaf, a twig lives in the frame of an organ site (`x`
//! along the twig, `z` towards the upper side of the site) and its unit is the leaf
//! length. The stem follows a circular arc of `twig.length` that bends by
//! `twig.bend_deg` towards `-z`.
//!
//! **Leaves.** The lateral leaves sit on `ceil(leaves / leaves_per_node)` nodes evenly
//! spaced along the stem, turned by `twig.divergence_deg` from node to node, and leave
//! the stem at `twig.inclination_deg`; the first leaf points to the side of the site,
//! or to a random azimuth with `twig.radial`. A terminal leaf continues the stem. With
//! `twig.radial` the plane of a leaf contains the stem; otherwise its upper side faces
//! the upper side of the site. Each leaf of a variant draws a size, a small rotation
//! and a shade from the amounts of the species (`size_variation`,
//! `orientation_variation_deg`, `brightness_variation`), which is what makes the
//! variants differ.
//!
//! **Stem.** A three-sided pyramid from the base to the tip, in two segments when the
//! twig bends. It belongs to the leaf part of the mesh and takes the colour of young
//! bark through the vertex colours: the base colour of the leaf material is, per
//! channel, the larger of the leaf and stem colours, and every vertex carries the
//! ratio of its own colour to that base as its tint.
//!
//! **Fins.** `twig.fins` flat strips along the whole stem, evenly turned around it:
//! `twig.fin_width` wide on each side at the base and a little narrower at the tip,
//! darkened by `twig.fin_shade`, two triangles each (four when the twig bends). The fins
//! of successive nodes join into a ribbon, which gives a needle-clad shoot its bulk.
//!
//! Every variant has the same number of vertices and indices, so instances keep a fixed
//! size in the buffers. The variants depend on the species, the tolerance and the mesh
//! seed only.

use crate::leaf::{LeafTemplate, midrib};
use arbrume_core::rng::{Rng, Stream};
use arbrume_core::species::{Bark, Foliage};
use glam::Vec3;
use std::f32::consts::TAU;

/// Bend in radians above which the stem gets a second segment.
const STEM_BEND: f32 = 0.6;

/// Fraction by which a fin narrows from the base of the stem to its tip.
const FIN_TAPER: f32 = 0.25;

/// The variants of the foliage template of one mesh.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct TemplateSet {
    /// The variants, all with the same vertex and index counts. Never empty once built.
    pub variants: Vec<LeafTemplate>,
    /// Base colour of the leaf material.
    pub base_color: [f32; 3],
    /// Number of leading vertices of each variant that belong to the twig stem; 0
    /// without a twig or without a stem.
    pub stem_vertices: usize,
    /// Per variant and vertex, the distance of the vertex from the attachment point of
    /// the instance divided by the largest such distance of the variant, in `[0, 1]`.
    pub flutter: Vec<Vec<f32>>,
}

/// Frame of one leaf on a twig.
#[derive(Debug, Clone, Copy)]
struct LeafFrame {
    origin: Vec3,
    forward: Vec3,
    up: Vec3,
    scale: f32,
    tint: [f32; 3],
}

/// Builds one variant.
struct TwigBuilder<'a> {
    foliage: &'a Foliage,
    leaf: &'a LeafTemplate,
    /// Twig length in leaf lengths.
    length: f32,
    /// Bend of the stem in radians.
    bend: f32,
    /// Tint of a leaf of full brightness.
    leaf_tint: [f32; 3],
    /// Tint of the stem.
    stem_tint: [f32; 3],
    /// Factor on the size of every leaf, above 1 when the twig keeps fewer leaves.
    leaf_scale: f32,
    /// Leaves on the twig, at most `twig.leaves`.
    leaves: u32,
    out: LeafTemplate,
}

impl TwigBuilder<'_> {
    /// Point, tangent and upper normal of the stem at the relative position `t`.
    fn stem_frame(&self, t: f32) -> (Vec3, Vec3, Vec3) {
        let (point, up) = midrib(t, self.bend);
        let tangent = Vec3::Y.cross(up);
        (point * self.length, tangent, up)
    }

    /// Appends the species leaf in `frame`.
    fn add_leaf(&mut self, frame: &LeafFrame) {
        let across = frame.up.cross(frame.forward);
        let turn = |v: Vec3| frame.forward * v.x + across * v.y + frame.up * v.z;
        let base = self.out.positions.len() as u32;
        let (leaf, out) = (self.leaf, &mut self.out);
        let positions = leaf.positions.iter();
        out.positions
            .extend(positions.map(|&p| frame.origin + turn(p) * frame.scale));
        out.normals.extend(leaf.normals.iter().map(|&n| turn(n)));
        out.uvs.extend_from_slice(&leaf.uvs);
        out.tints
            .extend(std::iter::repeat_n(frame.tint, leaf.positions.len()));
        out.indices.extend(leaf.indices.iter().map(|i| base + i));
    }

    /// Frame of a leaf leaving the stem at `t` towards the azimuth `azimuth`, with the
    /// random size, attitude and shade of one leaf drawn from `rng`.
    fn leaf_frame(&self, t: f32, azimuth: Option<f32>, rng: &mut Rng) -> LeafFrame {
        let foliage = self.foliage;
        let twig = &foliage.twig;
        let (origin, tangent, up) = self.stem_frame(t);
        let side = up.cross(tangent);
        let (mut forward, mut normal) = (tangent, up);
        if let Some(azimuth) = azimuth {
            let (sin, cos) = azimuth.sin_cos();
            let radial = side * cos + up * sin;
            let (incline_sin, incline_cos) = twig.inclination_deg.to_radians().sin_cos();
            forward = tangent * incline_cos + radial * incline_sin;
            if twig.radial {
                normal = radial.cross(tangent);
            }
        }
        let turn = foliage.orientation_variation_deg.to_radians().tan();
        let across = normal.cross(forward).try_normalize().unwrap_or(side);
        let lift = forward.cross(across);
        let forward =
            (forward + across * (turn * rng.signed()) + lift * (turn * rng.signed())).normalize();
        let up = (lift + across * (turn * rng.signed()))
            .reject_from_normalized(forward)
            .try_normalize()
            .unwrap_or(lift);
        let shade = (0.5 * foliage.brightness_variation).mul_add(-rng.f32(), 1.0);
        LeafFrame {
            origin,
            forward,
            up,
            scale: self.leaf_scale * foliage.size_variation.mul_add(rng.signed(), 1.0),
            tint: self.leaf_tint.map(|channel| channel * shade),
        }
    }

    /// Adds the lateral leaves and the terminal leaf.
    fn leaves(&mut self, rng: &mut Rng) {
        let twig = self.foliage.twig;
        let terminal = u32::from(twig.terminal_leaf).min(self.leaves);
        let laterals = self.leaves - terminal;
        let per_node = u32::from(twig.leaves_per_node);
        let nodes = laterals.div_ceil(per_node).max(1);
        let phase = if twig.radial { rng.angle() } else { 0.0 };
        self.fins(phase);
        let span = if terminal > 0 { 0.92 } else { 1.0 };
        for index in 0..laterals {
            let (node, slot) = (index / per_node, index % per_node);
            let t = span * (node as f32 + 0.5) / nodes as f32;
            let azimuth = phase
                + node as f32 * twig.divergence_deg.to_radians()
                + TAU * slot as f32 / per_node as f32;
            let frame = self.leaf_frame(t, Some(azimuth), rng);
            self.add_leaf(&frame);
        }
        if terminal > 0 {
            let frame = self.leaf_frame(1.0, None, rng);
            self.add_leaf(&frame);
        }
    }

    /// Adds the stem: rings of three vertices and an apex at the tip.
    fn stem(&mut self) {
        let radius = self.foliage.twig.stem_radius / self.foliage.length;
        if radius <= 0.0 {
            return;
        }
        let rings: u32 = if self.bend.abs() > STEM_BEND { 2 } else { 1 };
        let base = self.out.positions.len() as u32;
        for ring in 0..rings {
            let t = ring as f32 / rings as f32;
            let (center, tangent, up) = self.stem_frame(t);
            let side = up.cross(tangent);
            for corner in 0..3 {
                let (sin, cos) = (TAU * corner as f32 / 3.0).sin_cos();
                let radial = side * cos + up * sin;
                self.vertex(center + radial * (radius * (1.0 - 0.6 * t)), radial);
            }
        }
        let (tip, tangent, _) = self.stem_frame(1.0);
        let apex = self.vertex(tip, tangent);
        for ring in 0..rings {
            let (low, high) = (base + 3 * ring, base + 3 * ring + 3);
            for corner in 0..3 {
                let next = (corner + 1) % 3;
                let indices = &mut self.out.indices;
                if ring + 1 == rings {
                    indices.extend([low + corner, low + next, apex]);
                } else {
                    indices.extend([low + corner, low + next, high + corner]);
                    indices.extend([low + next, high + next, high + corner]);
                }
            }
        }
    }

    /// Adds a stem vertex and returns its index.
    fn vertex(&mut self, position: Vec3, normal: Vec3) -> u32 {
        self.tinted_vertex(position, normal, self.stem_tint)
    }

    /// Adds a vertex with no leaf texture and returns its index.
    fn tinted_vertex(&mut self, position: Vec3, normal: Vec3, tint: [f32; 3]) -> u32 {
        let out = &mut self.out;
        out.positions.push(position);
        out.normals.push(normal);
        out.uvs.push([0.5, 0.0]);
        out.tints.push(tint);
        (out.positions.len() - 1) as u32
    }

    /// Adds the fins, the first one at the azimuth `phase` around the stem.
    fn fins(&mut self, phase: f32) {
        let twig = self.foliage.twig;
        let half = twig.fin_width / self.foliage.length;
        let tint = self.leaf_tint.map(|channel| channel * twig.fin_shade);
        let spans: u32 = if self.bend.abs() > STEM_BEND { 2 } else { 1 };
        for fin in 0..twig.fins {
            let (sin, cos) =
                (phase + std::f32::consts::PI * fin as f32 / twig.fins as f32).sin_cos();
            let first = self.out.positions.len() as u32;
            for station in 0..=spans {
                let t = station as f32 / spans as f32;
                let (center, tangent, up) = self.stem_frame(t);
                let across = up.cross(tangent) * cos + up * sin;
                let normal = tangent.cross(across);
                let reach = across * (half * (1.0 - FIN_TAPER * t));
                self.tinted_vertex(center - reach, normal, tint);
                self.tinted_vertex(center + reach, normal, tint);
            }
            for span in 0..spans {
                let (right, left) = (first + 2 * span, first + 2 * span + 1);
                self.out.indices.extend([right, right + 2, left]);
                self.out.indices.extend([left, right + 2, left + 2]);
            }
        }
    }
}

impl TemplateSet {
    /// Templates of the foliage of a species, for a mesh built with `tolerance` and
    /// `seed`. `bark` gives the colour of twig stems.
    ///
    /// The twig keeps `max(1, round(fraction n))` of its `n` leaves, enlarged by
    /// `sqrt(n / kept)` so that its leaf area is preserved.
    pub(crate) fn thinned(
        foliage: &Foliage,
        bark: &Bark,
        tolerance: f32,
        seed: u64,
        fraction: f32,
    ) -> Self {
        let leaf = LeafTemplate::new(foliage, tolerance);
        let twig = &foliage.twig;
        if !twig.enabled() {
            let flutter = vec![flutter_of(&leaf)];
            return Self {
                variants: vec![leaf],
                base_color: foliage.color,
                stem_vertices: 0,
                flutter,
            };
        }
        let leaves = if twig.leaves == 0 {
            0
        } else {
            ((fraction * twig.leaves as f32).round() as u32).clamp(1, twig.leaves)
        };
        let leaf_scale = if leaves == 0 {
            1.0
        } else {
            (twig.leaves as f32 / leaves as f32).sqrt()
        };
        let stem = bark.color_at(0.0);
        let has_stem = twig.stem_radius > 0.0;
        let base_color: [f32; 3] = std::array::from_fn(|channel| {
            let other = if has_stem { stem[channel] } else { 0.0 };
            foliage.color[channel].max(other).max(1e-6)
        });
        let ratio =
            |color: [f32; 3]| std::array::from_fn(|channel| color[channel] / base_color[channel]);
        let mut stem_vertices = 0;
        let variants: Vec<LeafTemplate> = (0..twig.variants)
            .map(|variant| {
                let mut builder = TwigBuilder {
                    foliage,
                    leaf: &leaf,
                    length: twig.length / foliage.length,
                    bend: twig.bend_deg.to_radians(),
                    leaf_tint: ratio(foliage.color),
                    stem_tint: ratio(stem),
                    leaf_scale,
                    leaves,
                    out: LeafTemplate::default(),
                };
                let mut rng = Rng::new(seed, u32::MAX - variant, Stream::FOLIAGE);
                builder.stem();
                stem_vertices = builder.out.positions.len();
                builder.leaves(&mut rng);
                builder.out
            })
            .collect();
        let flutter = variants.iter().map(flutter_of).collect();
        Self {
            variants,
            base_color,
            stem_vertices,
            flutter,
        }
    }

    /// Number of vertices and of indices of one instance.
    pub(crate) fn instance_counts(&self) -> (usize, usize) {
        self.variants
            .first()
            .map_or((0, 0), |v| (v.positions.len(), v.indices.len()))
    }
}

/// Distance of each vertex of `template` from its origin, divided by the largest.
fn flutter_of(template: &LeafTemplate) -> Vec<f32> {
    let reach = template
        .positions
        .iter()
        .map(|p| p.length())
        .fold(0.0_f32, f32::max);
    let scale = if reach > 0.0 { reach.recip() } else { 0.0 };
    template
        .positions
        .iter()
        .map(|p| (p.length() * scale).min(1.0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::TemplateSet;
    use arbrume_core::species::Species;

    fn set(foliage: &str) -> TemplateSet {
        let species = Species::from_toml_str(&format!("[foliage]\n{foliage}")).unwrap();
        TemplateSet::thinned(&species.foliage, &species.bark, 1e-4, 7, 1.0)
    }

    #[test]
    fn without_a_twig_the_set_is_the_single_leaf() {
        let set = set("segments = 3");
        assert_eq!(set.variants.len(), 1);
        assert_eq!(set.instance_counts(), (8 + 3, 24 + 3));
        assert_eq!(set.variants[0].tints, vec![[1.0; 3]; 11]);
        assert_eq!(set.base_color, Species::default().foliage.color);
    }

    #[test]
    fn a_twig_bakes_its_leaves_and_stem_into_equal_sized_variants() {
        let text = "segments = 2\npetiole_length = 0.0\n\
                    twig = { leaves = 5, length = 0.2, variants = 3, bend_deg = 0.0 }";
        let set = set(text);
        assert_eq!(set.variants.len(), 3);
        let (vertices, indices) = set.instance_counts();
        assert_eq!((vertices, indices), (4 + 5 * 5, 9 + 5 * 12));
        for variant in &set.variants {
            assert_eq!(variant.positions.len(), vertices);
            assert_eq!(variant.normals.len(), vertices);
            assert_eq!(variant.tints.len(), vertices);
            assert_eq!(variant.indices.len(), indices);
            assert!(variant.indices.iter().all(|&i| (i as usize) < vertices));
            let unit = |n: &glam::Vec3| (n.length() - 1.0).abs() < 1e-4;
            assert!(variant.normals.iter().all(unit));
            let tinted = |tint: &[f32; 3]| tint.iter().all(|c| (0.0..=1.0).contains(c));
            assert!(variant.tints.iter().all(tinted));
        }
        assert_ne!(set.variants[0].positions, set.variants[1].positions);
        let length = 0.2 / Species::default().foliage.length;
        let reach = set.variants[0].positions.iter().map(|p| p.x);
        assert!(reach.fold(0.0, f32::max) > length);
    }

    #[test]
    fn a_bent_twig_droops_and_has_a_longer_stem_mesh() {
        let straight = set("twig = { leaves = 3, bend_deg = 0.0 }");
        let bent = set("twig = { leaves = 3, bend_deg = 60.0 }");
        let (straight_vertices, _) = straight.instance_counts();
        let (bent_vertices, _) = bent.instance_counts();
        assert_eq!(bent_vertices, straight_vertices + 3);
        let lowest = |set: &TemplateSet| {
            let heights = set.variants[0].positions.iter().map(|p| p.z);
            heights.fold(f32::MAX, f32::min)
        };
        assert!(lowest(&bent) < lowest(&straight) - 0.2);
    }

    #[test]
    fn stem_colour_raises_the_base_colour_where_it_is_brighter() {
        let species = Species::from_toml_str(
            "[foliage]\ncolor = [0.05, 0.3, 0.05]\ntwig = { leaves = 2 }\n\
             [bark]\ncolor = [0.2, 0.1, 0.02]\n",
        )
        .unwrap();
        let set = TemplateSet::thinned(&species.foliage, &species.bark, 1e-4, 1, 1.0);
        assert_eq!(set.base_color, [0.2, 0.3, 0.05]);
        let stem = set.variants[0].tints[0];
        assert!((stem[0] - 1.0).abs() < 1e-6 && (stem[1] - 1.0 / 3.0).abs() < 1e-6);
    }
}

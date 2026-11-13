//! Leaf template: the geometry of one leaf of a species, built once per mesh and then
//! instanced at every organ site.
//!
//! The template lives in the frame of an organ site: `x` along the leaf from its base to
//! its tip, `y` across it, `z` towards its upper side. The unit is the leaf length: the
//! petiole runs from `x = 0` to `x = petiole_length` and the blade (or the rachis and
//! leaflets of a compound leaf) over the next unit.
//!
//! An **outline** leaf is a strip of triangles on both sides of the midrib, bounded by the
//! outline `Foliage::blade_width`. Each half is raised about the midrib by the fold angle
//! and the midrib bends along a circular arc by the curvature angle. Normals are those of
//! the folded, bent surface and are shared along the midrib, so the fold shades as a
//! smooth crease. A blade of `n` segments has `3 n - 1` vertices and `4 (n - 1)`
//! triangles. A pinnately **compound** leaf is `2 pairs + 1` such blades on a straight
//! rachis: each leaflet has the length `1 / (1 + pairs / 2)`, the last pair sits at the
//! base of the terminal leaflet and the others are evenly spaced below it. The petiole
//! and the rachis are one thin triangle.
//!
//! A **card** leaf is a flat rectangle over the same extent, for an alpha-masked texture.
//! Both kinds have texture coordinates over that rectangle, `v` from base to tip, so a
//! leaf texture fits either.
//!
//! Triangles are counter-clockwise seen from the upper side; the leaf material is
//! double-sided.

use arbrume_core::species::Foliage;
use glam::{Vec2, Vec3};
use std::f32::consts::PI;

/// Half-width of the petiole at the stem, as a fraction of the leaf length.
const STALK_HALF_WIDTH: f32 = 0.02;

/// Geometry of one leaf in the frame of its organ site, for a leaf of length 1.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct LeafTemplate {
    /// Vertex positions.
    pub positions: Vec<Vec3>,
    /// Unit vertex normals.
    pub normals: Vec<Vec3>,
    /// Texture coordinates.
    pub uvs: Vec<[f32; 2]>,
    /// Triangle list.
    pub indices: Vec<u32>,
}

/// Where the parts of a leaf go, in leaf lengths.
#[derive(Debug, Clone, Copy)]
struct Layout {
    /// Length of the petiole.
    petiole: f32,
    /// Length of the rachis of a compound leaf, 0 for a simple leaf.
    rachis: f32,
    /// Length of a blade: 1 for a simple leaf, less for a leaflet.
    blade: f32,
    /// Half of the extent of the leaf across its axis.
    half_width: f32,
}

impl Layout {
    fn new(foliage: &Foliage) -> Self {
        let pairs = foliage.leaflet_pairs as f32;
        let blade = 1.0 / (1.0 + 0.5 * pairs);
        let widest = outline_fill(foliage, 2);
        let half_blade = 0.5 * foliage.width_ratio * blade * widest;
        let (sin, cos) = foliage.leaflet_angle_deg.to_radians().sin_cos();
        let half_width = if foliage.leaflet_pairs == 0 {
            half_blade
        } else {
            half_blade.max(blade * sin + half_blade * cos)
        };
        Self {
            petiole: foliage.petiole_length,
            rachis: 1.0 - blade,
            blade,
            half_width,
        }
    }

    /// Texture coordinates of the flat point `(x, y)` of the leaf.
    fn uv(&self, flat: Vec2) -> [f32; 2] {
        [
            0.5 + 0.5 * flat.y / self.half_width,
            flat.x / (self.petiole + 1.0),
        ]
    }
}

/// Position, rotation about `z` and size of one blade in the leaf.
#[derive(Debug, Clone, Copy)]
struct Placement {
    origin: f32,
    sin: f32,
    cos: f32,
    scale: f32,
}

impl Placement {
    fn new(origin: f32, angle: f32, scale: f32) -> Self {
        let (sin, cos) = angle.sin_cos();
        Self {
            origin,
            sin,
            cos,
            scale,
        }
    }

    /// Rotates a blade-local vector into the leaf frame.
    fn turn(&self, v: Vec3) -> Vec3 {
        Vec3::new(
            v.x * self.cos - v.y * self.sin,
            v.x * self.sin + v.y * self.cos,
            v.z,
        )
    }
}

/// Point and upward normal of the midrib at the relative position `t`, for a midrib of
/// length 1 bending by `bend` radians along a circular arc towards `-z`.
fn midrib(t: f32, bend: f32) -> (Vec3, Vec3) {
    if bend.abs() < 1e-3 {
        return (Vec3::new(t, 0.0, 0.0), Vec3::Z);
    }
    let (sin, cos) = (bend * t).sin_cos();
    (
        Vec3::new(sin / bend, 0.0, (cos - 1.0) / bend),
        Vec3::new(sin, 0.0, cos),
    )
}

/// Factor by which a blade of `segments` segments is widened so that its polygon has the
/// area of the smooth outline: the mean of the outline over the mean of the polygon.
fn outline_fill(foliage: &Foliage, segments: u32) -> f32 {
    const SAMPLES: u32 = 64;
    let mean = |count: u32, offset: f32| {
        let widths = (0..count).map(|i| foliage.blade_width((i as f32 + offset) / count as f32));
        widths.sum::<f32>() / count as f32
    };
    mean(SAMPLES, 0.5) / mean(segments, 0.0)
}

struct Builder<'a> {
    foliage: &'a Foliage,
    layout: Layout,
    segments: u32,
    /// Widening of the blades that preserves their area.
    fill: f32,
    template: LeafTemplate,
}

impl Builder<'_> {
    /// Adds a vertex given in the frame of a blade; `flat` is its position before the
    /// blade was folded and bent. Returns its index.
    fn vertex(&mut self, place: &Placement, local: Vec3, normal: Vec3, flat: Vec2) -> u32 {
        let shift = Vec3::new(place.origin, 0.0, 0.0);
        let flat = place.turn(flat.extend(0.0)) * place.scale + shift;
        let template = &mut self.template;
        template
            .positions
            .push(place.turn(local) * place.scale + shift);
        template.normals.push(place.turn(normal));
        template.uvs.push(self.layout.uv(flat.truncate()));
        (template.positions.len() - 1) as u32
    }

    /// Adds one folded, bent blade.
    fn blade(&mut self, place: &Placement) {
        let foliage = self.foliage;
        let bend = foliage.curvature_deg.to_radians();
        let (fold_sin, fold_cos) = foliage.fold_deg.to_radians().sin_cos();
        let half = 0.5 * foliage.width_ratio * self.fill;
        let segments = self.segments;
        let base = self.vertex(place, Vec3::ZERO, Vec3::Z, Vec2::ZERO);
        let first = base + 1;
        for station in 1..segments {
            let t = station as f32 / segments as f32;
            let (point, up) = midrib(t, bend);
            let width = half * foliage.blade_width(t);
            let rise = up * (width * fold_sin);
            let reach = Vec3::Y * (width * fold_cos);
            let tilt = Vec3::Y * fold_sin;
            self.vertex(
                place,
                point + reach + rise,
                up * fold_cos - tilt,
                Vec2::new(t, width),
            );
            self.vertex(place, point, up, Vec2::new(t, 0.0));
            self.vertex(
                place,
                point - reach + rise,
                up * fold_cos + tilt,
                Vec2::new(t, -width),
            );
        }
        let (point, up) = midrib(1.0, bend);
        let tip = self.vertex(place, point, up, Vec2::X);
        let last = first + 3 * (segments - 2);
        let indices = &mut self.template.indices;
        indices.extend([base, first + 1, first, base, first + 2, first + 1]);
        for station in 0..segments - 2 {
            let (left, next) = (first + 3 * station, first + 3 * station + 3);
            let (mid, right) = (left + 1, left + 2);
            indices.extend([mid, next + 1, next, mid, next, left]);
            indices.extend([right, next + 2, next + 1, right, next + 1, mid]);
        }
        indices.extend([last + 1, tip, last, last + 2, tip, last + 1]);
    }

    /// Adds the triangle of the petiole and rachis, from the stem to `end`.
    fn stalk(&mut self, end: f32) {
        let whole = Placement::new(0.0, 0.0, 1.0);
        let side = Vec2::new(0.0, STALK_HALF_WIDTH);
        let right = self.vertex(&whole, (-side).extend(0.0), Vec3::Z, -side);
        let tip = self.vertex(&whole, Vec3::X * end, Vec3::Z, Vec2::X * end);
        let left = self.vertex(&whole, side.extend(0.0), Vec3::Z, side);
        self.template.indices.extend([right, tip, left]);
    }

    /// Adds the blades of a simple or compound leaf and its stalk.
    fn outline(&mut self) {
        let layout = self.layout;
        let end = layout.petiole + layout.rachis;
        if end > 0.0 {
            self.stalk(end);
        }
        let pairs = self.foliage.leaflet_pairs;
        let angle = self.foliage.leaflet_angle_deg.to_radians();
        for pair in 1..=pairs {
            let origin = layout.petiole + layout.rachis * pair as f32 / pairs as f32;
            self.blade(&Placement::new(origin, angle, layout.blade));
            self.blade(&Placement::new(origin, -angle, layout.blade));
        }
        self.blade(&Placement::new(end, 0.0, layout.blade));
    }

    /// Adds the rectangle of a card leaf.
    fn card(&mut self) {
        let whole = Placement::new(0.0, 0.0, 1.0);
        let (length, half) = (self.layout.petiole + 1.0, self.layout.half_width);
        let corners = [[0.0, -half], [length, -half], [length, half], [0.0, half]];
        for corner in corners {
            let flat = Vec2::from(corner);
            self.vertex(&whole, flat.extend(0.0), Vec3::Z, flat);
        }
        self.template.indices.extend([0, 1, 2, 0, 2, 3]);
    }
}

impl LeafTemplate {
    /// Template of the leaves of `foliage`.
    ///
    /// The outline of a blade of width `w` cut into `n` segments deviates from the smooth
    /// outline by about `w pi^2 / (16 n^2)`, so a blade gets
    /// `n = pi sqrt(w / (16 tolerance))` segments, at least 2 and at most
    /// `Foliage::segments`. Whatever `n`, the polygon is widened so that its area is that
    /// of the smooth outline: a coarse leaf covers as much as a fine one.
    pub(crate) fn new(foliage: &Foliage, tolerance: f32) -> Self {
        let layout = Layout::new(foliage);
        let width = foliage.length * foliage.width_ratio * layout.blade;
        let ideal = (PI * (width / (16.0 * tolerance)).sqrt()).ceil() as u32;
        let segments = ideal.clamp(2, foliage.segments.max(2));
        let mut builder = Builder {
            foliage,
            layout,
            segments,
            fill: outline_fill(foliage, segments),
            template: Self::default(),
        };
        if foliage.uses_cards() {
            builder.card();
        } else {
            builder.outline();
        }
        builder.template
    }

    /// Surface area of the template, in squared leaf lengths.
    #[cfg(test)]
    pub(crate) fn area(&self) -> f32 {
        let corner = |index: u32| self.positions[index as usize];
        let (triangles, _) = self.indices.as_chunks::<3>();
        triangles
            .iter()
            .map(|t| {
                0.5 * (corner(t[1]) - corner(t[0]))
                    .cross(corner(t[2]) - corner(t[0]))
                    .length()
            })
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::LeafTemplate;
    use arbrume_core::species::{Foliage, Species};
    use glam::Vec3;

    fn foliage(toml: &str) -> Foliage {
        Species::from_toml_str(&format!("[foliage]\n{toml}"))
            .unwrap()
            .foliage
    }

    fn check(template: &LeafTemplate) {
        let count = template.positions.len();
        assert_eq!(template.normals.len(), count);
        assert_eq!(template.uvs.len(), count);
        assert_eq!(template.indices.len() % 3, 0);
        let mut used = vec![false; count];
        for triangle in template.indices.as_chunks::<3>().0 {
            let [a, b, c] = triangle.map(|index| template.positions[index as usize]);
            let face = (b - a).cross(c - a);
            assert!(face.length() > 1e-7, "degenerate triangle {triangle:?}");
            for &index in triangle {
                used[index as usize] = true;
                let normal = template.normals[index as usize];
                assert!((normal.length() - 1.0).abs() < 1e-5);
                assert!(
                    normal.dot(face) > 0.0,
                    "triangle faces away from its normals"
                );
            }
        }
        assert!(used.iter().all(|&used| used));
        let in_unit = |uv: &[f32; 2]| uv.iter().all(|c| (-1e-5..=1.0 + 1e-5).contains(c));
        assert!(template.uvs.iter().all(in_unit));
    }

    #[test]
    fn simple_blade_has_the_documented_counts() {
        for segments in 2..=8 {
            let foliage = foliage(&format!("segments = {segments}\npetiole_length = 0.0"));
            let template = LeafTemplate::new(&foliage, 1e-6);
            assert_eq!(template.positions.len(), 3 * segments - 1);
            assert_eq!(template.indices.len(), 3 * 4 * (segments - 1));
            check(&template);
        }
    }

    #[test]
    fn every_shape_and_layout_is_well_formed() {
        let shapes = ["elliptic", "ovate", "obovate", "lanceolate", "needle"];
        for shape in shapes {
            for extra in [
                "",
                "fold_deg = 0.0\ncurvature_deg = 0.0",
                "curvature_deg = -90.0\nfold_deg = 60.0",
                "leaflet_pairs = 3\nsegments = 2",
                "leaflet_pairs = 1\npetiole_length = 0.0",
            ] {
                let foliage = foliage(&format!("shape = \"{shape}\"\n{extra}"));
                check(&LeafTemplate::new(&foliage, 1e-6));
            }
        }
    }

    #[test]
    fn flat_elliptic_blade_approaches_the_area_of_its_ellipse() {
        let text = "shape = \"elliptic\"\nsegments = 16\nfold_deg = 0.0\ncurvature_deg = 0.0\n\
                    petiole_length = 0.0\nwidth_ratio = 0.5";
        let template = LeafTemplate::new(&foliage(text), 1e-7);
        let ellipse = std::f32::consts::PI * 0.5 * 0.25;
        assert!((template.area() - ellipse).abs() < 0.03 * ellipse);
    }

    #[test]
    fn leaf_spans_its_length_and_droops() {
        let template = LeafTemplate::new(&foliage("petiole_length = 0.2"), 1e-6);
        let tip = template
            .positions
            .iter()
            .copied()
            .fold(Vec3::ZERO, |far, p| if p.x > far.x { p } else { far });
        assert!(tip.x > 1.1 && tip.x <= 1.2 + 1e-5);
        assert!(tip.z < -0.1, "positive curvature bends the blade down");
        assert!(
            template.positions.iter().any(|p| p.z > 0.01),
            "fold raises the margins"
        );
    }

    #[test]
    fn coarse_tolerance_uses_fewer_segments() {
        let foliage = foliage("segments = 6");
        let fine = LeafTemplate::new(&foliage, 1e-5);
        let coarse = LeafTemplate::new(&foliage, 0.05);
        assert!(coarse.indices.len() < fine.indices.len());
        assert_eq!(coarse.indices.len(), 3 * 4 + 3);
    }

    #[test]
    fn card_is_one_flat_rectangle() {
        let template = LeafTemplate::new(&foliage("geometry = \"card\""), 1e-6);
        assert_eq!((template.positions.len(), template.indices.len()), (4, 6));
        check(&template);
        assert!(template.positions.iter().all(|p| p.z == 0.0));
    }
}

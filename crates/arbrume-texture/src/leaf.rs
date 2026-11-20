//! One cell of the leaf atlas: a leaf drawn over its [`LeafLayout`] rectangle.
//!
//! Each texel of the cell is a point of the leaf frame. Its coverage is the largest
//! over the blades of the leaf (one for a simple leaf, `2 pairs + 1` for a compound
//! one) and the stalk: inside a blade when its distance to the axis of the blade is less
//! than the half-width of the blade there, margin included (`venation::Blade::width`),
//! with an antialiased edge one texel wide. The stalk (petiole and rachis) always
//! reaches a little past the base, so that `LeafAtlas::STEM_UV` is opaque.
//!
//! Veins are segments rasterized into an intensity buffer within their bounding boxes;
//! a vein thinner than a texel fades instead of aliasing. A network of fine veins comes
//! from the borders of a Voronoi diagram in the frame of each blade. The colour is the
//! leaf colour with a hue and brightness of the variant, low-frequency mottling, a paler
//! margin, and the vein colour over the veins; heights are the veins and a faint
//! puckering, turned into normals.
//!
//! A needle (`shape = "needle"`) is drawn opaque over the whole cell, rounded across
//! with a paler band of stomata along its middle: the needle mesh is a triangle within
//! the cell, which an alpha cut would only thin.

use crate::noise::{random, smoothstep, tiled, worley};
use crate::surface::height_normals;
use crate::venation::{Blade, Vein};
use arbrume_core::species::{BladePlacement, BladeShape, Foliage, LeafLayout};
use glam::{Vec2, Vec3};

/// What one atlas cell holds, row-major.
#[derive(Debug, Clone, Default)]
pub(crate) struct CellImage {
    pub width: usize,
    pub colors: Vec<Vec3>,
    pub alpha: Vec<f32>,
    pub normals: Vec<Vec3>,
}

/// A blade of the leaf with its placement and its veins.
struct Placed<'a> {
    place: BladePlacement,
    blade: Blade<'a>,
    veins: Vec<Vein>,
}

impl Placed<'_> {
    /// Point of the frame of the blade at the leaf point `p`.
    fn local(&self, p: Vec2) -> Vec2 {
        let (sin, cos) = self.place.angle.sin_cos();
        let d = p - Vec2::new(self.place.origin, 0.0);
        Vec2::new(d.x * cos + d.y * sin, -d.x * sin + d.y * cos) / self.place.scale
    }

    /// Leaf point of the point `q` of the frame of the blade.
    fn leaf(&self, q: Vec2) -> Vec2 {
        let (sin, cos) = self.place.angle.sin_cos();
        let q = q * self.place.scale;
        Vec2::new(
            self.place.origin + q.x * cos - q.y * sin,
            q.x * sin + q.y * cos,
        )
    }

    /// Signed distance from the leaf point `p` into the blade, in leaf lengths.
    fn inside(&self, p: Vec2) -> f32 {
        self.blade.inside(self.local(p)) * self.place.scale
    }
}

/// Draws the leaf cells of one species.
pub(crate) struct Painter<'a> {
    foliage: &'a Foliage,
    layout: LeafLayout,
    width: usize,
    height: usize,
    /// Size of a texel in leaf lengths, across and along.
    texel: Vec2,
    seed: u64,
}

impl<'a> Painter<'a> {
    pub fn new(foliage: &'a Foliage, size: [usize; 2], seed: u64) -> Self {
        let layout = foliage.leaf_layout();
        let texel = Vec2::new(
            2.0 * layout.half_width / size[0] as f32,
            (layout.petiole + 1.0) / size[1] as f32,
        );
        Self {
            foliage,
            layout,
            width: size[0],
            height: size[1],
            texel,
            seed,
        }
    }

    /// Antialiasing width in leaf lengths.
    fn aa(&self) -> f32 {
        self.texel.max_element()
    }

    /// Leaf point at the centre of texel `(x, y)`.
    fn point(&self, x: usize, y: usize) -> Vec2 {
        let uv = [
            (x as f32 + 0.5) / self.width as f32,
            (y as f32 + 0.5) / self.height as f32,
        ];
        Vec2::from(self.layout.point(uv))
    }

    /// Texel coordinates (continuous) of the leaf point `p`.
    fn texel_of(&self, p: Vec2) -> Vec2 {
        let [u, v] = self.layout.uv(p.x, p.y);
        Vec2::new(u * self.width as f32, v * self.height as f32)
    }

    /// The cell of variant `variant`.
    pub fn cell(&self, variant: u32) -> CellImage {
        if self.foliage.shape == BladeShape::Needle {
            return self.needle(variant);
        }
        let seed = self.seed ^ u64::from(variant).wrapping_mul(0x9E37_79B9);
        let placed: Vec<Placed> = self
            .foliage
            .blade_placements()
            .into_iter()
            .enumerate()
            .map(|(index, place)| {
                let (blade, veins) = Blade::new(self.foliage, seed ^ index as u64);
                Placed {
                    place,
                    blade,
                    veins,
                }
            })
            .collect();
        let mut veins = vec![0.0_f32; self.width * self.height];
        for blade in &placed {
            for vein in &blade.veins {
                let a = blade.leaf(vein.a);
                let b = blade.leaf(vein.b);
                let scale = blade.place.scale;
                self.stroke(
                    &mut veins,
                    [a, b],
                    [vein.width_a * scale, vein.width_b * scale],
                );
            }
        }
        self.shade(&placed, &veins, seed)
    }

    /// Rasterizes the segment `ends` of widths `widths` (leaf lengths) into `buffer`.
    fn stroke(&self, buffer: &mut [f32], ends: [Vec2; 2], widths: [f32; 2]) {
        let aa = self.aa();
        let reach = 0.5 * widths[0].max(widths[1]) + aa;
        let (ta, tb) = (self.texel_of(ends[0]), self.texel_of(ends[1]));
        let margin = Vec2::new(reach / self.texel.x, reach / self.texel.y);
        let low = (ta.min(tb) - margin).floor().max(Vec2::ZERO);
        let high = (ta.max(tb) + margin)
            .ceil()
            .min(Vec2::new(self.width as f32, self.height as f32));
        let segment = ends[1] - ends[0];
        let length = segment.length_squared().max(1e-12);
        for y in low.y as usize..high.y as usize {
            for x in low.x as usize..high.x as usize {
                let p = self.point(x, y);
                let f = ((p - ends[0]).dot(segment) / length).clamp(0.0, 1.0);
                let distance = p.distance(ends[0] + segment * f);
                let width = widths[0] + (widths[1] - widths[0]) * f;
                let drawn = width.max(0.7 * aa);
                let fade = (width / drawn).min(1.0);
                let half = 0.5 * drawn;
                let value = fade * (1.0 - smoothstep(half - 0.5 * aa, half + 0.5 * aa, distance));
                let slot = &mut buffer[y * self.width + x];
                *slot = slot.max(value);
            }
        }
    }

    /// Coverage of the stalk at `p` and its rounded height.
    fn stalk(&self, p: Vec2) -> (f32, f32) {
        let layout = &self.layout;
        let end = (layout.petiole + layout.rachis).max(0.04 * (layout.petiole + 1.0));
        let half = 0.012_f32.max(0.8 * self.aa());
        let along = p.x.clamp(0.0, end);
        let distance = p.distance(Vec2::new(along, 0.0));
        let coverage = (0.5 + (half - distance) / self.aa()).clamp(0.0, 1.0);
        let round = (1.0 - (distance / half).min(1.0).powi(2)).sqrt();
        (coverage, round)
    }

    /// Colours, alpha and normals of a cell from its blades and vein buffer.
    fn shade(&self, placed: &[Placed], veins: &[f32], seed: u64) -> CellImage {
        let foliage = self.foliage;
        let variation = foliage.texture_variation;
        let leaf = Vec3::from(foliage.color) * self.variant_tint(seed);
        let vein_color = Vec3::from(foliage.venation.vein_color(foliage.color));
        let venation = foliage.venation;
        let aa = self.aa();
        let count = self.width * self.height;
        let mut image = CellImage {
            width: self.width,
            colors: vec![leaf; count],
            alpha: vec![0.0; count],
            normals: Vec::new(),
        };
        let mut heights = vec![0.0_f32; count];
        let fine = (venation.veins.max(4) as f32) * 3.0;
        for y in 0..self.height {
            for x in 0..self.width {
                let index = y * self.width + x;
                let p = self.point(x, y);
                let best = placed
                    .iter()
                    .map(|blade| (blade, blade.inside(p)))
                    .max_by(|a, b| a.1.total_cmp(&b.1));
                let (stalk, round) = self.stalk(p);
                let Some((blade, inside)) = best else {
                    continue;
                };
                let coverage = (0.5 + inside / aa).clamp(0.0, 1.0);
                let q = blade.local(p);
                let cell = worley(q * fine, [4096, 4096], 0.9, Vec2::ONE, seed ^ 0xB1);
                let line = 0.04 + aa / blade.place.scale * fine * 0.5;
                let tertiary = venation.tertiary * (1.0 - smoothstep(0.0, line, cell.border));
                let uv = Vec2::new(x as f32 / self.width as f32, y as f32 / self.height as f32);
                let mottle = tiled(uv, [3, 4], seed ^ 0xB2) + 0.5 * tiled(uv, [7, 9], seed ^ 0xB3);
                let edge =
                    1.0 - smoothstep(0.0, 0.08 * blade.blade.half, inside / blade.place.scale);
                let vein = veins[index];
                let mut color = leaf * (1.0 + 0.3 * variation * mottle);
                color = color.lerp(vein_color, 0.5 * variation * edge);
                color = color.lerp(vein_color, venation.contrast * vein.max(0.5 * tertiary));
                let mut height = vein + 0.35 * tertiary + 0.1 * mottle;
                if stalk > coverage {
                    color = vein_color * 0.85;
                    height = 1.5 * round;
                }
                image.colors[index] = color;
                image.alpha[index] = coverage.max(stalk);
                heights[index] = height;
            }
        }
        let width = foliage.venation.width * foliage.width_ratio * self.layout.blade;
        let amount = venation.relief * 0.5 * width.max(aa);
        let spacing = [self.texel.x, self.texel.y];
        image.normals = height_normals(&heights, self.width, spacing, amount, false);
        image
    }

    /// Hue and brightness factor of a variant.
    fn variant_tint(&self, seed: u64) -> Vec3 {
        let variation = self.foliage.texture_variation;
        let hue = 0.3 * variation * (random(seed, 0, 0, 0xB4) - 0.5);
        let brightness = 1.0 + 0.3 * variation * (random(seed, 0, 0, 0xB5) - 0.5);
        Vec3::new(1.0 + hue, 1.0, 1.0 - hue) * brightness
    }

    /// A needle cell: opaque, rounded across, with a pale band of stomata.
    fn needle(&self, variant: u32) -> CellImage {
        let seed = self.seed ^ u64::from(variant).wrapping_mul(0x9E37_79B9);
        let leaf = Vec3::from(self.foliage.color) * self.variant_tint(seed);
        let stomata = leaf.lerp(Vec3::new(0.35, 0.45, 0.42), 0.25);
        let count = self.width * self.height;
        let (mut colors, mut heights) = (Vec::with_capacity(count), Vec::with_capacity(count));
        for y in 0..self.height {
            for x in 0..self.width {
                let across = 2.0 * (x as f32 + 0.5) / self.width as f32 - 1.0;
                let round = (1.0 - across * across).max(0.0).sqrt();
                let uv = Vec2::new(x as f32 / self.width as f32, y as f32 / self.height as f32);
                let streak = tiled(uv, [2, 16], seed ^ 0xB6);
                let band = 1.0 - smoothstep(0.1, 0.3, across.abs());
                let shade = 0.75 + 0.25 * round + 0.08 * streak;
                colors.push(leaf.lerp(stomata, band) * shade);
                heights.push(round);
            }
        }
        let spacing = [self.texel.x, self.texel.y];
        let amount = self.layout.half_width * 0.5;
        CellImage {
            width: self.width,
            normals: height_normals(&heights, self.width, spacing, amount, false),
            colors,
            alpha: vec![1.0; count],
        }
    }
}

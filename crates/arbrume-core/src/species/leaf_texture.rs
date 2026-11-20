//! What the leaf texture of a species looks like and where it goes.
//!
//! * [`Venation`] and [`Margin`] describe the veins and the edge of the blade drawn by
//!   `arbrume-texture` (see `docs/textures.md`).
//! * [`LeafLayout`] is the rectangle a leaf occupies in its own frame: the leaf
//!   template of the mesher and the texture share it, so that texture coordinates
//!   `[0, 1]^2` over that rectangle put each blade of the texture on its geometry.
//! * [`LeafAtlas`] lays the variants of the leaf, and the needle spray of a conifer
//!   twig, out as cells of one texture.
//!
//! All defaults are starting points for tuning, not measured values.

use super::bark::check_resolution;
use super::{Foliage, SpeciesError, check_color, check_range};
use serde::{Deserialize, Serialize};

/// Largest value of [`Venation::veins`] and [`Margin::teeth`].
pub const MAX_VEINS: u32 = 64;
/// Largest value of [`Foliage::texture_variants`].
pub const MAX_TEXTURE_VARIANTS: u32 = 16;

/// Pattern of the veins of a blade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VenationKind {
    /// No veins drawn.
    None,
    /// A midrib with secondary veins leaving it at an angle on both sides, each ending
    /// at the margin (most broadleaves).
    #[default]
    Pinnate,
    /// Several primary veins fanning out from the base of the blade (maple, plane).
    Palmate,
    /// Veins running side by side from base to tip (grasses, palms, bamboo, needles).
    Parallel,
    /// Open venation grown towards sources scattered in the blade (Runions et al.
    /// 2005): a tree of veins whose widths follow Murray's law.
    Open,
}

/// Veins of the leaf texture.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Venation {
    /// Pattern of the veins.
    pub kind: VenationKind,
    /// Number of secondary veins on each side of the midrib (pinnate), of primary veins
    /// on each side of the central one (palmate), or of veins across the blade
    /// (parallel); at most [`MAX_VEINS`].
    pub veins: u32,
    /// Angle in degrees between a secondary vein and the midrib where they meet, in
    /// `[5, 85]`; for palmate venation the angle between the outermost primary vein and
    /// the central one.
    pub angle_deg: f32,
    /// Width of the midrib at the base of the blade, as a fraction of the blade width,
    /// in `[0, 0.25]`. Other veins are thinner in proportion.
    pub width: f32,
    /// How visible the veins are in the colour, in `[0, 1]`.
    pub contrast: f32,
    /// Height of the veins in the normal map, in `[0, 1]`.
    pub relief: f32,
    /// Strength of the network of fine veins between the secondary ones, in `[0, 1]`.
    pub tertiary: f32,
    /// Linear RGB colour of the veins; by default the leaf colour lightened towards
    /// yellow.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<[f32; 3]>,
}

impl Default for Venation {
    fn default() -> Self {
        Self {
            kind: VenationKind::default(),
            veins: 7,
            angle_deg: 45.0,
            width: 0.04,
            contrast: 0.4,
            relief: 0.4,
            tertiary: 0.3,
            color: None,
        }
    }
}

impl Venation {
    /// Colour of the veins of a leaf of colour `leaf`.
    #[must_use]
    pub fn vein_color(&self, leaf: [f32; 3]) -> [f32; 3] {
        self.color.unwrap_or([
            (leaf[0] * 2.2 + 0.03).min(1.0),
            (leaf[1] * 1.5 + 0.02).min(1.0),
            (leaf[2] * 1.2).min(1.0),
        ])
    }

    fn validate(&self) -> Result<(), SpeciesError> {
        let veins = self.veins as f32;
        check_range("foliage.venation.veins", veins, 0.0, MAX_VEINS as f32)?;
        check_range("foliage.venation.angle_deg", self.angle_deg, 5.0, 85.0)?;
        check_range("foliage.venation.width", self.width, 0.0, 0.25)?;
        check_range("foliage.venation.contrast", self.contrast, 0.0, 1.0)?;
        check_range("foliage.venation.relief", self.relief, 0.0, 1.0)?;
        check_range("foliage.venation.tertiary", self.tertiary, 0.0, 1.0)?;
        if let Some(color) = self.color {
            check_color("foliage.venation.color", color)?;
        }
        Ok(())
    }
}

/// Shape of the edge of a blade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MarginKind {
    /// A smooth edge.
    #[default]
    Entire,
    /// Saw teeth pointing towards the tip.
    Serrate,
    /// Teeth pointing outwards.
    Dentate,
    /// Rounded scallops.
    Crenate,
    /// Deep rounded lobes, one at the end of each secondary (or primary) vein, with
    /// sinuses between them.
    Lobed,
}

/// Edge of the leaf texture: where its alpha is cut.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Margin {
    /// Shape of the edge.
    pub kind: MarginKind,
    /// Number of teeth on each side of the blade, at most [`MAX_VEINS`]; lobes follow
    /// the veins instead.
    pub teeth: u32,
    /// Depth of a tooth or of a sinus between lobes, as a fraction of the local
    /// half-width of the blade, in `[0, 0.9]`.
    pub depth: f32,
    /// Random variation of the size and place of teeth and lobes, in `[0, 1]`.
    pub irregularity: f32,
}

impl Default for Margin {
    fn default() -> Self {
        Self {
            kind: MarginKind::default(),
            teeth: 16,
            depth: 0.06,
            irregularity: 0.3,
        }
    }
}

impl Margin {
    fn validate(&self) -> Result<(), SpeciesError> {
        let teeth = self.teeth as f32;
        check_range("foliage.margin.teeth", teeth, 0.0, MAX_VEINS as f32)?;
        check_range("foliage.margin.depth", self.depth, 0.0, 0.9)?;
        check_range("foliage.margin.irregularity", self.irregularity, 0.0, 1.0)
    }
}

/// Checks the texture fields of `foliage`.
pub(super) fn validate_texture(foliage: &Foliage) -> Result<(), SpeciesError> {
    check_resolution("foliage.texture_resolution", foliage.texture_resolution)?;
    let variants = foliage.texture_variants as f32;
    let most = MAX_TEXTURE_VARIANTS as f32;
    check_range("foliage.texture_variants", variants, 1.0, most)?;
    let variation = foliage.texture_variation;
    check_range("foliage.texture_variation", variation, 0.0, 1.0)?;
    foliage.venation.validate()?;
    foliage.margin.validate()
}

/// Where the parts of a leaf go in its own frame, in leaf lengths: `x` from the stem
/// along the leaf, `y` across it.
///
/// The petiole runs from `x = 0` to `petiole`, then the blade, or the rachis and the
/// leaflets of a compound leaf, over one unit. The leaf fits in the rectangle
/// `[0, petiole + 1] x [-half_width, half_width]`, which texture coordinates map to
/// `[0, 1]^2`: `u = 0.5 + 0.5 y / half_width`, `v = x / (petiole + 1)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LeafLayout {
    /// Length of the petiole.
    pub petiole: f32,
    /// Length of the rachis of a compound leaf, 0 for a simple leaf.
    pub rachis: f32,
    /// Length of a blade: 1 for a simple leaf, less for a leaflet.
    pub blade: f32,
    /// Half of the extent of the leaf across its axis.
    pub half_width: f32,
}

/// Position, rotation and size of one blade (a leaflet, or the blade of a simple leaf)
/// in the frame of its leaf.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BladePlacement {
    /// `x` of the base of the blade.
    pub origin: f32,
    /// Angle in radians from the axis of the leaf, positive towards `+y`.
    pub angle: f32,
    /// Length of the blade.
    pub scale: f32,
}

impl LeafLayout {
    /// Texture coordinates of the point `(x, y)` of the leaf frame.
    #[must_use]
    pub fn uv(&self, x: f32, y: f32) -> [f32; 2] {
        [0.5 + 0.5 * y / self.half_width, x / (self.petiole + 1.0)]
    }

    /// Point of the leaf frame at texture coordinates `uv`.
    #[must_use]
    pub fn point(&self, uv: [f32; 2]) -> [f32; 2] {
        [
            uv[1] * (self.petiole + 1.0),
            (2.0 * uv[0] - 1.0) * self.half_width,
        ]
    }
}

impl Foliage {
    /// Factor by which a blade of `segments` segments is widened so that its polygon has
    /// the area of the smooth outline: the mean of the outline over the mean of the
    /// polygon. A blade of fewer than two segments is a triangle, widened twice as much.
    #[must_use]
    pub fn outline_fill(&self, segments: u32) -> f32 {
        const SAMPLES: u32 = 64;
        let mean = |count: u32, offset: f32| {
            let widths = (0..count).map(|i| self.blade_width((i as f32 + offset) / count as f32));
            widths.sum::<f32>() / count as f32
        };
        if segments < 2 {
            return 2.0 * mean(SAMPLES, 0.5);
        }
        mean(SAMPLES, 0.5) / mean(segments, 0.0)
    }

    /// The rectangle of the leaf and the size of its blades, see [`LeafLayout`].
    #[must_use]
    pub fn leaf_layout(&self) -> LeafLayout {
        let pairs = self.leaflet_pairs as f32;
        let blade = 1.0 / (1.0 + 0.5 * pairs);
        let widest = self.outline_fill(2);
        let half_blade = 0.5 * self.width_ratio * blade * widest;
        let (sin, cos) = self.leaflet_angle_deg.to_radians().sin_cos();
        let half_width = if self.leaflet_pairs == 0 {
            half_blade
        } else {
            half_blade.max(blade * sin + half_blade * cos)
        };
        LeafLayout {
            petiole: self.petiole_length,
            rachis: 1.0 - blade,
            blade,
            half_width,
        }
    }

    /// The blades of a leaf: the lateral leaflets pair by pair from the base, then the
    /// terminal blade (the only one of a simple leaf).
    #[must_use]
    pub fn blade_placements(&self) -> Vec<BladePlacement> {
        let layout = self.leaf_layout();
        let pairs = self.leaflet_pairs;
        let angle = self.leaflet_angle_deg.to_radians();
        let mut blades = Vec::with_capacity(2 * pairs as usize + 1);
        for pair in 1..=pairs {
            let origin = layout.petiole + layout.rachis * pair as f32 / pairs as f32;
            for side in [angle, -angle] {
                blades.push(BladePlacement {
                    origin,
                    angle: side,
                    scale: layout.blade,
                });
            }
        }
        blades.push(BladePlacement {
            origin: layout.petiole + layout.rachis,
            angle: 0.0,
            scale: layout.blade,
        });
        blades
    }

    /// Base colour of the leaf material: per channel the larger of the leaf colour and,
    /// for a twig with a stem, of the colour of young bark (`bark`), so that vertex
    /// colours of at most 1 reach both.
    #[must_use]
    pub fn material_color(&self, bark: &super::Bark) -> [f32; 3] {
        let stem = bark.color_at(0.0);
        let has_stem = self.twig.enabled() && self.twig.stem_radius > 0.0;
        std::array::from_fn(|channel| {
            let other = if has_stem { stem[channel] } else { 0.0 };
            self.color[channel].max(other).max(1e-6)
        })
    }

    /// Layout of the leaf texture atlas, see [`LeafAtlas`].
    #[must_use]
    pub fn atlas(&self) -> LeafAtlas {
        let layout = self.leaf_layout();
        let leaf_aspect = 2.0 * layout.half_width / (layout.petiole + 1.0);
        let twig = &self.twig;
        let spray = twig.fins > 0 && twig.fin_width > 0.0;
        let spray_aspect = 2.0 * twig.fin_width / twig.length;
        let cells = self.texture_variants + u32::from(spray);
        let error = |columns: u32| {
            let rows = cells.div_ceil(columns);
            let aspect = rows as f32 / columns as f32;
            let leaf = (aspect / leaf_aspect).ln().abs();
            let fins = if spray {
                (aspect / spray_aspect).ln().abs()
            } else {
                0.0
            };
            let empty = (rows * columns - cells) as f32;
            leaf + fins + 0.05 * empty
        };
        let columns = (1..=cells)
            .min_by(|&a, &b| error(a).total_cmp(&error(b)))
            .unwrap_or(1);
        LeafAtlas {
            columns,
            rows: cells.div_ceil(columns),
            leaf_cells: self.texture_variants,
            spray,
        }
    }
}

/// The cells of the leaf texture of a species.
///
/// The square texture (`Foliage::texture_resolution` texels) is cut into
/// `columns x rows` cells, filled row by row from the top: first `leaf_cells` variants of the leaf, then, for a twig
/// with fins, the needle spray drawn on the fins. A leaf maps its texture coordinates
/// `[0, 1]^2` (see [`LeafLayout`]) to the inner rectangle of its cell, inset by a
/// padding that keeps filtering from bleeding between cells. The grid is chosen so that
/// the cells have about the shape of the leaf (and of the fins).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeafAtlas {
    /// Cells per row.
    pub columns: u32,
    /// Rows of cells.
    pub rows: u32,
    /// Number of leaf variants, the first cells.
    pub leaf_cells: u32,
    /// Whether the cell after the leaves holds the needle spray of the fins.
    pub spray: bool,
}

impl LeafAtlas {
    /// Fraction of the smaller side of a cell left empty on each side of it, so that
    /// filtering and mipmaps do not bleed from one cell into the next.
    pub const PADDING: f32 = 1.0 / 32.0;

    /// Point of a leaf cell that the texture keeps opaque, on the stalk at the base of
    /// the leaf: the stem of a twig is textured with it.
    pub const STEM_UV: [f32; 2] = [0.5, 0.01];

    /// Index of the cell of the needle spray, if any.
    #[must_use]
    pub const fn spray_cell(&self) -> Option<u32> {
        if self.spray {
            Some(self.leaf_cells)
        } else {
            None
        }
    }

    /// Rectangle `[u0, v0, u1, v1]` of the inside of cell `cell` in atlas texture
    /// coordinates: the cell inset on every side by [`Self::PADDING`] of its smaller
    /// side. It does not depend on the resolution of the texture.
    #[must_use]
    pub fn cell_rect(&self, cell: u32) -> [f32; 4] {
        let (column, row) = (cell % self.columns, cell / self.columns);
        let (width, height) = (1.0 / self.columns as f32, 1.0 / self.rows as f32);
        let pad = Self::PADDING * width.min(height);
        let (u0, v0) = (column as f32 * width, row as f32 * height);
        [u0 + pad, v0 + pad, u0 + width - pad, v0 + height - pad]
    }

    /// Texture coordinates in the atlas of the point `uv` of cell `cell`.
    #[must_use]
    pub fn map(&self, cell: u32, uv: [f32; 2]) -> [f32; 2] {
        let [u0, v0, u1, v1] = self.cell_rect(cell);
        [u0 + uv[0] * (u1 - u0), v0 + uv[1] * (v1 - v0)]
    }
}

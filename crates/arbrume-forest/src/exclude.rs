//! Where trees may not stand: clearings, roads, rivers, water and painted masks.
//!
//! Every exclusion gives, at a point of the ground, a factor in `[0, 1]` by which the
//! density there is multiplied: 0 inside the shape, rising linearly to 1 over `falloff`
//! meters outside it (a soft edge), 1 farther away. The factors of all exclusions are
//! multiplied. A raster mask gives `1 - value` directly (white excludes, black allows,
//! grey thins). Points below the water level get 0.

use std::path::Path;

use crate::error::ForestError;
use crate::terrain::Terrain;

/// A shape in the ground plane, in meters (`x`, `z`).
#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    /// A disc: a clearing.
    Circle {
        /// Centre.
        center: [f32; 2],
        /// Radius.
        radius: f32,
    },
    /// A band along a polyline: a road or a river.
    Path {
        /// The polyline, at least two points.
        points: Vec<[f32; 2]>,
        /// Full width of the band.
        width: f32,
    },
    /// A simple polygon (even-odd rule).
    Polygon {
        /// Corners, at least three.
        points: Vec<[f32; 2]>,
    },
}

impl Shape {
    /// Distance from `(x, z)` to the shape, 0 inside.
    #[must_use]
    pub fn distance(&self, x: f32, z: f32) -> f32 {
        match self {
            Self::Circle { center, radius } => {
                ((x - center[0]).hypot(z - center[1]) - radius).max(0.0)
            }
            Self::Path { points, width } => {
                (polyline_distance(points, x, z) - width / 2.0).max(0.0)
            }
            Self::Polygon { points } => {
                if inside_polygon(points, x, z) {
                    0.0
                } else {
                    closed_distance(points, x, z)
                }
            }
        }
    }

    /// Bounding rectangle `[x0, z0, x1, z1]`, grown by `margin`.
    #[must_use]
    pub fn bounds(&self, margin: f32) -> [f32; 4] {
        let (points, grow): (Vec<[f32; 2]>, f32) = match self {
            Self::Circle { center, radius } => (vec![*center], *radius),
            Self::Path { points, width } => (points.clone(), width / 2.0),
            Self::Polygon { points } => (points.clone(), 0.0),
        };
        let mut b = [
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ];
        for p in points {
            b = [
                b[0].min(p[0]),
                b[1].min(p[1]),
                b[2].max(p[0]),
                b[3].max(p[1]),
            ];
        }
        let m = grow + margin;
        [b[0] - m, b[1] - m, b[2] + m, b[3] + m]
    }

    /// Checks the sizes and the point counts.
    ///
    /// # Errors
    /// [`ForestError::Exclusion`] describing the problem.
    pub fn validate(&self) -> Result<(), ForestError> {
        let fail = |reason: &str| Err(ForestError::Exclusion(reason.to_owned()));
        match self {
            Self::Circle { radius, .. } if !(*radius >= 0.0 && radius.is_finite()) => {
                fail("a circle needs a radius >= 0")
            }
            Self::Path { points, width } if points.len() < 2 || width.is_nan() || *width < 0.0 => {
                fail("a path needs two points and a width >= 0")
            }
            Self::Polygon { points } if points.len() < 3 => fail("a polygon needs three points"),
            _ => Ok(()),
        }
    }
}

/// Distance from `(x, z)` to the segment from `start` to `end`.
fn segment_distance(start: [f32; 2], end: [f32; 2], x: f32, z: f32) -> f32 {
    let (dx, dz) = (end[0] - start[0], end[1] - start[1]);
    let length2 = dx * dx + dz * dz;
    let along = if length2 > 0.0 {
        (((x - start[0]) * dx + (z - start[1]) * dz) / length2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (x - start[0] - along * dx).hypot(z - start[1] - along * dz)
}

/// Distance from `(x, z)` to an open polyline.
fn polyline_distance(points: &[[f32; 2]], x: f32, z: f32) -> f32 {
    points
        .windows(2)
        .map(|w| segment_distance(w[0], w[1], x, z))
        .fold(f32::INFINITY, f32::min)
}

/// Distance from `(x, z)` to the boundary of a closed polygon.
fn closed_distance(points: &[[f32; 2]], x: f32, z: f32) -> f32 {
    let last = points[points.len() - 1];
    polyline_distance(points, x, z).min(segment_distance(last, points[0], x, z))
}

/// Even-odd point-in-polygon test.
fn inside_polygon(points: &[[f32; 2]], x: f32, z: f32) -> bool {
    let mut inside = false;
    let mut before = points[points.len() - 1];
    for &here in points {
        let crosses = (here[1] > z) != (before[1] > z);
        if crosses && x < (before[0] - here[0]) * (z - here[1]) / (before[1] - here[1]) + here[0] {
            inside = !inside;
        }
        before = here;
    }
    inside
}

/// One excluded shape with its soft edge.
#[derive(Debug, Clone, PartialEq)]
pub struct Exclusion {
    /// The shape.
    pub shape: Shape,
    /// Width of the soft edge outside the shape, in meters.
    pub falloff: f32,
    /// Depth by which a path lowers the terrain along its centre line, in meters
    /// (a river bed); 0 leaves the terrain alone.
    pub carve: f32,
}

/// A raster mask over the terrain rectangle: values in `[0, 1]`, row-major, the top row
/// at the smallest `z` as for heightmaps.
#[derive(Debug, Clone, PartialEq)]
pub struct RasterMask {
    columns: usize,
    rows: usize,
    values: Vec<f32>,
}

impl RasterMask {
    /// A mask of `columns * rows` values.
    ///
    /// # Errors
    /// [`ForestError::Exclusion`] when the count does not match or is empty.
    pub fn new(columns: usize, rows: usize, values: Vec<f32>) -> Result<Self, ForestError> {
        if columns == 0 || rows == 0 || values.len() != columns * rows {
            return Err(ForestError::Exclusion(format!(
                "{} mask values for {columns} x {rows}",
                values.len()
            )));
        }
        Ok(Self {
            columns,
            rows,
            values,
        })
    }

    /// Reads a grayscale PNG mask (8 or 16 bits).
    ///
    /// # Errors
    /// [`ForestError::Exclusion`] when it cannot be read.
    pub fn from_png(path: &Path) -> Result<Self, ForestError> {
        let terrain = Terrain::from_png(path, [0.0, 0.0], [1.0, 1.0], [0.0, 1.0])
            .map_err(|e| ForestError::Exclusion(e.to_string()))?;
        let [columns, rows] = terrain.samples();
        Self::new(columns, rows, terrain.heights().to_vec())
    }

    /// The value nearest to the relative position `(u, v)` in `[0, 1]^2`.
    fn at(&self, u: f32, v: f32) -> f32 {
        let i = ((u * self.columns as f32) as usize).min(self.columns - 1);
        let j = ((v * self.rows as f32) as usize).min(self.rows - 1);
        self.values[j * self.columns + i]
    }
}

/// Every exclusion of a forest.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Exclusions {
    /// Shapes, each with its soft edge.
    pub shapes: Vec<Exclusion>,
    /// A painted mask over the terrain rectangle.
    pub mask: Option<RasterMask>,
    /// Points whose ground is below this height are water.
    pub water_level: Option<f32>,
    bounds: Vec<[f32; 4]>,
}

impl Exclusions {
    /// The exclusions, checked.
    ///
    /// # Errors
    /// [`ForestError::Exclusion`] for an invalid shape or a negative falloff.
    pub fn new(
        shapes: Vec<Exclusion>,
        mask: Option<RasterMask>,
        water_level: Option<f32>,
    ) -> Result<Self, ForestError> {
        for exclusion in &shapes {
            exclusion.shape.validate()?;
            if !(exclusion.falloff >= 0.0 && exclusion.carve >= 0.0) {
                return Err(ForestError::Exclusion(
                    "falloff and carve must be >= 0".to_owned(),
                ));
            }
        }
        let bounds = shapes.iter().map(|e| e.shape.bounds(e.falloff)).collect();
        Ok(Self {
            shapes,
            mask,
            water_level,
            bounds,
        })
    }

    /// Density factor at `(x, z)` on `terrain`: 0 excluded, 1 free.
    #[must_use]
    pub fn factor(&self, terrain: &Terrain, x: f32, z: f32) -> f32 {
        let mut factor = 1.0;
        for (exclusion, b) in self.shapes.iter().zip(&self.bounds) {
            if x < b[0] || z < b[1] || x > b[2] || z > b[3] {
                continue;
            }
            let distance = exclusion.shape.distance(x, z);
            if distance <= 0.0 {
                return 0.0;
            }
            if distance < exclusion.falloff {
                factor *= distance / exclusion.falloff;
            }
        }
        if let Some(mask) = &self.mask {
            let [u0, v0] = terrain.origin();
            let [w, d] = terrain.size();
            factor *= 1.0 - mask.at((x - u0) / w, (z - v0) / d).clamp(0.0, 1.0);
        }
        if let Some(level) = self.water_level
            && terrain.height(x, z) < level
        {
            return 0.0;
        }
        factor
    }

    /// Lowers `terrain` along the paths that carve: by `carve` on the centre line,
    /// easing to nothing at the edge of the band plus its falloff.
    pub fn carve(&self, terrain: &mut Terrain) {
        let [columns, rows] = terrain.samples();
        for exclusion in self.shapes.iter().filter(|e| e.carve > 0.0) {
            let Shape::Path { points, width } = &exclusion.shape else {
                continue;
            };
            let reach = width / 2.0 + exclusion.falloff;
            let b = exclusion.shape.bounds(exclusion.falloff);
            for index in 0..rows * columns {
                let [x, z] = terrain.sample_position(index % columns, index / columns);
                if x < b[0] || z < b[1] || x > b[2] || z > b[3] {
                    continue;
                }
                let t = (polyline_distance(points, x, z) / reach.max(1e-6)).min(1.0);
                let ease = 0.5 + 0.5 * (std::f32::consts::PI * t).cos();
                terrain.heights_mut()[index] -= exclusion.carve * ease;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Exclusion, Exclusions, Shape};
    use crate::terrain::Terrain;

    #[test]
    fn soft_edges_ramp() {
        let circle = Exclusion {
            shape: Shape::Circle {
                center: [0.0, 0.0],
                radius: 10.0,
            },
            falloff: 4.0,
            carve: 0.0,
        };
        let exclusions = Exclusions::new(vec![circle], None, None).unwrap();
        let terrain = Terrain::flat([-50.0, -50.0], [100.0, 100.0], 0.0);
        assert_eq!(exclusions.factor(&terrain, 5.0, 0.0), 0.0);
        assert!((exclusions.factor(&terrain, 12.0, 0.0) - 0.5).abs() < 1e-5);
        assert_eq!(exclusions.factor(&terrain, 20.0, 0.0), 1.0);
    }

    #[test]
    fn polygons_and_paths() {
        let square = Shape::Polygon {
            points: vec![[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]],
        };
        assert_eq!(square.distance(2.0, 2.0), 0.0);
        assert!((square.distance(6.0, 2.0) - 2.0).abs() < 1e-6);
        let road = Shape::Path {
            points: vec![[0.0, 0.0], [10.0, 0.0]],
            width: 2.0,
        };
        assert_eq!(road.distance(5.0, 0.5), 0.0);
        assert!((road.distance(5.0, 3.0) - 2.0).abs() < 1e-6);
    }
}

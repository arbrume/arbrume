//! Terrain: a regular grid of heights over a rectangle of the ground, Y up.
//!
//! Sample `(i, j)` (column `i`, row `j`) lies at `x = origin[0] + i * size[0] / (columns - 1)`
//! and `z = origin[1] + j * size[1] / (rows - 1)`; heights are interpolated bilinearly in
//! between and clamped to the edge outside. Slopes come from the gradient of that bilinear
//! surface. A heightmap image maps its top row to the smallest `z` and its left column to
//! the smallest `x`, so an image seen from above with `+x` to the right has `+z` downwards.

use std::path::Path;

use arbrume_core::rng::mix64;

use crate::error::ForestError;

/// Heights on a regular grid.
#[derive(Debug, Clone, PartialEq)]
pub struct Terrain {
    origin: [f32; 2],
    size: [f32; 2],
    columns: usize,
    rows: usize,
    heights: Vec<f32>,
}

/// Procedural hills: a sum of `octaves` layers of smooth value noise.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hills {
    /// Height difference between the lowest and highest possible point, in meters.
    pub amplitude: f32,
    /// Distance between two hills of the first layer, in meters.
    pub wavelength: f32,
    /// Number of layers, each at half the wavelength and half the amplitude of the last.
    pub octaves: u32,
    /// Distance between two height samples, in meters.
    pub resolution: f32,
    /// Seed of the noise.
    pub seed: u64,
}

impl Terrain {
    /// A flat terrain at `height`.
    #[must_use]
    pub fn flat(origin: [f32; 2], size: [f32; 2], height: f32) -> Self {
        Self {
            origin,
            size,
            columns: 2,
            rows: 2,
            heights: vec![height; 4],
        }
    }

    /// A terrain from `columns * rows` heights in row-major order (rows along `z`).
    ///
    /// # Errors
    /// [`ForestError::Terrain`] when there are fewer than 2 samples along an axis, the
    /// count does not match, a height is not finite or the size is not positive.
    pub fn from_grid(
        origin: [f32; 2],
        size: [f32; 2],
        columns: usize,
        rows: usize,
        heights: Vec<f32>,
    ) -> Result<Self, ForestError> {
        let fail = |reason: String| Err(ForestError::Terrain(reason));
        if columns < 2 || rows < 2 {
            return fail(format!(
                "a grid of {columns} x {rows} samples needs 2 per axis"
            ));
        }
        if heights.len() != columns * rows {
            return fail(format!(
                "{} heights for {columns} x {rows} samples",
                heights.len()
            ));
        }
        if !size.iter().chain(&origin).all(|v| v.is_finite()) || size.iter().any(|v| *v <= 0.0) {
            return fail(format!("size {size:?} is not positive and finite"));
        }
        if !heights.iter().all(|h| h.is_finite()) {
            return fail("a height is not finite".to_owned());
        }
        Ok(Self {
            origin,
            size,
            columns,
            rows,
            heights,
        })
    }

    /// A terrain from a grayscale PNG (8 or 16 bits): black is `range[0]` meters, white
    /// `range[1]`.
    ///
    /// # Errors
    /// [`ForestError::Terrain`] when the file cannot be read or is not a grayscale PNG.
    pub fn from_png(
        path: &Path,
        origin: [f32; 2],
        size: [f32; 2],
        range: [f32; 2],
    ) -> Result<Self, ForestError> {
        let (columns, rows, values) = read_gray_png(path)?;
        let heights = values
            .into_iter()
            .map(|v| range[0] + v * (range[1] - range[0]))
            .collect();
        Self::from_grid(origin, size, columns, rows, heights)
    }

    /// Procedural hills over the rectangle.
    ///
    /// # Errors
    /// [`ForestError::Terrain`] for a resolution or wavelength that is not positive.
    pub fn hills(origin: [f32; 2], size: [f32; 2], hills: &Hills) -> Result<Self, ForestError> {
        if !(hills.resolution > 0.0 && hills.wavelength > 0.0) {
            return Err(ForestError::Terrain(
                "hills need a positive resolution and wavelength".to_owned(),
            ));
        }
        let columns = (size[0] / hills.resolution).ceil() as usize + 1;
        let rows = (size[1] / hills.resolution).ceil() as usize + 1;
        let mut heights = Vec::with_capacity(columns * rows);
        for j in 0..rows {
            for i in 0..columns {
                let x = origin[0] + size[0] * i as f32 / (columns - 1) as f32;
                let z = origin[1] + size[1] * j as f32 / (rows - 1) as f32;
                heights.push(fractal_noise(x, z, hills));
            }
        }
        Self::from_grid(origin, size, columns, rows, heights)
    }

    /// Smallest `x` and `z`.
    #[must_use]
    pub const fn origin(&self) -> [f32; 2] {
        self.origin
    }

    /// Extent along `x` and `z`.
    #[must_use]
    pub const fn size(&self) -> [f32; 2] {
        self.size
    }

    /// Samples along `x` and along `z`.
    #[must_use]
    pub const fn samples(&self) -> [usize; 2] {
        [self.columns, self.rows]
    }

    /// The heights, row-major.
    #[must_use]
    pub fn heights(&self) -> &[f32] {
        &self.heights
    }

    /// Mutable heights, row-major.
    pub fn heights_mut(&mut self) -> &mut [f32] {
        &mut self.heights
    }

    /// Position of sample `(i, j)` in the ground plane.
    #[must_use]
    pub fn sample_position(&self, i: usize, j: usize) -> [f32; 2] {
        [
            self.origin[0] + self.size[0] * i as f32 / (self.columns - 1) as f32,
            self.origin[1] + self.size[1] * j as f32 / (self.rows - 1) as f32,
        ]
    }

    /// `true` when `(x, z)` is inside the rectangle.
    #[must_use]
    pub fn contains(&self, x: f32, z: f32) -> bool {
        let (u, v) = (x - self.origin[0], z - self.origin[1]);
        (0.0..=self.size[0]).contains(&u) && (0.0..=self.size[1]).contains(&v)
    }

    /// The cell of `(x, z)` and the position inside it, both clamped to the grid.
    fn locate(&self, x: f32, z: f32) -> (usize, usize, f32, f32) {
        let gx = ((x - self.origin[0]) / self.size[0] * (self.columns - 1) as f32)
            .clamp(0.0, (self.columns - 1) as f32);
        let gz = ((z - self.origin[1]) / self.size[1] * (self.rows - 1) as f32)
            .clamp(0.0, (self.rows - 1) as f32);
        let i = (gx as usize).min(self.columns - 2);
        let j = (gz as usize).min(self.rows - 2);
        (i, j, gx - i as f32, gz - j as f32)
    }

    /// The four heights of cell `(i, j)`: `(i, j)`, `(i + 1, j)`, `(i, j + 1)`, `(i + 1, j + 1)`.
    fn corners(&self, i: usize, j: usize) -> [f32; 4] {
        let at = |i: usize, j: usize| self.heights[j * self.columns + i];
        [at(i, j), at(i + 1, j), at(i, j + 1), at(i + 1, j + 1)]
    }

    /// Height at `(x, z)`, bilinear.
    #[must_use]
    pub fn height(&self, x: f32, z: f32) -> f32 {
        let (i, j, fx, fz) = self.locate(x, z);
        let [h00, h10, h01, h11] = self.corners(i, j);
        let low = h00 + (h10 - h00) * fx;
        let high = h01 + (h11 - h01) * fx;
        low + (high - low) * fz
    }

    /// Gradient `(dh/dx, dh/dz)` of the bilinear surface at `(x, z)`.
    #[must_use]
    pub fn gradient(&self, x: f32, z: f32) -> [f32; 2] {
        let (i, j, fx, fz) = self.locate(x, z);
        let [h00, h10, h01, h11] = self.corners(i, j);
        let dx = self.size[0] / (self.columns - 1) as f32;
        let dz = self.size[1] / (self.rows - 1) as f32;
        let along_x = ((h10 - h00) * (1.0 - fz) + (h11 - h01) * fz) / dx;
        let along_z = ((h01 - h00) * (1.0 - fx) + (h11 - h10) * fx) / dz;
        [along_x, along_z]
    }

    /// Slope at `(x, z)`, in degrees from the horizontal.
    #[must_use]
    pub fn slope_deg(&self, x: f32, z: f32) -> f32 {
        let [gx, gz] = self.gradient(x, z);
        gx.hypot(gz).atan().to_degrees()
    }

    /// Lowest and highest height of the grid.
    #[must_use]
    pub fn height_range(&self) -> [f32; 2] {
        self.heights
            .iter()
            .fold([f32::INFINITY, f32::NEG_INFINITY], |[lo, hi], h| {
                [lo.min(*h), hi.max(*h)]
            })
    }
}

/// Reads a grayscale PNG as values in `[0, 1]`, row-major.
fn read_gray_png(path: &Path) -> Result<(usize, usize, Vec<f32>), ForestError> {
    let fail = |e: &dyn std::fmt::Display| ForestError::Terrain(format!("{}: {e}", path.display()));
    let file = std::fs::File::open(path).map_err(|e| fail(&e))?;
    let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info().map_err(|e| fail(&e))?;
    let mut buffer = vec![
        0;
        reader
            .output_buffer_size()
            .ok_or_else(|| fail(&"too large"))?
    ];
    let frame = reader.next_frame(&mut buffer).map_err(|e| fail(&e))?;
    let (columns, rows) = (frame.width as usize, frame.height as usize);
    let channels = frame.color_type.samples();
    if !matches!(
        frame.color_type,
        png::ColorType::Grayscale | png::ColorType::GrayscaleAlpha
    ) {
        return Err(fail(&"not a grayscale image"));
    }
    let bytes = &buffer[..frame.buffer_size()];
    let values = match frame.bit_depth {
        png::BitDepth::Sixteen => bytes
            .chunks_exact(2 * channels)
            .map(|p| f32::from(u16::from_be_bytes([p[0], p[1]])) / 65535.0)
            .collect(),
        _ => bytes
            .chunks_exact(channels)
            .map(|p| f32::from(p[0]) / 255.0)
            .collect(),
    };
    Ok((columns, rows, values))
}

/// Value noise in `[0, 1]` at lattice coordinates `(x, z)`, smooth between the nodes.
fn value_noise(x: f32, z: f32, seed: u64) -> f32 {
    let (ix, iz) = (x.floor(), z.floor());
    let (fx, fz) = (x - ix, z - iz);
    let node = |dx: i64, dz: i64| {
        let key = (ix as i64 + dx) as u64 ^ ((iz as i64 + dz) as u64).rotate_left(32);
        (mix64(seed ^ mix64(key)) >> 40) as f32 / (1u64 << 24) as f32
    };
    let smooth = |t: f32| t * t * (3.0 - 2.0 * t);
    let (sx, sz) = (smooth(fx), smooth(fz));
    let low = node(0, 0) + (node(1, 0) - node(0, 0)) * sx;
    let high = node(0, 1) + (node(1, 1) - node(0, 1)) * sx;
    low + (high - low) * sz
}

/// The height of [`Hills`] at `(x, z)`, between 0 and the amplitude.
fn fractal_noise(x: f32, z: f32, hills: &Hills) -> f32 {
    let (mut sum, mut weight, mut total) = (0.0, 1.0, 0.0);
    let mut frequency = 1.0 / hills.wavelength;
    for octave in 0..hills.octaves.max(1) {
        let seed = mix64(hills.seed.wrapping_add(u64::from(octave)));
        sum += weight * value_noise(x * frequency, z * frequency, seed);
        total += weight;
        weight *= 0.5;
        frequency *= 2.0;
    }
    hills.amplitude * sum / total
}

#[cfg(test)]
mod tests {
    use super::Terrain;

    #[test]
    fn a_plane_has_its_slope() {
        let heights = vec![0.0, 10.0, 0.0, 10.0];
        let terrain = Terrain::from_grid([0.0, 0.0], [10.0, 10.0], 2, 2, heights).unwrap();
        assert!((terrain.height(5.0, 3.0) - 5.0).abs() < 1e-5);
        assert!((terrain.slope_deg(2.0, 2.0) - 45.0).abs() < 1e-3);
        assert_eq!(terrain.height(-5.0, 0.0), 0.0);
    }
}

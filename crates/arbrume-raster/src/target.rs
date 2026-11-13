//! Render targets and 8-bit images.
//!
//! A [`RenderTarget`] holds the resolved result of a render at output resolution, in
//! linear floating point: shaded colour, surface albedo, world normal, depth and plant
//! coverage. An [`Image`] is the 8-bit sRGB RGBA picture made from its colour buffer,
//! ready for encoding.

/// Resolved buffers of one render, row-major with the first row at the top.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderTarget {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Linear RGBA colour, alpha-premultiplied. Alpha is 1 over an opaque background.
    pub color: Vec<[f32; 4]>,
    /// Linear albedo of the plant, averaged over the covered samples of each pixel.
    pub albedo: Vec<[f32; 3]>,
    /// Unit world normal of the plant facing the camera, averaged over the covered
    /// samples; zero where the plant does not cover the pixel.
    pub normal: Vec<[f32; 3]>,
    /// Clip-space depth `z / w` of the nearest plant sample, 1 where uncovered.
    pub depth: Vec<f32>,
    /// Fraction of the samples of each pixel covered by the plant (ground and background
    /// excluded).
    pub coverage: Vec<f32>,
}

impl RenderTarget {
    /// A cleared target: transparent black, zero albedo and normal, depth 1, no coverage.
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        let count = width as usize * height as usize;
        Self {
            width,
            height,
            color: vec![[0.0; 4]; count],
            albedo: vec![[0.0; 3]; count],
            normal: vec![[0.0; 3]; count],
            depth: vec![1.0; count],
            coverage: vec![0.0; count],
        }
    }

    /// The colour buffer as 8-bit sRGB with straight alpha.
    #[must_use]
    pub fn to_image(&self) -> Image {
        let lut = SrgbTable::new();
        let pixels = self
            .color
            .iter()
            .flat_map(|&[r, g, b, a]| {
                let inv = if a > 0.0 { 1.0 / a } else { 0.0 };
                [
                    lut.encode(r * inv),
                    lut.encode(g * inv),
                    lut.encode(b * inv),
                    (a.clamp(0.0, 1.0) * 255.0).round() as u8,
                ]
            })
            .collect();
        Image {
            width: self.width,
            height: self.height,
            pixels,
        }
    }
}

/// An 8-bit sRGB RGBA image with straight (not premultiplied) alpha.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `4 * width * height` bytes, row-major from the top row, RGBA order.
    pub pixels: Vec<u8>,
}

impl Image {
    /// The RGBA bytes of the pixel at column `x`, row `y`.
    ///
    /// # Panics
    ///
    /// Panics when the pixel is outside the image.
    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        assert!(x < self.width && y < self.height, "pixel outside the image");
        let i = 4 * (y as usize * self.width as usize + x as usize);
        [
            self.pixels[i],
            self.pixels[i + 1],
            self.pixels[i + 2],
            self.pixels[i + 3],
        ]
    }

    /// Whether every pixel is fully opaque.
    #[must_use]
    pub fn is_opaque(&self) -> bool {
        self.pixels.as_chunks::<4>().0.iter().all(|p| p[3] == 255)
    }
}

/// Number of entries of the linear-to-sRGB table.
const SRGB_STEPS: usize = 4096;

/// Lookup table from linear values in `[0, 1]` to 8-bit sRGB.
#[derive(Debug, Clone)]
pub(crate) struct SrgbTable {
    table: Vec<u8>,
}

impl SrgbTable {
    pub(crate) fn new() -> Self {
        let table = (0..SRGB_STEPS)
            .map(|i| {
                let linear = i as f32 / (SRGB_STEPS - 1) as f32;
                (linear_to_srgb(linear) * 255.0).round() as u8
            })
            .collect();
        Self { table }
    }

    /// The 8-bit sRGB code of a linear value, clamped to `[0, 1]`.
    pub(crate) fn encode(&self, linear: f32) -> u8 {
        let index = (linear.clamp(0.0, 1.0) * (SRGB_STEPS - 1) as f32).round() as usize;
        self.table[index]
    }
}

/// The sRGB transfer function (IEC 61966-2-1) of a linear value in `[0, 1]`.
#[must_use]
pub fn linear_to_srgb(linear: f32) -> f32 {
    if linear <= 0.003_130_8 {
        12.92 * linear
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}

/// The inverse of [`linear_to_srgb`].
#[must_use]
pub fn srgb_to_linear(encoded: f32) -> f32 {
    if encoded <= 0.040_45 {
        encoded / 12.92
    } else {
        ((encoded + 0.055) / 1.055).powf(2.4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgb_round_trips_and_table_matches_the_formula() {
        let lut = SrgbTable::new();
        for code in 0..=255_u8 {
            let linear = srgb_to_linear(f32::from(code) / 255.0);
            assert_eq!(lut.encode(linear), code);
        }
    }

    #[test]
    fn premultiplied_colour_is_unpremultiplied_in_images() {
        let mut target = RenderTarget::new(1, 1);
        target.color[0] = [0.25, 0.0, 0.5, 0.5];
        let image = target.to_image();
        assert_eq!(image.pixel(0, 0), [188, 0, 255, 128]);
    }
}

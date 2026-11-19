//! Textures of mesh parts: decoding and sampling.
//!
//! A part whose material carries a PNG base colour texture, and that has texture
//! coordinates, is drawn with it: the rasterizer discards the samples whose texel alpha
//! is below the alpha cutoff of the material (alpha masking), and the shader multiplies
//! the albedo by the texel colour. A PNG normal texture perturbs the shading normal in
//! the tangent frame of each triangle, derived from its texture coordinates (glTF
//! convention). Textures are decoded once per render; JPEG textures are ignored.
//! Sampling is bilinear with clamped edges, and `v` grows downwards in the image, as in
//! glTF.

use glam::{Vec2, Vec4};

/// A decoded RGBA texture, linear floating point.
#[derive(Debug, Clone, PartialEq)]
pub struct Texture {
    /// Width in texels.
    pub width: u32,
    /// Height in texels.
    pub height: u32,
    /// Texels, row-major from the top row.
    pub texels: Vec<Vec4>,
}

impl Texture {
    /// Decodes 8-bit RGBA, RGB, grey or grey-alpha PNG bytes. Colour channels are
    /// converted from sRGB to linear when `srgb`; alpha is always linear. `None` for
    /// anything else.
    #[must_use]
    pub fn from_png(bytes: &[u8], srgb: bool) -> Option<Self> {
        let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = decoder.read_info().ok()?;
        let mut data = vec![0; reader.output_buffer_size()?];
        let info = reader.next_frame(&mut data).ok()?;
        let channels = info.color_type.samples();
        let decode = |code: u8| {
            let value = f32::from(code) / 255.0;
            if srgb {
                crate::target::srgb_to_linear(value)
            } else {
                value
            }
        };
        let texels = data[..info.buffer_size()]
            .chunks_exact(channels)
            .map(|px| match channels {
                1 => Vec4::new(decode(px[0]), decode(px[0]), decode(px[0]), 1.0),
                2 => Vec4::new(
                    decode(px[0]),
                    decode(px[0]),
                    decode(px[0]),
                    f32::from(px[1]) / 255.0,
                ),
                3 => Vec4::new(decode(px[0]), decode(px[1]), decode(px[2]), 1.0),
                _ => Vec4::new(
                    decode(px[0]),
                    decode(px[1]),
                    decode(px[2]),
                    f32::from(px[3]) / 255.0,
                ),
            })
            .collect();
        Some(Self {
            width: info.width,
            height: info.height,
            texels,
        })
    }

    /// The texel at column `x`, row `y`, clamped to the image.
    fn texel(&self, x: i64, y: i64) -> Vec4 {
        let x = x.clamp(0, i64::from(self.width) - 1) as usize;
        let y = y.clamp(0, i64::from(self.height) - 1) as usize;
        self.texels[y * self.width as usize + x]
    }

    /// Bilinear sample at texture coordinates `uv`.
    #[must_use]
    pub fn sample(&self, uv: Vec2) -> Vec4 {
        if self.texels.is_empty() {
            return Vec4::ONE;
        }
        let x = uv.x * self.width as f32 - 0.5;
        let y = uv.y * self.height as f32 - 0.5;
        let (fx, fy) = (x.floor(), y.floor());
        let (tx, ty) = (x - fx, y - fy);
        let (ix, iy) = (fx as i64, fy as i64);
        let top = self.texel(ix, iy).lerp(self.texel(ix + 1, iy), tx);
        let bottom = self.texel(ix, iy + 1).lerp(self.texel(ix + 1, iy + 1), tx);
        top.lerp(bottom, ty)
    }
}

/// Alpha masking of the triangles of one draw call.
#[derive(Debug, Clone, Copy)]
pub struct AlphaTest<'a> {
    /// Texture coordinates of the vertices of the call.
    pub uvs: &'a [[f32; 2]],
    /// The texture whose alpha masks the triangles.
    pub texture: &'a Texture,
    /// Samples whose alpha is below this value are discarded.
    pub cutoff: f32,
}

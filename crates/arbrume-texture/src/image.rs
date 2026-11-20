//! 8-bit images, conversion from linear floating point and PNG encoding.

use glam::{Vec3, Vec4};
use std::sync::OnceLock;

/// An 8-bit image, row-major from the top row; `v` of texture coordinates grows
/// downwards, as in glTF.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    /// Width in texels.
    pub width: u32,
    /// Height in texels.
    pub height: u32,
    /// Channels per texel: 1 (grey), 3 (RGB) or 4 (RGBA).
    pub channels: u8,
    /// Whether the colour channels are sRGB-encoded (albedo) rather than linear data
    /// (normals, roughness).
    pub srgb: bool,
    /// `width * height * channels` bytes.
    pub data: Vec<u8>,
}

/// Number of entries of the linear-to-sRGB table.
const SRGB_STEPS: usize = 4096;

/// The sRGB transfer function (IEC 61966-2-1) of a linear value in `[0, 1]`.
fn linear_to_srgb(linear: f32) -> f32 {
    if linear <= 0.003_130_8 {
        12.92 * linear
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}

/// 8-bit sRGB code of a linear value, from a table.
pub(crate) fn srgb_code(linear: f32) -> u8 {
    static TABLE: OnceLock<Vec<u8>> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        (0..SRGB_STEPS)
            .map(|i| {
                let linear = i as f32 / (SRGB_STEPS - 1) as f32;
                (linear_to_srgb(linear) * 255.0).round() as u8
            })
            .collect()
    });
    table[(linear.clamp(0.0, 1.0) * (SRGB_STEPS - 1) as f32).round() as usize]
}

/// 8-bit code of a linear value in `[0, 1]`.
pub(crate) fn linear_code(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

impl Image {
    /// An sRGB albedo image from linear colours, with alpha when `alpha` is given.
    pub(crate) fn albedo(width: u32, colors: &[Vec3], alpha: Option<&[f32]>) -> Self {
        let channels = if alpha.is_some() { 4 } else { 3 };
        let mut data = Vec::with_capacity(colors.len() * channels);
        for (index, color) in colors.iter().enumerate() {
            data.extend(color.to_array().map(srgb_code));
            if let Some(alpha) = alpha {
                data.push(linear_code(alpha[index]));
            }
        }
        Self {
            width,
            height: (colors.len() / width as usize) as u32,
            channels: channels as u8,
            srgb: true,
            data,
        }
    }

    /// A tangent-space normal image (RGB, `x y z` mapped from `[-1, 1]` to `[0, 1]`).
    pub(crate) fn normals(width: u32, normals: &[Vec3]) -> Self {
        let data = normals
            .iter()
            .flat_map(|n| n.to_array().map(|c| linear_code(0.5 * c + 0.5)))
            .collect();
        Self {
            width,
            height: (normals.len() / width as usize) as u32,
            channels: 3,
            srgb: false,
            data,
        }
    }

    /// A linear grey image.
    pub(crate) fn grey(width: u32, values: &[f32]) -> Self {
        Self {
            width,
            height: (values.len() / width as usize) as u32,
            channels: 1,
            srgb: false,
            data: values.iter().map(|&v| linear_code(v)).collect(),
        }
    }

    /// The texel at column `x`, row `y` as floats in `[0, 1]`, still encoded; grey is
    /// repeated over RGB and a missing alpha is 1.
    #[must_use]
    pub fn texel(&self, x: u32, y: u32) -> Vec4 {
        let channels = usize::from(self.channels);
        let at = (y as usize * self.width as usize + x as usize) * channels;
        let px = &self.data[at..at + channels];
        let value = |byte: u8| f32::from(byte) / 255.0;
        match channels {
            1 => Vec4::new(value(px[0]), value(px[0]), value(px[0]), 1.0),
            3 => Vec4::new(value(px[0]), value(px[1]), value(px[2]), 1.0),
            _ => Vec4::new(value(px[0]), value(px[1]), value(px[2]), value(px[3])),
        }
    }

    /// The PNG encoding of the image; an sRGB image is marked as such.
    ///
    /// # Panics
    ///
    /// Never in practice: encoding into memory cannot fail for a well-formed image.
    #[must_use]
    pub fn to_png(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, self.width, self.height);
        encoder.set_color(match self.channels {
            1 => png::ColorType::Grayscale,
            3 => png::ColorType::Rgb,
            _ => png::ColorType::Rgba,
        });
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        if self.srgb {
            encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
        }
        let written = encoder
            .write_header()
            .and_then(|mut writer| writer.write_image_data(&self.data));
        written.expect("encoding a PNG into memory cannot fail");
        out
    }
}

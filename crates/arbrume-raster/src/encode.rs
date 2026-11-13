//! PNG and JPEG encoding of [`Image`]s.
//!
//! PNG keeps the alpha channel and is lossless. JPEG has no alpha: the image is
//! composited over a matte colour first. It is meant for small gallery thumbnails, where
//! it is several times smaller than PNG. Both encoders are pure Rust.

use crate::target::Image;
use std::fmt;
use std::io::Write;
use std::path::Path;

/// Failure to encode or write an image.
#[derive(Debug)]
pub enum EncodeError {
    /// The PNG encoder failed.
    Png(png::EncodingError),
    /// The JPEG encoder failed.
    Jpeg(jpeg_encoder::EncodingError),
    /// The image is larger than the format allows.
    TooLarge,
    /// Writing the file failed.
    Io(std::io::Error),
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Png(error) => write!(f, "PNG encoding failed: {error}"),
            Self::Jpeg(error) => write!(f, "JPEG encoding failed: {error}"),
            Self::TooLarge => write!(f, "image too large for the format"),
            Self::Io(error) => write!(f, "writing the image failed: {error}"),
        }
    }
}

impl std::error::Error for EncodeError {}

impl From<std::io::Error> for EncodeError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl Image {
    /// The image as an RGBA PNG file.
    ///
    /// # Errors
    ///
    /// Returns [`EncodeError::Png`] when the encoder fails (for example on a zero-sized
    /// image).
    pub fn to_png(&self) -> Result<Vec<u8>, EncodeError> {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, self.width, self.height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
            let mut writer = encoder.write_header().map_err(EncodeError::Png)?;
            writer
                .write_image_data(&self.pixels)
                .map_err(EncodeError::Png)?;
            writer.finish().map_err(EncodeError::Png)?;
        }
        Ok(bytes)
    }

    /// The image as a baseline JPEG of `quality` in `[1, 100]`, composited over the sRGB
    /// colour `matte`.
    ///
    /// # Errors
    ///
    /// Returns [`EncodeError::TooLarge`] for a side above 65535 pixels and
    /// [`EncodeError::Jpeg`] when the encoder fails.
    pub fn to_jpeg(&self, quality: u8, matte: [u8; 3]) -> Result<Vec<u8>, EncodeError> {
        let (Ok(width), Ok(height)) = (u16::try_from(self.width), u16::try_from(self.height))
        else {
            return Err(EncodeError::TooLarge);
        };
        let rgb: Vec<u8> = self
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| {
                let a = u32::from(p[3]);
                let mix = |c: u8, m: u8| {
                    ((u32::from(c) * a + u32::from(m) * (255 - a) + 127) / 255) as u8
                };
                [
                    mix(p[0], matte[0]),
                    mix(p[1], matte[1]),
                    mix(p[2], matte[2]),
                ]
            })
            .collect();
        let mut bytes = Vec::new();
        let encoder = jpeg_encoder::Encoder::new(&mut bytes, quality.clamp(1, 100));
        encoder
            .encode(&rgb, width, height, jpeg_encoder::ColorType::Rgb)
            .map_err(EncodeError::Jpeg)?;
        Ok(bytes)
    }

    /// Writes the image as PNG to `path`.
    ///
    /// # Errors
    ///
    /// Returns an [`EncodeError`] when encoding or writing fails.
    pub fn write_png(&self, path: impl AsRef<Path>) -> Result<(), EncodeError> {
        let bytes = self.to_png()?;
        std::fs::File::create(path)?.write_all(&bytes)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(width: u32, height: u32) -> Image {
        let pixels = (0..width * height)
            .flat_map(|i| {
                [
                    (i % 256) as u8,
                    (i * 7 % 256) as u8,
                    40,
                    (i * 3 % 256) as u8,
                ]
            })
            .collect();
        Image {
            width,
            height,
            pixels,
        }
    }

    #[test]
    fn png_round_trips_exactly() {
        let image = gradient(37, 21);
        let bytes = image.to_png().unwrap();
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        let mut reader = decoder.read_info().unwrap();
        let mut buffer = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buffer).unwrap();
        assert_eq!((info.width, info.height), (37, 21));
        assert_eq!(info.color_type, png::ColorType::Rgba);
        assert_eq!(&buffer[..info.buffer_size()], &image.pixels[..]);
    }

    #[test]
    fn jpeg_has_markers_and_rejects_huge_sides() {
        let bytes = gradient(16, 16).to_jpeg(80, [255, 255, 255]).unwrap();
        assert_eq!(&bytes[..2], &[0xFF, 0xD8]);
        assert_eq!(&bytes[bytes.len() - 2..], &[0xFF, 0xD9]);
        let huge = Image {
            width: 70_000,
            height: 1,
            pixels: vec![0; 4 * 70_000],
        };
        assert!(matches!(
            huge.to_jpeg(80, [0; 3]),
            Err(EncodeError::TooLarge)
        ));
    }
}

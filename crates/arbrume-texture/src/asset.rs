//! Image textures: albedo and normal maps read from files (`bark.texture`,
//! `foliage.texture` and their `normal_texture`) instead of synthesized.
//!
//! * [`decode_image`] reads a PNG (8 or 16 bits, grey, grey and alpha, RGB, RGBA or
//!   palette) or a baseline JPEG (grey or RGB) into an 8-bit RGBA [`Image`].
//! * [`check_texture_size`] accepts a square image whose side is a power of two from
//!   `MIN_TEXTURE_RESOLUTION` to `MAX_TEXTURE_RESOLUTION`, the sizes a synthesized
//!   texture may have.
//! * [`read_images`] reads the images a species names, resolving relative paths with
//!   `Species::resolve_asset`; [`crate::load_textures`] makes the textures from them.
//!
//! **Bark.** The image is the albedo of one repeat of `bark.texture_size`, used at its
//! own size (halved until it fits `TextureOptions::max_resolution`). Without a normal
//! map the normal is flat; the roughness is the constant `bark.roughness`.
//!
//! **Leaves.** The image is one leaf over its rectangle (see `LeafLayout`), stalk at the
//! bottom centre and tip at the top, its alpha cutting the outline. It fills every leaf
//! cell of the atlas of the species, each texel the mean of the image pixels under it
//! (the nearest pixel when the cell is larger than the image); the needle spray of a
//! conifer twig stays synthesized. The texels around `LeafAtlas::STEM_UV` are made
//! opaque, in the colour of young bark where the image is transparent, since twig stems
//! are textured with that point. A normal map is flipped like the image: its `+y`, up
//! the image towards the tip, becomes `-y` in the atlas, whose `v` grows from the stalk.
//!
//! As for synthesized textures, the colour of an image counts relative to its mean: the
//! base colour factor of a material is its colour divided by that mean, so `bark.color`
//! and `foliage.color` still set the mean colour of the plant. Set them to the mean
//! colour of the image to keep it as it is.

use crate::image::{Image, srgb_code};
use crate::leaf::CellImage;
use arbrume_core::species::{LeafAtlas, MAX_TEXTURE_RESOLUTION, MIN_TEXTURE_RESOLUTION, Species};
use glam::{Vec3, Vec4};
use std::path::PathBuf;
use std::sync::OnceLock;
use thiserror::Error;

/// An image texture that cannot be read, decoded or used.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("texture {}: {reason}", path.display())]
pub struct TextureError {
    /// Path of the image file.
    pub path: PathBuf,
    /// What is wrong with it.
    pub reason: String,
}

/// The decoded images of a species, each an 8-bit RGBA [`Image`] from [`decode_image`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextureImages {
    /// Albedo of the bark (`bark.texture`).
    pub bark: Option<Image>,
    /// Normal map of the bark (`bark.normal_texture`).
    pub bark_normal: Option<Image>,
    /// One leaf with its alpha (`foliage.texture`).
    pub leaf: Option<Image>,
    /// Normal map of the leaf (`foliage.normal_texture`).
    pub leaf_normal: Option<Image>,
}

/// Decodes a PNG or JPEG file into an 8-bit RGBA image, sRGB-encoded.
///
/// # Errors
///
/// Returns why the bytes are not a supported image: another format, a corrupt file, a
/// CMYK JPEG, or a size outside [`check_texture_size`] (checked before decoding the
/// pixels).
pub fn decode_image(bytes: &[u8]) -> Result<Image, String> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        decode_png(bytes)
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        decode_jpeg(bytes)
    } else {
        Err("not a PNG or JPEG image".to_owned())
    }
}

/// Fails unless the image is square with a power-of-two side from
/// `MIN_TEXTURE_RESOLUTION` to `MAX_TEXTURE_RESOLUTION` texels.
///
/// # Errors
///
/// Returns the reason, naming the size.
pub fn check_texture_size(width: u32, height: u32) -> Result<(), String> {
    let range = MIN_TEXTURE_RESOLUTION..=MAX_TEXTURE_RESOLUTION;
    if width == height && width.is_power_of_two() && range.contains(&width) {
        return Ok(());
    }
    Err(format!(
        "{width} x {height} texels: an image texture is square, its side a power of two \
         from {MIN_TEXTURE_RESOLUTION} to {MAX_TEXTURE_RESOLUTION}"
    ))
}

/// Decodes a PNG into RGBA.
fn decode_png(bytes: &[u8]) -> Result<Image, String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| format!("PNG: {e}"))?;
    let (width, height) = (reader.info().width, reader.info().height);
    check_texture_size(width, height)?;
    let size = reader
        .output_buffer_size()
        .ok_or("PNG: the image is too large")?;
    let mut data = vec![0; size];
    let frame = reader
        .next_frame(&mut data)
        .map_err(|e| format!("PNG: {e}"))?;
    data.truncate(frame.buffer_size());
    let channels = match frame.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => return Err("PNG: palette was not expanded".to_owned()),
    };
    Ok(to_rgba(width, height, channels, &data))
}

/// Decodes a JPEG into RGBA.
fn decode_jpeg(bytes: &[u8]) -> Result<Image, String> {
    let mut decoder = jpeg_decoder::Decoder::new(bytes);
    decoder.read_info().map_err(|e| format!("JPEG: {e}"))?;
    let info = decoder.info().ok_or("JPEG: no image header")?;
    let (width, height) = (u32::from(info.width), u32::from(info.height));
    check_texture_size(width, height)?;
    let channels = match info.pixel_format {
        jpeg_decoder::PixelFormat::L8 => 1,
        jpeg_decoder::PixelFormat::RGB24 => 3,
        other => return Err(format!("JPEG: pixel format {other:?} is not grey or RGB")),
    };
    let data = decoder.decode().map_err(|e| format!("JPEG: {e}"))?;
    Ok(to_rgba(width, height, channels, &data))
}

/// An RGBA image from 8-bit pixels of 1 to 4 channels (grey, grey and alpha, RGB, RGBA).
fn to_rgba(width: u32, height: u32, channels: usize, data: &[u8]) -> Image {
    let rgba = data
        .chunks_exact(channels)
        .flat_map(|px| match channels {
            1 => [px[0], px[0], px[0], 255],
            2 => [px[0], px[0], px[0], px[1]],
            3 => [px[0], px[1], px[2], 255],
            _ => [px[0], px[1], px[2], px[3]],
        })
        .collect();
    Image {
        width,
        height,
        channels: 4,
        srgb: true,
        data: rgba,
    }
}

/// Reads and decodes the image textures of `species`.
///
/// # Errors
///
/// Returns a [`TextureError`] naming the file that cannot be read or decoded.
pub fn read_images(species: &Species) -> Result<TextureImages, TextureError> {
    let read = |path: &Option<String>| -> Result<Option<Image>, TextureError> {
        let Some(path) = path else {
            return Ok(None);
        };
        let path = species.resolve_asset(path);
        let error = |reason: String| TextureError {
            path: path.clone(),
            reason,
        };
        let bytes = std::fs::read(&path).map_err(|e| error(e.to_string()))?;
        decode_image(&bytes).map(Some).map_err(error)
    };
    let (bark, foliage) = (&species.bark, &species.foliage);
    Ok(TextureImages {
        bark: read(&bark.texture)?,
        bark_normal: read(&bark.normal_texture)?,
        leaf: read(&foliage.texture)?,
        leaf_normal: read(&foliage.normal_texture)?,
    })
}

/// 8-bit sRGB code to linear value.
fn srgb_to_linear(code: u8) -> f32 {
    static TABLE: OnceLock<[f32; 256]> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        std::array::from_fn(|code| {
            let encoded = code as f32 / 255.0;
            if encoded <= 0.040_45 {
                encoded / 12.92
            } else {
                ((encoded + 0.055) / 1.055).powf(2.4)
            }
        })
    });
    table[usize::from(code)]
}

/// Texel `(x, y)` of an RGBA image as linear colour and alpha (`srgb`) or as the
/// 0-to-1 values of its bytes.
fn texel(image: &Image, x: usize, y: usize, srgb: bool) -> Vec4 {
    let at = (y * image.width as usize + x) * 4;
    let px = &image.data[at..at + 4];
    let decode = |byte: u8| {
        if srgb {
            srgb_to_linear(byte)
        } else {
            f32::from(byte) / 255.0
        }
    };
    Vec4::new(
        decode(px[0]),
        decode(px[1]),
        decode(px[2]),
        f32::from(px[3]) / 255.0,
    )
}

/// The RGBA image at half its size, each texel the mean of four (in linear colour
/// when `srgb`).
fn halve(image: &Image, srgb: bool) -> Image {
    let (width, height) = ((image.width / 2).max(1), (image.height / 2).max(1));
    let mut data = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height as usize {
        for x in 0..width as usize {
            let mut sum = Vec4::ZERO;
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let sx = (2 * x + dx).min(image.width as usize - 1);
                sum += texel(image, sx, (2 * y + dy).min(image.height as usize - 1), srgb);
            }
            let mean = sum / 4.0;
            let encode = |value: f32| {
                if srgb {
                    srgb_code(value)
                } else {
                    (value.clamp(0.0, 1.0) * 255.0).round() as u8
                }
            };
            let alpha = (mean.w.clamp(0.0, 1.0) * 255.0).round() as u8;
            data.extend([encode(mean.x), encode(mean.y), encode(mean.z), alpha]);
        }
    }
    Image {
        width,
        height,
        channels: 4,
        srgb: image.srgb,
        data,
    }
}

/// `image` halved until its side is at most `max`.
pub(crate) fn fit(image: &Image, max: u32, srgb: bool) -> Image {
    let mut image = image.clone();
    while image.width > max.max(MIN_TEXTURE_RESOLUTION) {
        image = halve(&image, srgb);
    }
    image
}

/// The RGB image of an RGBA image (alpha dropped), sRGB-encoded or not.
pub(crate) fn rgb(image: &Image, srgb: bool) -> Image {
    Image {
        width: image.width,
        height: image.height,
        channels: 3,
        srgb,
        data: image
            .data
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|px| [px[0], px[1], px[2]])
            .collect(),
    }
}

/// Mean linear colour of an sRGB RGBA image.
pub(crate) fn mean_color(image: &Image) -> [f32; 3] {
    let mut sum = glam::DVec3::ZERO;
    for px in image.data.as_chunks::<4>().0 {
        let color = Vec3::new(
            srgb_to_linear(px[0]),
            srgb_to_linear(px[1]),
            srgb_to_linear(px[2]),
        );
        sum += color.as_dvec3();
    }
    let count = (image.data.len() / 4).max(1) as f64;
    (sum / count).as_vec3().max(Vec3::splat(1e-6)).to_array()
}

/// A flat tangent-space normal map, `MIN_TEXTURE_RESOLUTION` texels square.
pub(crate) fn flat_normal() -> Image {
    let side = MIN_TEXTURE_RESOLUTION;
    Image::normals(side, &vec![Vec3::Z; (side * side) as usize])
}

/// The leaf image and its normal map, as they fill the leaf cells of an atlas.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LeafImage<'a> {
    /// One leaf, RGBA, stalk at the bottom.
    pub albedo: &'a Image,
    /// Its normal map, if any.
    pub normal: Option<&'a Image>,
    /// Linear colour of the stalk texels the image leaves transparent.
    pub stem: Vec3,
}

/// Rows or columns `[first, last]` of a source of `count` pixels under the texel
/// interval `[a, b)` of a cell of `cells` texels, flipped when `flip`.
fn footprint(index: usize, cells: usize, count: usize, flip: bool) -> (usize, usize) {
    let (a, b) = (
        index as f32 / cells as f32,
        (index + 1) as f32 / cells as f32,
    );
    let (a, b) = if flip { (1.0 - b, 1.0 - a) } else { (a, b) };
    let first = ((a * count as f32).floor() as usize).min(count - 1);
    let last = (((b * count as f32).ceil() as usize).saturating_sub(1)).clamp(first, count - 1);
    (first, last)
}

impl LeafImage<'_> {
    /// The mean over the source pixels under texel `(x, y)` of a `size` cell: colour
    /// (linear, not premultiplied), alpha and normal.
    fn sample(&self, x: usize, y: usize, size: [usize; 2]) -> (Vec3, f32, Vec3) {
        let width = self.albedo.width as usize;
        let height = self.albedo.height as usize;
        let (x0, x1) = footprint(x, size[0], width, false);
        let (y0, y1) = footprint(y, size[1], height, true);
        let (mut premultiplied, mut alpha, mut normal) = (Vec3::ZERO, 0.0, Vec3::ZERO);
        for sy in y0..=y1 {
            for sx in x0..=x1 {
                let px = texel(self.albedo, sx, sy, true);
                premultiplied += px.truncate() * px.w;
                alpha += px.w;
                if let Some(map) = self.normal {
                    let map_x = sx * map.width as usize / width;
                    let map_y = sy * map.height as usize / height;
                    let n = texel(map, map_x, map_y, false).truncate() * 2.0 - Vec3::ONE;
                    normal += Vec3::new(n.x, -n.y, n.z);
                }
            }
        }
        let count = ((x1 - x0 + 1) * (y1 - y0 + 1)) as f32;
        let color = if alpha > 0.0 {
            premultiplied / alpha
        } else {
            Vec3::ZERO
        };
        let normal = normal.try_normalize().unwrap_or(Vec3::Z);
        (color, alpha / count, normal)
    }

    /// One leaf cell of `size` texels.
    pub(crate) fn cell(&self, size: [usize; 2]) -> CellImage {
        let texels = size[0] * size[1];
        let mut cell = CellImage {
            width: size[0],
            colors: Vec::with_capacity(texels),
            alpha: Vec::with_capacity(texels),
            normals: Vec::with_capacity(texels),
        };
        for y in 0..size[1] {
            for x in 0..size[0] {
                let (color, alpha, normal) = self.sample(x, y, size);
                cell.colors.push(color);
                cell.alpha.push(alpha);
                cell.normals.push(normal);
            }
        }
        self.open_stem(&mut cell, size);
        cell
    }

    /// Makes the texels at the stalk point `LeafAtlas::STEM_UV` opaque.
    fn open_stem(&self, cell: &mut CellImage, size: [usize; 2]) {
        let [u, v] = LeafAtlas::STEM_UV;
        let column = ((u * size[0] as f32) as usize).min(size[0] - 1);
        let rows = ((2.0 * v * size[1] as f32).ceil() as usize).clamp(1, size[1]);
        for y in 0..rows {
            for x in column.saturating_sub(1)..=column {
                let at = y * size[0] + x;
                if cell.alpha[at] < 0.5 {
                    cell.colors[at] = self.stem;
                }
                cell.alpha[at] = 1.0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{check_texture_size, decode_image, fit, footprint};
    use crate::image::Image;

    fn rgba(side: u32, pixel: [u8; 4]) -> Image {
        Image {
            width: side,
            height: side,
            channels: 4,
            srgb: true,
            data: pixel.repeat((side * side) as usize),
        }
    }

    #[test]
    fn png_round_trips_and_other_bytes_are_rejected() {
        let image = rgba(16, [200, 100, 50, 128]);
        let decoded = decode_image(&image.to_png()).unwrap();
        assert_eq!(decoded, image);
        assert!(
            decode_image(b"GIF89a")
                .unwrap_err()
                .contains("not a PNG or JPEG")
        );
        let mut broken = image.to_png();
        broken.truncate(40);
        assert!(decode_image(&broken).is_err());
    }

    #[test]
    fn sizes_must_be_square_powers_of_two_in_range() {
        assert!(check_texture_size(256, 256).is_ok());
        for (w, h) in [(256, 128), (100, 100), (8, 8), (8192, 8192)] {
            assert!(check_texture_size(w, h).is_err(), "{w} x {h}");
        }
        let wide = Image {
            width: 32,
            height: 16,
            data: vec![0; 32 * 16 * 4],
            ..rgba(16, [0; 4])
        };
        let error = decode_image(&wide.to_png()).unwrap_err();
        assert!(error.contains("32 x 16"), "{error}");
    }

    #[test]
    fn large_images_are_halved_to_fit() {
        let image = rgba(64, [255, 255, 255, 255]);
        let small = fit(&image, 16, true);
        assert_eq!((small.width, small.height), (16, 16));
        assert_eq!(small.data[..4], [255, 255, 255, 255]);
    }

    #[test]
    fn footprints_cover_the_source_and_flip() {
        assert_eq!(footprint(0, 4, 16, false), (0, 3));
        assert_eq!(footprint(3, 4, 16, false), (12, 15));
        assert_eq!(footprint(0, 4, 16, true), (12, 15));
        assert_eq!(footprint(5, 32, 16, false), (2, 2));
    }
}

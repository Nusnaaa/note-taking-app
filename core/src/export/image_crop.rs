//! Crop pixels rather than clipping a wrapper around a recoverable full image.
use crate::{objects::Rect, processing::JobControl, Error, Result};
use image::{ImageDecoder, ImageEncoder, ImageFormat, ImageReader};
use std::io::{self, Cursor, Write};
const DECODE_BYTES: u64 = 64 * 1024 * 1024;
const ENCODE_BYTES: usize = 32 * 1024 * 1024;
fn decode_error(error: image::ImageError) -> Error {
    if matches!(error, image::ImageError::Limits(_)) {
        Error::ResourceLimit
    } else {
        Error::Invalid("cropped image decode")
    }
}
struct Encoded(Vec<u8>);
impl Write for Encoded {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > ENCODE_BYTES.saturating_sub(self.0.len()) {
            return Err(io::Error::other("cropped image encoding limit"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
pub(super) fn crop_image(
    mime: &str,
    bytes: &[u8],
    crop: Rect,
    control: &JobControl,
) -> Result<(Vec<u8>, bool)> {
    control.check()?;
    crop.validate()?;
    if crop.x < 0. || crop.y < 0. || crop.x + crop.width > 1. || crop.y + crop.height > 1. {
        return Err(Error::Invalid("normalised image crop"));
    }
    if bytes.len() > ENCODE_BYTES {
        return Err(Error::ResourceLimit);
    }
    let format = match mime {
        "image/png" => ImageFormat::Png,
        "image/jpeg" => ImageFormat::Jpeg,
        _ => return Err(Error::Unsupported),
    };
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16384);
    limits.max_image_height = Some(16384);
    limits.max_alloc = Some(DECODE_BYTES);
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(decode_error)?;
    let (width, height) = decoder.dimensions();
    if width == 0
        || height == 0
        || decoder.total_bytes() > DECODE_BYTES
        || u64::from(width) * u64::from(height) * 4 > DECODE_BYTES
    {
        return Err(Error::ResourceLimit);
    }
    // No established H.I. orientation/crop convention exists yet. Refuse to
    // guess whether a stored crop was made before or after EXIF rotation.
    if decoder.orientation().map_err(decode_error)? != image::metadata::Orientation::NoTransforms {
        return Err(Error::ReviewRequired);
    }
    let edges = [
        crop.x * f64::from(width),
        crop.y * f64::from(height),
        (crop.x + crop.width) * f64::from(width),
        (crop.y + crop.height) * f64::from(height),
    ];
    let x = edges[0].ceil() as u32;
    let y = edges[1].ceil() as u32;
    let right = edges[2].floor() as u32;
    let bottom = edges[3].floor() as u32;
    if right <= x || bottom <= y {
        return Err(Error::ReviewRequired);
    }
    let rounded = edges.iter().any(|v| v.fract() != 0.);
    control.check()?;
    let decoded = image::DynamicImage::from_decoder(decoder).map_err(decode_error)?;
    control.check()?;
    let pixels = decoded.crop_imm(x, y, right - x, bottom - y).to_rgba8();
    let mut output = Encoded(Vec::new());
    image::codecs::png::PngEncoder::new(&mut output)
        .write_image(
            &pixels,
            right - x,
            bottom - y,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|_| Error::ResourceLimit)?;
    control.check()?;
    Ok((output.0, rounded))
}

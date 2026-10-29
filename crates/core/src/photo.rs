//! Patient portraits. Any upload is normalized to a small square JPEG before it is stored, so
//! the database never holds the original file (no EXIF/GPS, no full-size camera image).

use std::io::Cursor;

use image::{imageops::FilterType, DynamicImage, ImageDecoder, ImageReader, Limits};

use crate::{Error, Result};

/// Edge length of the stored square, in pixels: sharp at the 96 px header avatar on HiDPI.
pub const SIZE: u32 = 256;
/// Uploads larger than this are refused before decoding.
pub const MAX_UPLOAD: usize = 25 * 1024 * 1024;

/// Decodes JPEG/PNG/WebP, applies the EXIF orientation, crops the centre square and returns a
/// `SIZE`×`SIZE` JPEG. Metadata is dropped because only pixels are re-encoded.
pub fn normalize(bytes: &[u8]) -> Result<Vec<u8>> {
    if bytes.len() > MAX_UPLOAD {
        return Err(Error::Invalid("Das Bild ist zu groß (höchstens 25 MB).".into()));
    }
    let unreadable = || Error::Invalid("Bild konnte nicht gelesen werden (JPEG, PNG oder WebP).".into());
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format().map_err(|_| unreadable())?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(12_000);
    limits.max_image_height = Some(12_000);
    limits.max_alloc = Some(512 * 1024 * 1024);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(|_| unreadable())?;
    let orientation = decoder.orientation().ok();
    let mut img = DynamicImage::from_decoder(decoder).map_err(|_| unreadable())?;
    if let Some(o) = orientation {
        img.apply_orientation(o);
    }
    let side = img.width().min(img.height());
    if side == 0 {
        return Err(unreadable());
    }
    let square = img.crop_imm((img.width() - side) / 2, (img.height() - side) / 2, side, side);
    let small = square.resize_exact(SIZE, SIZE, FilterType::Lanczos3).to_rgb8();
    let mut out = Vec::new();
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85);
    small.write_with_encoder(encoder).map_err(|e| Error::Invalid(format!("Bild konnte nicht gespeichert werden: {e}")))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageFormat, Rgb, RgbImage};

    fn encoded(w: u32, h: u32, fmt: ImageFormat) -> Vec<u8> {
        let img = RgbImage::from_fn(w, h, |x, _| if x < w / 2 { Rgb([200, 30, 30]) } else { Rgb([30, 30, 200]) });
        let mut out = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(img).write_to(&mut out, fmt).unwrap();
        out.into_inner()
    }

    #[test]
    fn crops_to_a_small_square_jpeg() {
        for fmt in [ImageFormat::Png, ImageFormat::Jpeg] {
            let jpeg = normalize(&encoded(900, 500, fmt)).unwrap();
            assert_eq!(&jpeg[..2], &[0xFF, 0xD8], "JPEG magic");
            let back = image::load_from_memory(&jpeg).unwrap();
            assert_eq!((back.width(), back.height()), (SIZE, SIZE));
        }
    }

    #[test]
    fn refuses_garbage_and_oversize() {
        assert!(normalize(b"not an image").is_err());
        assert!(normalize(&vec![0u8; MAX_UPLOAD + 1]).is_err());
    }
}

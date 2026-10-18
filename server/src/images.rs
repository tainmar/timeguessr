//! Upload pipeline: decode (with limits), apply EXIF orientation, resize and re-encode
//! as JPEG. Re-encoding strips all metadata, so players can't read the answer
//! (GPS position, capture date) from the served file.

use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};

pub const MAX_UPLOAD_BYTES: usize = 20 * 1024 * 1024;
const MAX_DIMENSION: u32 = 10_000;
const MAX_DECODE_ALLOC: u64 = 256 * 1024 * 1024;
const FULL_SIZE: u32 = 2048;
const THUMB_SIZE: u32 = 480;

pub struct ProcessedImage {
    pub image: Vec<u8>,
    pub thumb: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum ImageProcessError {
    #[error("{0}")]
    Unsupported(String),
    #[error("This image is too large. Please use a photo under 10,000 pixels wide.")]
    TooLarge,
    #[error("We couldn't read this image ({0}). Please try another file.")]
    Invalid(String),
}

fn is_heic(bytes: &[u8]) -> bool {
    bytes.len() > 12
        && &bytes[4..8] == b"ftyp"
        && matches!(
            &bytes[8..12],
            b"heic" | b"heix" | b"hevc" | b"hevx" | b"mif1" | b"msf1" | b"avif"
        )
}

pub fn process(bytes: &[u8]) -> Result<ProcessedImage, ImageProcessError> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| ImageProcessError::Invalid(e.to_string()))?;
    match reader.format() {
        Some(ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::WebP | ImageFormat::Gif) => {}
        _ if is_heic(bytes) => {
            return Err(ImageProcessError::Unsupported(
                "HEIC/AVIF photos aren't supported by this browser upload. Please export the photo as JPEG and try again.".into(),
            ))
        }
        _ => {
            return Err(ImageProcessError::Unsupported(
                "Unsupported image format. Please upload a JPEG, PNG or WebP photo.".into(),
            ))
        }
    }

    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    reader.limits(limits);

    let mut decoder = reader.into_decoder().map_err(map_decode_error)?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(decoder).map_err(map_decode_error)?;
    img.apply_orientation(orientation);

    let full = if img.width() > FULL_SIZE || img.height() > FULL_SIZE {
        img.resize(FULL_SIZE, FULL_SIZE, FilterType::CatmullRom)
    } else {
        img.clone()
    };
    let thumb = img.resize(THUMB_SIZE, THUMB_SIZE, FilterType::Triangle);
    drop(img);

    Ok(ProcessedImage {
        width: full.width(),
        height: full.height(),
        image: encode_jpeg(&full, 85)?,
        thumb: encode_jpeg(&thumb, 78)?,
    })
}

fn map_decode_error(e: image::ImageError) -> ImageProcessError {
    match e {
        image::ImageError::Limits(_) => ImageProcessError::TooLarge,
        image::ImageError::Unsupported(u) => ImageProcessError::Unsupported(format!(
            "Unsupported image ({u}). Please upload a JPEG, PNG or WebP photo."
        )),
        other => ImageProcessError::Invalid(other.to_string()),
    }
}

fn encode_jpeg(img: &DynamicImage, quality: u8) -> Result<Vec<u8>, ImageProcessError> {
    let rgb = img.to_rgb8();
    let mut out = Vec::with_capacity(512 * 1024);
    JpegEncoder::new_with_quality(&mut out, quality)
        .encode_image(&rgb)
        .map_err(|e| ImageProcessError::Invalid(e.to_string()))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    fn sample_png(w: u32, h: u32) -> Vec<u8> {
        let img = RgbImage::from_fn(w, h, |x, y| Rgb([(x % 255) as u8, (y % 255) as u8, 128]));
        let mut out = Vec::new();
        DynamicImage::ImageRgb8(img)
            .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
            .unwrap();
        out
    }

    #[test]
    fn resizes_and_reencodes() {
        let processed = process(&sample_png(3000, 1500)).unwrap();
        assert_eq!((processed.width, processed.height), (2048, 1024));
        assert_eq!(&processed.image[..2], &[0xFF, 0xD8]); // JPEG magic
        assert_eq!(&processed.thumb[..2], &[0xFF, 0xD8]);
    }

    #[test]
    fn keeps_small_images() {
        let processed = process(&sample_png(640, 480)).unwrap();
        assert_eq!((processed.width, processed.height), (640, 480));
    }

    #[test]
    fn rejects_garbage_and_heic() {
        assert!(matches!(
            process(b"not an image at all"),
            Err(ImageProcessError::Unsupported(_))
        ));
        let mut heic = vec![0, 0, 0, 24];
        heic.extend_from_slice(b"ftypheic");
        heic.extend_from_slice(&[0; 32]);
        match process(&heic) {
            Err(ImageProcessError::Unsupported(msg)) => assert!(msg.contains("HEIC")),
            _ => panic!("expected HEIC rejection"),
        }
    }
}

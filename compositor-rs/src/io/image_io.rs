//! Reading and writing ordinary image files.
//!
//! Files on disk are straight (non-premultiplied) sRGB; the document is premultiplied, so both
//! directions convert here and nowhere else. Where the original app uses ImageIO, this uses the
//! `image` crate, which covers the formats the Linux build offers.

use std::path::{Path, PathBuf};

use crate::core::pixel::{PremulImage, Rgba};

/// Everything that can go wrong reading or writing an image.
#[derive(Debug, thiserror::Error)]
pub enum ImageError {
    #[error("无法读取 {path}：{source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("无法打开 {path}（格式不受支持）：{source}")]
    Decode {
        path: PathBuf,
        source: image::ImageError,
    },
    #[error("无法写入 {path}：{source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("无法编码 {path}：{source}")]
    Encode {
        path: PathBuf,
        source: image::ImageError,
    },
    #[error("{path} 没有扩展名，无法判断格式")]
    UnknownFormat { path: PathBuf },
    #[error("本版本不支持写出 {0} 格式")]
    UnsupportedFormat(String),
}

/// The extensions the Open and Import dialogs offer.
pub const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "webp", "tif", "tiff", "bmp", "gif", "psd", "psb", "comp",
];

/// The extensions this build writes.
pub const EXPORT_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "tif", "tiff", "bmp"];

/// True when the path looks like an image this build can open.
#[must_use]
pub fn is_supported_image(path: &Path) -> bool {
    extension(path).is_some_and(|extension| IMAGE_EXTENSIONS.contains(&extension.as_str()))
}

/// The lower-case extension, without the dot.
#[must_use]
pub fn extension(path: &Path) -> Option<String> {
    path.extension()
        .map(|extension| extension.to_string_lossy().to_lowercase())
}

/// Reads any supported image file into premultiplied pixels.
///
/// # Errors
///
/// Fails when the file cannot be read or the bytes are not an image this build decodes.
pub fn load(path: &Path) -> Result<PremulImage, ImageError> {
    let bytes = std::fs::read(path).map_err(|source| ImageError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    decode(&bytes, path)
}

/// Decodes image bytes, using `path` only for error messages.
///
/// # Errors
///
/// Fails when the bytes are not an image this build decodes.
pub fn decode(bytes: &[u8], path: &Path) -> Result<PremulImage, ImageError> {
    let decoded = image::load_from_memory(bytes).map_err(|source| ImageError::Decode {
        path: path.to_path_buf(),
        source,
    })?;
    let rgba = decoded.to_rgba8();
    PremulImage::from_straight(rgba.width(), rgba.height(), rgba.as_raw()).map_err(|error| {
        ImageError::Decode {
            path: path.to_path_buf(),
            source: image::ImageError::IoError(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                error.to_string(),
            )),
        }
    })
}

/// Writes the image as it is, with its transparency.
///
/// # Errors
///
/// Fails when the format is unknown, the file cannot be written, or encoding fails.
pub fn save(image: &PremulImage, path: &Path) -> Result<(), ImageError> {
    let format = extension(path).ok_or_else(|| ImageError::UnknownFormat {
        path: path.to_path_buf(),
    })?;
    match format.as_str() {
        "png" => save_png(image, path),
        "jpg" | "jpeg" => save_jpeg(image, path, 90, [255, 255, 255, 255]),
        "webp" => save_with(image, path, image::ImageFormat::WebP),
        "tif" | "tiff" => save_with(image, path, image::ImageFormat::Tiff),
        "bmp" => save_with(image, path, image::ImageFormat::Bmp),
        other => Err(ImageError::UnsupportedFormat(other.to_owned())),
    }
}

/// Writes a PNG, the format layer images and exports use.
///
/// # Errors
///
/// Fails when the file cannot be written or encoding fails.
pub fn save_png(image: &PremulImage, path: &Path) -> Result<(), ImageError> {
    save_with(image, path, image::ImageFormat::Png)
}

/// Writes a JPEG, which has no transparency, so the image is flattened over `background`.
///
/// # Errors
///
/// Fails when the file cannot be written or encoding fails.
pub fn save_jpeg(
    image: &PremulImage,
    path: &Path,
    quality: u8,
    background: Rgba,
) -> Result<(), ImageError> {
    let straight = image.to_straight();
    let mut flattened = image::RgbImage::new(image.width(), image.height());
    for (index, pixel) in straight.chunks_exact(4).enumerate() {
        let alpha = u32::from(pixel[3]);
        let over = |channel: usize| -> u8 {
            let source = u32::from(pixel[channel]) * alpha / 255;
            let backdrop = u32::from(background[channel]) * (255 - alpha) / 255;
            (source + backdrop).min(255) as u8
        };
        let x = index as u32 % image.width();
        let y = index as u32 / image.width();
        flattened.put_pixel(x, y, image::Rgb([over(0), over(1), over(2)]));
    }
    let file = std::fs::File::create(path).map_err(|source| ImageError::Write {
        path: path.to_path_buf(),
        source,
    })?;
    let mut writer = std::io::BufWriter::new(file);
    let mut encoder =
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut writer, quality.max(1));
    encoder
        .encode(
            flattened.as_raw(),
            flattened.width(),
            flattened.height(),
            image::ExtendedColorType::Rgb8,
        )
        .map_err(|source| ImageError::Encode {
            path: path.to_path_buf(),
            source,
        })
}

fn save_with(
    image: &PremulImage,
    path: &Path,
    format: image::ImageFormat,
) -> Result<(), ImageError> {
    let straight = image.to_straight();
    let buffer =
        image::RgbaImage::from_raw(image.width(), image.height(), straight).ok_or_else(|| {
            ImageError::Encode {
                path: path.to_path_buf(),
                source: image::ImageError::Parameter(image::error::ParameterError::from_kind(
                    image::error::ParameterErrorKind::DimensionMismatch,
                )),
            }
        })?;
    // Every failure here is an encode failure as far as the caller is concerned; the cause is
    // carried in the source error either way.
    buffer
        .save_with_format(path, format)
        .map_err(|source| ImageError::Encode {
            path: path.to_path_buf(),
            source,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> PremulImage {
        let mut image = PremulImage::new(4, 3);
        for y in 0..3 {
            for x in 0..4 {
                image.set_pixel(x, y, [40 * (x + 1) as u8, 20 * (y + 1) as u8, 90, 255]);
            }
        }
        image.set_pixel(0, 0, [0, 0, 0, 0]);
        image
    }

    #[test]
    fn png_round_trip_keeps_every_channel() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("round-trip.png");
        let original = sample();
        save_png(&original, &path).expect("save");
        let loaded = load(&path).expect("load");
        assert_eq!(loaded, original);
    }

    #[test]
    fn jpeg_flattens_transparency_without_keeping_alpha() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("flat.jpg");
        let original = PremulImage::filled(8, 8, [0, 0, 0, 0]);
        save_jpeg(&original, &path, 92, [255, 255, 255, 255]).expect("save");
        let loaded = load(&path).expect("load");
        let pixel = loaded.pixel(4, 4).expect("pixel");
        assert!(pixel[0] > 240, "the white backdrop should show: {pixel:?}");
        assert_eq!(pixel[3], 255);
    }

    #[test]
    fn an_unsupported_extension_is_named_in_the_error() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("picture.xyz");
        let error = save(&sample(), &path).expect_err("should fail");
        assert!(error.to_string().contains("xyz"), "{error}");
    }

    #[test]
    fn the_supported_extensions_cover_the_documented_formats() {
        for name in [
            "a.png", "a.JPG", "a.jpeg", "a.webp", "a.tif", "a.bmp", "a.gif", "a.psd",
        ] {
            assert!(
                is_supported_image(Path::new(name)),
                "{name} should be openable"
            );
        }
        assert!(!is_supported_image(Path::new("notes.txt")));
    }
}

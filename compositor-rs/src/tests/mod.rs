//! Helpers shared by the unit tests and the integration tests.
//!
//! Compiled into the library so `tests/*.rs` can build the same documents the unit tests do,
//! instead of each test file growing its own fixtures.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::core::document::Document;
use crate::core::pixel::PremulImage;

/// The 256px icon, embedded so a window started from a bare executable still has one.
pub const EMBEDDED_ICON: &[u8] =
    include_bytes!("../../assets/icons/hicolor/256x256/apps/rustphotoshop.png");

/// The directory holding the sample files the tests read.
#[must_use]
pub fn sample_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/samples")
}

/// A path inside the sample directory.
#[must_use]
pub fn sample(name: &str) -> PathBuf {
    sample_dir().join(name)
}

/// A document with one full-canvas layer, for tests that need something to edit.
#[must_use]
pub fn document_with_background(width: u32, height: u32, color: [u8; 4]) -> Document {
    let mut document = Document::new(width, height);
    let layer = crate::core::document::Layer::new_pixel(
        "Background",
        Arc::new(PremulImage::filled(width, height, color)),
    );
    document.active_layer = Some(layer.id);
    document.layers.push(layer);
    document
}

/// A document with a gradient-ish pattern, so edits leave visible marks in the tests.
#[must_use]
pub fn document_with_pattern(width: u32, height: u32) -> Document {
    let mut image = PremulImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let red = (x * 255 / width.max(1)) as u8;
            let green = (y * 255 / height.max(1)) as u8;
            let blue = if (x / 8 + y / 8) % 2 == 0 { 40 } else { 200 };
            image.set_pixel(x, y, [red, green, blue, 255]);
        }
    }
    let mut document = Document::new(width, height);
    let layer = crate::core::document::Layer::new_pixel("Pattern", Arc::new(image));
    document.active_layer = Some(layer.id);
    document.layers.push(layer);
    document
}

/// Writes a PNG into a directory, returning its path: the "user's photo" in a test.
///
/// # Errors
///
/// Fails when the file cannot be written.
pub fn write_png(directory: &Path, name: &str, image: &PremulImage) -> Result<PathBuf, String> {
    let path = directory.join(name);
    crate::io::image_io::save_png(image, &path).map_err(|error| error.to_string())?;
    Ok(path)
}

/// A small image with transparent corners and an opaque middle, for round-trip checks.
#[must_use]
pub fn test_image(width: u32, height: u32) -> PremulImage {
    let mut image = PremulImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let inside = x > 0 && y > 0 && x < width - 1 && y < height - 1;
            if inside {
                image.set_pixel(
                    x,
                    y,
                    [
                        (x * 251 % 255) as u8,
                        (y * 241 % 255) as u8,
                        ((x + y) * 233 % 255) as u8,
                        255,
                    ],
                );
            }
        }
    }
    image
}

/// A one-line summary of where the test run's files live, printed by the integration tests.
#[must_use]
pub fn scratch_dir(name: &str) -> PathBuf {
    let base = std::env::temp_dir().join("rustphotoshop-tests").join(name);
    let _ = std::fs::create_dir_all(&base);
    base
}

#[cfg(test)]
mod helper_checks {
    use super::*;

    #[test]
    fn the_embedded_icon_is_a_real_png() {
        assert!(EMBEDDED_ICON.starts_with(&[0x89, b'P', b'N', b'G']));
        let image = crate::io::image_io::decode(EMBEDDED_ICON, Path::new("embedded"))
            .expect("the embedded icon decodes");
        assert_eq!((image.width(), image.height()), (256, 256));
    }

    #[test]
    fn the_pattern_document_is_not_flat() {
        let document = document_with_pattern(16, 16);
        let image = document.layers[0].image.clone().expect("pixels");
        assert_ne!(image.pixel(0, 0), image.pixel(15, 15));
        assert_eq!(test_image(8, 8).pixel(0, 0).expect("pixel")[3], 0);
    }
}

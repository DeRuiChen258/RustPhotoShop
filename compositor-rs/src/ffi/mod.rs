//! The upstream pixel kernels, reused unchanged.
//!
//! `csrc/reused/*.c` are byte-identical copies of `Compositor/Rendering/*.c` from the macOS
//! project (see `scripts/sync-kernels.sh` and `tests/kernel_parity.rs`). They already speak the
//! same pixel layout this crate stores — 8-bit premultiplied RGBA, 4 bytes per pixel, top-down
//! rows — so the wrappers below only have to turn slices into pointers and keep the sizes
//! honest.
//!
//! Every wrapper is safe: sizes come from the buffers themselves, and out-of-range geometry
//! returns a value instead of panicking.

use std::ffi::{c_double, c_float, c_int, c_long, c_uint};

use crate::core::geometry::RectI;
use crate::core::pixel::{GrayImage, PremulImage, Rgba};

/// The kernels exactly as the C headers declare them. Kept in their own module so the safe
/// wrappers below can use the natural Rust names.
mod raw {
    #[cfg(have_dither_kernel)]
    use super::DitherParams;
    use super::{c_double, c_float, c_int, c_long, c_uint};

    unsafe extern "C" {
        // BrushPixels.c
        pub fn brush_alpha_bounds(
            bytes: *const u8,
            width: usize,
            height: usize,
            stride: usize,
            bounds: *mut usize,
        );
        pub fn layer_extract_alpha(
            rgba: *const u8,
            rgba_stride: usize,
            gray: *mut u8,
            gray_stride: usize,
            width: usize,
            height: usize,
        );

        // WandPixels.c
        pub fn wand_mask(
            rgba: *const u8,
            width: usize,
            height: usize,
            stride: usize,
            seed_x: usize,
            seed_y: usize,
            radius: usize,
            tolerance: c_int,
            contiguous: c_int,
            mask: *mut u8,
        ) -> c_long;
        pub fn color_range_mask(
            rgba: *const u8,
            width: usize,
            height: usize,
            stride: usize,
            include: *const u8,
            include_count: c_int,
            exclude: *const u8,
            exclude_count: c_int,
            fuzziness: c_int,
            invert: c_int,
            mask: *mut u8,
        ) -> c_long;

        // LevelsPixels.c
        pub fn levels_apply(pixels: *mut u8, count: usize, tables: *const c_float);
        pub fn levels_histogram(
            pixels: *const u8,
            coverage: *const u8,
            count: usize,
            bins: *mut c_double,
        );

        // NoisePixels.c
        pub fn noise_add(
            rgba: *mut u8,
            width: usize,
            height: usize,
            stride: usize,
            amount: c_float,
            gaussian: c_int,
            monochromatic: c_int,
            seed: c_uint,
        );

        // LensPixels.c
        pub fn lens_distort(
            source: *const u8,
            destination: *mut u8,
            width: usize,
            height: usize,
            stride: usize,
            k: c_double,
        );

        // HealPixels.c
        pub fn spot_heal(
            rgba: *mut u8,
            coverage: *const u8,
            width: usize,
            height: usize,
            stride: usize,
            opacity: c_float,
            mode: c_int,
            seed: c_uint,
        ) -> c_int;

        // ContentFill.c
        pub fn content_fill(
            rgba: *mut u8,
            stride: usize,
            mask: *const u8,
            mask_stride: usize,
            width: c_int,
            height: c_int,
        ) -> c_int;

        // AdjustPixels.c
        pub fn adjust_gradient_map(
            rgba: *mut u8,
            width: usize,
            height: usize,
            stride: usize,
            table: *const u8,
        );
        pub fn adjust_grain(
            rgba: *mut u8,
            width: usize,
            height: usize,
            stride: usize,
            amount: c_double,
            size: c_double,
            roughness: c_double,
            seed: c_uint,
            origin_x: c_double,
            origin_y: c_double,
            units_per_pixel: c_double,
        );
        pub fn adjust_black_white(
            rgba: *mut u8,
            width: usize,
            height: usize,
            stride: usize,
            weights: *const c_float,
            tint: c_int,
            tint_hue: c_double,
            tint_saturation: c_double,
        );
        pub fn adjust_color_balance(
            rgba: *mut u8,
            width: usize,
            height: usize,
            stride: usize,
            shadows: *const c_float,
            midtones: *const c_float,
            highlights: *const c_float,
            preserve_luminosity: c_int,
        );
        pub fn adjust_colored_vignette(
            rgba: *mut u8,
            width: usize,
            height: usize,
            stride: usize,
            frame_x: c_double,
            frame_y: c_double,
            frame_width: c_double,
            frame_height: c_double,
            fills_clear: c_int,
            amount: c_double,
            midpoint: c_double,
            roundness: c_double,
            feather: c_double,
            highlights: c_double,
            red: c_double,
            green: c_double,
            blue: c_double,
        );
        pub fn adjust_tonal_contrast(
            rgba: *mut u8,
            blurred: *const u8,
            width: usize,
            height: usize,
            stride: usize,
            blurred_stride: usize,
            amount: c_double,
            shadows: c_double,
            midtones: c_double,
            highlights: c_double,
        );
        pub fn rgba_clamp_premultiplied(rgba: *mut u8, count: usize);

        // DitherPixels.c — only compiled when the Blocks toolchain is present.
        #[cfg(have_dither_kernel)]
        pub fn dither_apply(
            rgba: *mut u8,
            width: usize,
            height: usize,
            stride: usize,
            params: *const DitherParams,
        ) -> c_int;
    }
}

use raw::*;

/// Mirror of `DitherParams` in `DitherPixels.h`, field for field.
#[cfg(have_dither_kernel)]
#[repr(C)]
struct DitherParams {
    style: c_int,
    levels: c_int,
    diffusion: c_float,
    density: c_float,
    contrast: c_float,
    cell: c_int,
    angle: c_float,
    light_on_dark: c_int,
    original_colors: c_int,
    dark: [u8; 3],
    light: [u8; 3],
    glyph_width: c_int,
    glyph_height: c_int,
    glyphs: *const u8,
    glyph_coverage: *const c_float,
    glyph_count: c_int,
    dots: c_float,
    wobble: c_float,
}

/// The dither styles, in the order `DitherPixels.h` lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DitherStyle {
    Atkinson,
    FloydSteinberg,
    Bayer2,
    Bayer4,
    Bayer8,
    Dots,
    Lines,
    Diamonds,
    Patterns,
    Glyphs,
    Scanlines,
}

impl DitherStyle {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Atkinson => "Atkinson",
            Self::FloydSteinberg => "Floyd–Steinberg",
            Self::Bayer2 => "Bayer 2×2",
            Self::Bayer4 => "Bayer 4×4",
            Self::Bayer8 => "Bayer 8×8",
            Self::Dots => "Dots",
            Self::Lines => "Lines",
            Self::Diamonds => "Diamonds",
            Self::Patterns => "Patterns",
            Self::Glyphs => "Glyphs",
            Self::Scanlines => "Scanlines",
        }
    }

    /// Every style, in the order the Filter panel lists them.
    pub const ALL: &'static [Self] = &[
        Self::Atkinson,
        Self::FloydSteinberg,
        Self::Bayer2,
        Self::Bayer4,
        Self::Bayer8,
        Self::Dots,
        Self::Lines,
        Self::Diamonds,
        Self::Patterns,
        Self::Glyphs,
        Self::Scanlines,
    ];

    /// The value `DitherParams.style` expects. Only the dither call site uses it, and that is
    /// compiled out when the kernel is unavailable, hence the `allow`.
    #[allow(dead_code)]
    fn index(self) -> c_int {
        Self::ALL
            .iter()
            .position(|style| *style == self)
            .unwrap_or(0) as c_int
    }
}

/// The settings the Dither filter offers, a subset of the original's panel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DitherSettings {
    pub style: DitherStyle,
    /// Tones per channel for the diffusion and ordered styles, 2–8.
    pub levels: i32,
    /// How much error diffusion passes on, 0–1.
    pub diffusion: f32,
    pub density: f32,
    pub contrast: f32,
    pub cell: i32,
    /// Keep the image's own colors instead of two inks.
    pub original_colors: bool,
}

impl Default for DitherSettings {
    fn default() -> Self {
        Self {
            style: DitherStyle::Atkinson,
            levels: 2,
            diffusion: 0.75,
            density: 0.0,
            contrast: 0.0,
            cell: 8,
            original_colors: true,
        }
    }
}

/// True when this build compiled the dither kernel (needs clang plus a Blocks runtime).
#[must_use]
pub const fn dither_available() -> bool {
    cfg!(have_dither_kernel)
}

/// Half-open bounds of the pixels that are not fully transparent, straight from the kernel the
/// macOS build uses for its layer thumbnails and Trim command.
#[must_use]
pub fn alpha_bounds(image: &PremulImage) -> Option<RectI> {
    if image.is_empty() {
        return None;
    }
    let mut bounds = [0_usize; 4];
    // SAFETY: the buffer is `width * height * 4` bytes and `bounds` has the four slots the
    // kernel writes.
    unsafe {
        brush_alpha_bounds(
            image.as_slice().as_ptr(),
            image.width() as usize,
            image.height() as usize,
            image.stride(),
            bounds.as_mut_ptr(),
        );
    }
    if bounds[2] == 0 || bounds[3] == 0 {
        return None;
    }
    Some(RectI::new(
        bounds[0] as i64,
        bounds[1] as i64,
        bounds[2] as i64,
        bounds[3] as i64,
    ))
}

/// Copies a layer's alpha into a grayscale buffer, the way Select ▸ Load Layer Alpha does.
///
/// # Panics
///
/// Panics when `out` is not the same size as `image`.
pub fn extract_alpha(image: &PremulImage, out: &mut GrayImage) {
    assert_eq!(image.width(), out.width());
    assert_eq!(image.height(), out.height());
    // SAFETY: both buffers are sized from their images, which are asserted equal.
    unsafe {
        layer_extract_alpha(
            image.as_slice().as_ptr(),
            image.stride(),
            out.as_mut_slice().as_mut_ptr(),
            out.width() as usize,
            image.width() as usize,
            image.height() as usize,
        );
    }
}

/// Selects a contiguous (or global) region by color, into a document-sized mask.
///
/// Returns how many pixels were selected.
pub fn magic_wand(
    image: &PremulImage,
    seed: (u32, u32),
    radius: u32,
    tolerance: u8,
    contiguous: bool,
    mask: &mut GrayImage,
) -> i64 {
    if image.is_empty() || mask.width() == 0 || mask.height() == 0 {
        return 0;
    }
    let seed_x = (seed.0 as usize).min(image.width() as usize - 1);
    let seed_y = (seed.1 as usize).min(image.height() as usize - 1);
    // SAFETY: the mask is `width * height` bytes, which is what the kernel writes.
    unsafe {
        wand_mask(
            image.as_slice().as_ptr(),
            image.width() as usize,
            image.height() as usize,
            image.stride(),
            seed_x,
            seed_y,
            radius as usize,
            c_int::from(tolerance),
            c_int::from(contiguous),
            mask.as_mut_slice().as_mut_ptr(),
        )
    }
}

/// Select ▸ Color Range: matches any of `include` and none of `exclude`.
pub fn color_range(
    image: &PremulImage,
    include: &[[u8; 3]],
    exclude: &[[u8; 3]],
    fuzziness: u8,
    invert: bool,
    mask: &mut GrayImage,
) -> i64 {
    if image.is_empty() {
        return 0;
    }
    let include_flat: Vec<u8> = include.iter().flatten().copied().collect();
    let exclude_flat: Vec<u8> = exclude.iter().flatten().copied().collect();
    // SAFETY: the color arrays are `count * 3` bytes, and the mask matches the image size.
    unsafe {
        color_range_mask(
            image.as_slice().as_ptr(),
            image.width() as usize,
            image.height() as usize,
            image.stride(),
            include_flat.as_ptr(),
            include.len() as c_int,
            exclude_flat.as_ptr(),
            exclude.len() as c_int,
            c_int::from(fuzziness),
            c_int::from(invert),
            mask.as_mut_slice().as_mut_ptr(),
        )
    }
}

/// Content-Aware Fill: paints over the masked pixels using texture from around them.
///
/// `mask` is `width * height` bytes marking what to fill with anything non-zero.
pub fn content_aware_fill(image: &mut PremulImage, mask: &GrayImage) -> bool {
    if image.is_empty() || mask.width() != image.width() || mask.height() != image.height() {
        return false;
    }
    // SAFETY: the mask matches the image, which the check above guarantees.
    let result = unsafe {
        content_fill(
            image.as_mut_slice().as_mut_ptr(),
            image.stride(),
            mask.as_slice().as_ptr(),
            mask.width() as usize,
            image.width() as c_int,
            image.height() as c_int,
        )
    };
    result == 1
}

/// The healing brush, over premultiplied pixels, masked by `coverage`.
pub fn heal(
    image: &mut PremulImage,
    coverage: &GrayImage,
    opacity: f32,
    mode: i32,
    seed: u32,
) -> bool {
    if image.is_empty() || coverage.width() != image.width() || coverage.height() != image.height()
    {
        return false;
    }
    // SAFETY: the coverage buffer matches the image, checked above.
    let result = unsafe {
        spot_heal(
            image.as_mut_slice().as_mut_ptr(),
            coverage.as_slice().as_ptr(),
            image.width() as usize,
            image.height() as usize,
            image.stride(),
            opacity,
            mode,
            seed,
        )
    };
    result == 0
}

/// Applies four 256-entry channel curves (red, green, blue and the composite) to the image.
pub fn apply_levels(image: &mut PremulImage, tables: &[f32; 768]) {
    if image.is_empty() {
        return;
    }
    let count = image.width() as usize * image.height() as usize;
    // SAFETY: the table has the 3 × 256 floats the kernel indexes.
    unsafe {
        levels_apply(image.as_mut_slice().as_mut_ptr(), count, tables.as_ptr());
    }
}

/// Fills the histogram bins the Levels panel draws: RGB in `bins[0..256]`, then red, green and
/// blue in the three blocks after it.
pub fn histogram(image: &PremulImage, coverage: Option<&GrayImage>) -> [f64; 1024] {
    let mut bins = [0.0_f64; 1024];
    if image.is_empty() {
        return bins;
    }
    let count = image.width() as usize * image.height() as usize;
    let coverage_ptr = coverage
        .filter(|mask| mask.width() == image.width() && mask.height() == image.height())
        .map_or(std::ptr::null(), |mask| mask.as_slice().as_ptr());
    // SAFETY: `bins` has the four 256-entry blocks the kernel writes, and `coverage_ptr` is
    // either null (the kernel allows that) or a matching buffer.
    unsafe {
        levels_histogram(
            image.as_slice().as_ptr(),
            coverage_ptr,
            count,
            bins.as_mut_ptr(),
        );
    }
    bins
}

/// Photoshop's Add Noise, on premultiplied pixels; the pattern depends only on the seed.
pub fn add_noise(
    image: &mut PremulImage,
    amount: f32,
    gaussian: bool,
    monochromatic: bool,
    seed: u32,
) {
    if image.is_empty() {
        return;
    }
    // SAFETY: sizes come from the image itself.
    unsafe {
        noise_add(
            image.as_mut_slice().as_mut_ptr(),
            image.width() as usize,
            image.height() as usize,
            image.stride(),
            amount,
            c_int::from(gaussian),
            c_int::from(monochromatic),
            seed,
        );
    }
}

/// Radial lens distortion; `k > 0` pulls samples in, straightening barrel distortion.
#[must_use]
pub fn lens_correct(source: &PremulImage, k: f64) -> PremulImage {
    let mut destination = PremulImage::new(source.width(), source.height());
    if source.is_empty() {
        return destination;
    }
    // SAFETY: both buffers are the same size and layout.
    unsafe {
        lens_distort(
            source.as_slice().as_ptr(),
            destination.as_mut_slice().as_mut_ptr(),
            source.width() as usize,
            source.height() as usize,
            source.stride(),
            k,
        );
    }
    destination
}

/// Gradient Map: a 256-step table of straight sRGB colors, darkest first.
pub fn gradient_map(image: &mut PremulImage, table: &[u8; 768]) {
    if image.is_empty() {
        return;
    }
    // SAFETY: the table has the 256 × 3 bytes the kernel reads.
    unsafe {
        adjust_gradient_map(
            image.as_mut_slice().as_mut_ptr(),
            image.width() as usize,
            image.height() as usize,
            image.stride(),
            table.as_ptr(),
        );
    }
}

/// Film grain, positioned in document units so a crop keeps the same pattern.
pub fn grain(
    image: &mut PremulImage,
    amount: f64,
    size: f64,
    roughness: f64,
    seed: u32,
    origin: [f64; 2],
    units_per_pixel: f64,
) {
    if image.is_empty() {
        return;
    }
    // SAFETY: sizes come from the image itself.
    unsafe {
        adjust_grain(
            image.as_mut_slice().as_mut_ptr(),
            image.width() as usize,
            image.height() as usize,
            image.stride(),
            amount,
            size,
            roughness,
            seed,
            origin[0],
            origin[1],
            units_per_pixel,
        );
    }
}

/// Black & White with six color weights, as the panel's sliders provide.
pub fn black_and_white(
    image: &mut PremulImage,
    weights: &[f32; 6],
    tint: bool,
    tint_hue: f64,
    tint_saturation: f64,
) {
    if image.is_empty() {
        return;
    }
    // SAFETY: the weights array has the six floats the kernel reads.
    unsafe {
        adjust_black_white(
            image.as_mut_slice().as_mut_ptr(),
            image.width() as usize,
            image.height() as usize,
            image.stride(),
            weights.as_ptr(),
            c_int::from(tint),
            tint_hue,
            tint_saturation,
        );
    }
}

/// Color Balance, with each tonal range shifting cyan/red, magenta/green and yellow/blue.
pub fn color_balance(
    image: &mut PremulImage,
    shadows: [f32; 3],
    midtones: [f32; 3],
    highlights: [f32; 3],
    preserve_luminosity: bool,
) {
    if image.is_empty() {
        return;
    }
    // SAFETY: the three arrays hold the three floats each the kernel reads.
    unsafe {
        adjust_color_balance(
            image.as_mut_slice().as_mut_ptr(),
            image.width() as usize,
            image.height() as usize,
            image.stride(),
            shadows.as_ptr(),
            midtones.as_ptr(),
            highlights.as_ptr(),
            c_int::from(preserve_luminosity),
        );
    }
}

/// A vignette shaped to a frame, either darkening or tinting the edges.
#[allow(clippy::too_many_arguments)]
pub fn colored_vignette(
    image: &mut PremulImage,
    frame: RectI,
    fills_clear: bool,
    amount: f64,
    midpoint: f64,
    roundness: f64,
    feather: f64,
    highlights: f64,
    color: Rgba,
) {
    if image.is_empty() {
        return;
    }
    let color = [
        f64::from(color[0]) / 255.0,
        f64::from(color[1]) / 255.0,
        f64::from(color[2]) / 255.0,
    ];
    // SAFETY: sizes come from the image itself.
    unsafe {
        adjust_colored_vignette(
            image.as_mut_slice().as_mut_ptr(),
            image.width() as usize,
            image.height() as usize,
            image.stride(),
            frame.x0 as c_double,
            frame.y0 as c_double,
            frame.width() as c_double,
            frame.height() as c_double,
            c_int::from(fills_clear),
            amount,
            midpoint,
            roundness,
            feather,
            highlights,
            color[0],
            color[1],
            color[2],
        );
    }
}

/// Local contrast with separate shadow, midtone and highlight gains, using a blurred copy.
pub fn tonal_contrast(
    image: &mut PremulImage,
    blurred: &PremulImage,
    amount: f64,
    shadows: f64,
    midtones: f64,
    highlights: f64,
) {
    if image.is_empty() || blurred.width() != image.width() || blurred.height() != image.height() {
        return;
    }
    // SAFETY: the blurred copy matches the image, checked above.
    unsafe {
        adjust_tonal_contrast(
            image.as_mut_slice().as_mut_ptr(),
            blurred.as_slice().as_ptr(),
            image.width() as usize,
            image.height() as usize,
            image.stride(),
            blurred.stride(),
            amount,
            shadows,
            midtones,
            highlights,
        );
    }
}

/// Clamps the premultiplied channels back to their alpha, after a resample that can ring.
pub fn clamp_premultiplied(image: &mut PremulImage) {
    if image.is_empty() {
        return;
    }
    let count = image.width() as usize * image.height() as usize;
    // SAFETY: the buffer holds exactly `count` pixels.
    unsafe {
        rgba_clamp_premultiplied(image.as_mut_slice().as_mut_ptr(), count);
    }
}

/// The Dither filter. Returns `false` when this build has no dither kernel, or the kernel ran
/// out of memory.
pub fn dither(image: &mut PremulImage, settings: DitherSettings) -> bool {
    #[cfg(have_dither_kernel)]
    {
        if image.is_empty() {
            return false;
        }
        let params = DitherParams {
            style: settings.style.index(),
            levels: settings.levels as c_int,
            diffusion: settings.diffusion,
            density: settings.density,
            contrast: settings.contrast,
            cell: settings.cell as c_int,
            angle: 0.0,
            light_on_dark: 0,
            original_colors: c_int::from(settings.original_colors),
            dark: [0, 0, 0],
            light: [255, 255, 255],
            glyph_width: 0,
            glyph_height: 0,
            glyphs: std::ptr::null(),
            glyph_coverage: std::ptr::null(),
            glyph_count: 0,
            dots: 0.0,
            wobble: 0.0,
        };
        // SAFETY: the params mirror the C struct and the image buffer is its own size.
        let result = unsafe {
            dither_apply(
                image.as_mut_slice().as_mut_ptr(),
                image.width() as usize,
                image.height() as usize,
                image.stride(),
                &params,
            )
        };
        // The kernel answers 1 when it ran and 0 when it ran out of working memory.
        result != 0
    }
    #[cfg(not(have_dither_kernel))]
    {
        let _ = (image, settings);
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alpha_bounds_match_the_drawing() {
        let mut image = PremulImage::new(8, 8);
        image.set_pixel(2, 3, [10, 10, 10, 255]);
        image.set_pixel(5, 6, [10, 10, 10, 255]);
        assert_eq!(alpha_bounds(&image), Some(RectI::new(2, 3, 6, 7)));
        assert_eq!(alpha_bounds(&PremulImage::new(8, 8)), None);
    }

    #[test]
    fn extract_alpha_copies_coverage() {
        let mut image = PremulImage::new(3, 1);
        image.set_pixel(0, 0, [100, 100, 100, 255]);
        image.set_pixel(2, 0, [0, 0, 0, 128]);
        let mut gray = GrayImage::new(3, 1, 0);
        extract_alpha(&image, &mut gray);
        assert_eq!(gray.pixel(0, 0), 255);
        assert_eq!(gray.pixel(1, 0), 0);
        assert_eq!(gray.pixel(2, 0), 128);
    }

    #[test]
    fn magic_wand_selects_a_solid_region() {
        let mut image = PremulImage::filled(4, 4, [255, 0, 0, 255]);
        for y in 0..4 {
            image.set_pixel(3, y, [0, 0, 255, 255]);
        }
        let mut mask = GrayImage::new(4, 4, 0);
        let selected = magic_wand(&image, (0, 0), 0, 10, true, &mut mask);
        assert_eq!(selected, 12);
        assert_eq!(mask.pixel(0, 0), 255);
        assert_eq!(mask.pixel(3, 0), 0);
    }

    #[test]
    fn levels_curve_maps_black_and_white() {
        let mut image = PremulImage::filled(2, 1, [0, 0, 0, 255]);
        image.set_pixel(1, 0, [255, 255, 255, 255]);
        let mut tables = [0.0_f32; 768];
        for channel in 0..3 {
            for value in 0..256 {
                tables[channel * 256 + value] = 255.0 - value as f32;
            }
        }
        apply_levels(&mut image, &tables);
        assert_eq!(image.pixel(0, 0), Some([255, 255, 255, 255]));
        assert_eq!(image.pixel(1, 0), Some([0, 0, 0, 255]));
    }

    #[test]
    fn add_noise_changes_pixels_but_not_alpha() {
        let mut image = PremulImage::filled(16, 16, [128, 128, 128, 255]);
        let before = image.clone();
        add_noise(&mut image, 50.0, false, false, 7);
        assert_ne!(image.as_slice(), before.as_slice());
        assert!(
            image
                .as_slice()
                .chunks_exact(4)
                .all(|pixel| pixel[3] == 255)
        );
    }

    #[test]
    fn content_aware_fill_replaces_a_hole() {
        let mut image = PremulImage::filled(32, 32, [200, 100, 50, 255]);
        for y in 12..20 {
            for x in 12..20 {
                image.set_pixel(x, y, [0, 255, 0, 255]);
            }
        }
        let mut mask = GrayImage::new(32, 32, 0);
        for y in 12..20 {
            for x in 12..20 {
                mask.set_pixel(x, y, 255);
            }
        }
        assert!(content_aware_fill(&mut image, &mask));
        let filled = image.pixel(15, 15).expect("pixel");
        assert!(
            filled[0] > 150,
            "hole should take the surrounding texture: {filled:?}"
        );
    }

    #[test]
    fn heal_smooths_a_spot() {
        let mut image = PremulImage::filled(32, 32, [120, 120, 120, 255]);
        for y in 14..18 {
            for x in 14..18 {
                image.set_pixel(x, y, [255, 0, 0, 255]);
            }
        }
        let mut coverage = GrayImage::new(32, 32, 0);
        for y in 14..18 {
            for x in 14..18 {
                coverage.set_pixel(x, y, 255);
            }
        }
        assert!(heal(&mut image, &coverage, 1.0, 0, 1));
        let healed = image.pixel(15, 15).expect("pixel");
        assert!(
            healed[1] > 60,
            "red spot should be blended away: {healed:?}"
        );
    }

    #[test]
    fn histogram_counts_the_selected_pixels() {
        let image = PremulImage::filled(4, 4, [255, 255, 255, 255]);
        let bins = histogram(&image, None);
        assert!((bins[255] - 16.0).abs() < 1e-6);
    }

    #[test]
    fn vignette_darkens_the_corners() {
        let mut image = PremulImage::filled(64, 64, [255, 255, 255, 255]);
        colored_vignette(
            &mut image,
            RectI::new(0, 0, 64, 64),
            false,
            80.0,
            50.0,
            0.0,
            50.0,
            0.0,
            [0, 0, 0, 255],
        );
        let corner = image.pixel(1, 1).expect("pixel");
        let center = image.pixel(32, 32).expect("pixel");
        assert!(corner[0] < center[0], "corner {corner:?} center {center:?}");
    }

    #[test]
    fn dither_reports_availability() {
        // The build either has the kernel (clang + Blocks runtime) or cleanly says it does not.
        let mut image = PremulImage::filled(32, 32, [128, 128, 128, 255]);
        let applied = dither(&mut image, DitherSettings::default());
        assert_eq!(applied, dither_available());
    }
}

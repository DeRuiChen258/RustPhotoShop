//! Adjustments and filters.
//!
//! [`Adjustment`] holds the kinds the project format can store on an adjustment layer, so a
//! project saved here opens in the macOS build with the same look. Where the original has a C
//! kernel, this calls it ([`crate::ffi`]) rather than approximating it; the rest (Hue/Saturation,
//! Exposure, Curves, blurs) are implemented here.

use crate::core::geometry::RectI;
use crate::core::pixel::{GrayImage, PremulImage, Rgba};
use crate::ffi::{self, DitherSettings};

/// One channel's Levels range, in 8-bit units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LevelRange {
    pub black: f64,
    pub gamma: f64,
    pub white: f64,
    pub output_black: f64,
    pub output_white: f64,
}

impl Default for LevelRange {
    fn default() -> Self {
        Self {
            black: 0.0,
            gamma: 1.0,
            white: 255.0,
            output_black: 0.0,
            output_white: 255.0,
        }
    }
}

impl LevelRange {
    #[must_use]
    pub fn is_identity(&self) -> bool {
        *self == Self::default()
    }

    /// Maps one 0–255 value through the range.
    #[must_use]
    pub fn map(&self, value: f64) -> f64 {
        let span = if (self.white - self.black).abs() < 1e-9 {
            1e-9
        } else {
            self.white - self.black
        };
        let normalized = ((value - self.black) / span).clamp(0.0, 1.0);
        let gamma = if self.gamma.abs() < 1e-6 {
            1e-6
        } else {
            self.gamma
        };
        let corrected = normalized.powf(1.0 / gamma);
        self.output_black + corrected * (self.output_white - self.output_black)
    }
}

/// Levels for the composite channel plus red, green and blue.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct LevelsSettings {
    pub ranges: [LevelRange; 4],
}

impl LevelsSettings {
    #[must_use]
    pub fn is_identity(&self) -> bool {
        self.ranges.iter().all(LevelRange::is_identity)
    }

    /// The three lookup tables the kernel applies, per-channel then composite.
    #[must_use]
    pub fn lookup_tables(&self) -> [f32; 768] {
        let mut tables = [0.0_f32; 768];
        for channel in 0..3 {
            for value in 0..256 {
                let after_channel = self.ranges[channel + 1].map(value as f64);
                let after_rgb = self.ranges[0].map(after_channel);
                tables[channel * 256 + value] = after_rgb.clamp(0.0, 255.0) as f32;
            }
        }
        tables
    }
}

/// One channel of a Curves adjustment: control points from x=0 to x=255 in increasing order.
#[derive(Clone, Debug, PartialEq)]
pub struct CurveChannel {
    pub points: Vec<[f64; 2]>,
}

impl Default for CurveChannel {
    fn default() -> Self {
        Self {
            points: vec![[0.0, 0.0], [255.0, 255.0]],
        }
    }
}

impl CurveChannel {
    #[must_use]
    pub fn is_identity(&self) -> bool {
        self.points.len() == 2
            && self.points[0][0].abs() < 1e-9
            && self.points[0][1].abs() < 1e-9
            && (self.points[1][0] - 255.0).abs() < 1e-9
            && (self.points[1][1] - 255.0).abs() < 1e-9
    }

    /// A 256-entry lookup table, interpolated with monotone cubic segments so a curve never
    /// overshoots between its points.
    #[must_use]
    pub fn lookup_table(&self) -> [f32; 256] {
        let mut table = [0.0_f32; 256];
        let mut points: Vec<[f64; 2]> = self.points.clone();
        points.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap_or(std::cmp::Ordering::Equal));
        points.dedup_by(|a, b| (a[0] - b[0]).abs() < 1e-9);
        if points.len() < 2 {
            for (value, entry) in table.iter_mut().enumerate() {
                *entry = value as f32;
            }
            return table;
        }
        let count = points.len();
        let mut slopes = vec![0.0_f64; count - 1];
        for index in 0..count - 1 {
            let dx = points[index + 1][0] - points[index][0];
            let dy = points[index + 1][1] - points[index][1];
            slopes[index] = if dx.abs() < 1e-9 { 0.0 } else { dy / dx };
        }
        let mut tangents = vec![0.0_f64; count];
        tangents[0] = slopes[0];
        tangents[count - 1] = slopes[count - 2];
        for index in 1..count - 1 {
            let previous = slopes[index - 1];
            let next = slopes[index];
            tangents[index] = if previous * next <= 0.0 {
                0.0
            } else {
                (previous + next) / 2.0
            };
        }
        for (value, entry) in table.iter_mut().enumerate() {
            let x = value as f64;
            if x <= points[0][0] {
                *entry = points[0][1].clamp(0.0, 255.0) as f32;
                continue;
            }
            if x >= points[count - 1][0] {
                *entry = points[count - 1][1].clamp(0.0, 255.0) as f32;
                continue;
            }
            let segment = (0..count - 1)
                .find(|index| x >= points[*index][0] && x <= points[index + 1][0])
                .unwrap_or(0);
            let x0 = points[segment][0];
            let x1 = points[segment + 1][0];
            let h = (x1 - x0).max(1e-9);
            let t = (x - x0) / h;
            let t2 = t * t;
            let t3 = t2 * t;
            let interpolated = (2.0 * t3 - 3.0 * t2 + 1.0) * points[segment][1]
                + (t3 - 2.0 * t2 + t) * h * tangents[segment]
                + (-2.0 * t3 + 3.0 * t2) * points[segment + 1][1]
                + (t3 - t2) * h * tangents[segment + 1];
            *entry = interpolated.clamp(0.0, 255.0) as f32;
        }
        table
    }
}

/// Curves for the composite channel plus red, green and blue.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct CurvesSettings {
    pub channels: [CurveChannel; 4],
}

impl CurvesSettings {
    #[must_use]
    pub fn is_identity(&self) -> bool {
        self.channels.iter().all(CurveChannel::is_identity)
    }

    /// The kernel's tables: per-channel curve first, then the composite curve.
    #[must_use]
    pub fn lookup_tables(&self) -> [f32; 768] {
        let mut tables = [0.0_f32; 768];
        let composite = self.channels[0].lookup_table();
        for channel in 0..3 {
            let own = self.channels[channel + 1].lookup_table();
            for value in 0..256 {
                let after_channel = own[value];
                let index = after_channel.round().clamp(0.0, 255.0) as usize;
                tables[channel * 256 + value] = composite[index];
            }
        }
        tables
    }
}

/// The Exposure adjustment's settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExposureSettings {
    /// Stops of light, −5…5.
    pub exposure: f64,
    /// −0.5…0.5, added in linear light.
    pub offset: f64,
    /// 0.01…9.99.
    pub gamma: f64,
}

impl Default for ExposureSettings {
    fn default() -> Self {
        Self {
            exposure: 0.0,
            offset: 0.0,
            gamma: 1.0,
        }
    }
}

impl ExposureSettings {
    #[must_use]
    pub fn is_identity(&self) -> bool {
        self.exposure.abs() < 1e-9 && self.offset.abs() < 1e-9 && (self.gamma - 1.0).abs() < 1e-9
    }
}

/// The Gradient Map adjustment's settings: two straight sRGB colors.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradientMapSettings {
    pub shadows: [u8; 3],
    pub highlights: [u8; 3],
    pub reversed: bool,
}

impl Default for GradientMapSettings {
    fn default() -> Self {
        Self {
            shadows: [0, 0, 0],
            highlights: [255, 255, 255],
            reversed: false,
        }
    }
}

/// The Grain adjustment's settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GrainSettings {
    /// 0–100.
    pub amount: f64,
    /// The grain's scale in document units.
    pub size: f64,
    /// 0–100 of smaller irregular particles.
    pub roughness: f64,
    pub seed: u32,
}

impl Default for GrainSettings {
    fn default() -> Self {
        Self {
            amount: 0.0,
            size: 1.0,
            roughness: 0.0,
            seed: 0,
        }
    }
}

/// The Color Balance adjustment's settings: shifts per tonal range, −100…100.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorBalanceSettings {
    pub shadows: [f32; 3],
    pub midtones: [f32; 3],
    pub highlights: [f32; 3],
    pub preserve_luminosity: bool,
}

impl Default for ColorBalanceSettings {
    fn default() -> Self {
        Self {
            shadows: [0.0; 3],
            midtones: [0.0; 3],
            highlights: [0.0; 3],
            preserve_luminosity: true,
        }
    }
}

impl ColorBalanceSettings {
    #[must_use]
    pub fn is_identity(&self) -> bool {
        self.shadows.iter().all(|value| value.abs() < 1e-9)
            && self.midtones.iter().all(|value| value.abs() < 1e-9)
            && self.highlights.iter().all(|value| value.abs() < 1e-9)
    }
}

/// An adjustment the project format can store on an adjustment layer.
#[derive(Clone, Debug, PartialEq)]
pub enum Adjustment {
    Invert,
    Levels(LevelsSettings),
    Curves(CurvesSettings),
    HueSaturation {
        /// −180…180 degrees.
        hue: f64,
        /// −100…100.
        saturation: f64,
        /// −100…100.
        lightness: f64,
        colorize: bool,
    },
    Exposure(ExposureSettings),
    BlackAndWhite {
        /// Red, yellow, green, cyan, blue, magenta — as fractions, so 40% is 0.4.
        weights: [f32; 6],
        tint: bool,
        tint_hue: f64,
        tint_saturation: f64,
    },
    ColorBalance(ColorBalanceSettings),
    GradientMap(GradientMapSettings),
    GaussianBlur {
        /// Radius in document pixels, 0.1–250.
        radius: f64,
    },
    MotionBlur {
        /// −90…90 degrees.
        angle: f64,
        /// 1–2000 pixels.
        distance: f64,
    },
    AddNoise {
        /// 0.1–400, Photoshop's percentage.
        amount: f64,
        gaussian: bool,
        monochromatic: bool,
        seed: u32,
    },
    Grain(GrainSettings),
}

impl Adjustment {
    /// The `kind` string the project format stores for this adjustment.
    #[must_use]
    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::Invert => "Invert",
            Self::Levels(_) => "Levels",
            Self::Curves(_) => "Curves",
            Self::HueSaturation { .. } => "Hue/Saturation",
            Self::Exposure(_) => "Exposure",
            Self::BlackAndWhite { .. } => "Black & White",
            Self::ColorBalance(_) => "Color Balance",
            Self::GradientMap(_) => "Gradient Map",
            Self::GaussianBlur { .. } => "Gaussian Blur",
            Self::MotionBlur { .. } => "Motion Blur",
            Self::AddNoise { .. } => "Add Noise",
            Self::Grain(_) => "Grain",
        }
    }

    /// A short name for the layers panel and the dialog title.
    #[must_use]
    pub fn label(&self) -> &'static str {
        self.kind_name()
    }

    /// What the interface calls this adjustment. The manifest keeps `kind_name`, because that
    /// spelling is part of the file format; this is only for people to read.
    #[must_use]
    pub fn display_label(&self) -> &'static str {
        match self {
            Self::Invert => "反相",
            Self::Levels(_) => "色阶",
            Self::Curves(_) => "曲线",
            Self::HueSaturation { .. } => "色相/饱和度",
            Self::Exposure(_) => "曝光",
            Self::BlackAndWhite { .. } => "黑白",
            Self::ColorBalance(_) => "色彩平衡",
            Self::GradientMap(_) => "渐变映射",
            Self::GaussianBlur { .. } => "高斯模糊",
            Self::MotionBlur { .. } => "动感模糊",
            Self::AddNoise { .. } => "添加杂色",
            Self::Grain(_) => "颗粒",
        }
    }

    /// True when the adjustment would leave the pixels alone.
    #[must_use]
    pub fn is_identity(&self) -> bool {
        match self {
            Self::Invert => false,
            Self::Levels(settings) => settings.is_identity(),
            Self::Curves(settings) => settings.is_identity(),
            Self::HueSaturation {
                hue,
                saturation,
                lightness,
                colorize,
            } => hue.abs() < 1e-9 && saturation.abs() < 1e-9 && lightness.abs() < 1e-9 && !colorize,
            Self::Exposure(settings) => settings.is_identity(),
            Self::BlackAndWhite { .. } => false,
            Self::ColorBalance(settings) => settings.is_identity(),
            Self::GradientMap(_) => false,
            Self::GaussianBlur { radius } => radius.abs() < 1e-9,
            Self::MotionBlur { distance, .. } => distance.abs() < 1e-9,
            Self::AddNoise { amount, .. } => amount.abs() < 1e-9,
            Self::Grain(settings) => settings.amount.abs() < 1e-9,
        }
    }

    /// Runs the adjustment over every pixel of the buffer.
    pub fn apply_direct(&self, image: &mut PremulImage) {
        match self {
            Self::Invert => invert(image),
            Self::Levels(settings) => ffi::apply_levels(image, &settings.lookup_tables()),
            Self::Curves(settings) => ffi::apply_levels(image, &settings.lookup_tables()),
            Self::HueSaturation {
                hue,
                saturation,
                lightness,
                colorize,
            } => hue_saturation(image, *hue, *saturation, *lightness, *colorize),
            Self::Exposure(settings) => {
                apply_exposure(image, settings.exposure, settings.offset, settings.gamma);
            }
            Self::BlackAndWhite {
                weights,
                tint,
                tint_hue,
                tint_saturation,
            } => ffi::black_and_white(image, weights, *tint, *tint_hue, *tint_saturation),
            Self::ColorBalance(settings) => ffi::color_balance(
                image,
                settings.shadows,
                settings.midtones,
                settings.highlights,
                settings.preserve_luminosity,
            ),
            Self::GradientMap(settings) => ffi::gradient_map(
                image,
                &gradient_table(settings.shadows, settings.highlights, settings.reversed),
            ),
            Self::GaussianBlur { radius } => {
                *image = blurred(image, radius.max(0.0));
            }
            Self::MotionBlur { angle, distance } => motion_blur(image, *angle, *distance),
            Self::AddNoise {
                amount,
                gaussian,
                monochromatic,
                seed,
            } => ffi::add_noise(image, *amount as f32, *gaussian, *monochromatic, *seed),
            Self::Grain(settings) => ffi::grain(
                image,
                settings.amount,
                settings.size,
                settings.roughness,
                settings.seed,
                [0.0, 0.0],
                1.0,
            ),
        }
    }
}

/// Applies an adjustment, optionally limited to a selection's coverage mask.
///
/// The mask must be in the same pixel space as the buffer. Only the covered rectangle is
/// processed, so a small selection stays cheap.
pub fn apply_adjustment(
    adjustment: &Adjustment,
    image: &mut PremulImage,
    coverage: Option<&GrayImage>,
) {
    let Some(coverage) = coverage else {
        adjustment.apply_direct(image);
        return;
    };
    if coverage.width() != image.width() || coverage.height() != image.height() {
        adjustment.apply_direct(image);
        return;
    }
    let Some((x0, y0, x1, y1)) = coverage.bounds() else {
        return;
    };
    let region = RectI::new(i64::from(x0), i64::from(y0), i64::from(x1), i64::from(y1));
    let Some(mut processed) = image.crop(
        region.x0,
        region.y0,
        region.width() as u32,
        region.height() as u32,
    ) else {
        return;
    };
    adjustment.apply_direct(&mut processed);
    for y in region.y0..region.y1 {
        for x in region.x0..region.x1 {
            let weight = coverage.pixel(x as u32, y as u32);
            if weight == 0 {
                continue;
            }
            let amount = f32::from(weight) / 255.0;
            let base = image.pixel(x as u32, y as u32).unwrap_or([0; 4]);
            let target = processed
                .pixel((x - region.x0) as u32, (y - region.y0) as u32)
                .unwrap_or(base);
            let mut out = [0_u8; 4];
            for channel in 0..4 {
                out[channel] = (f32::from(base[channel])
                    + (f32::from(target[channel]) - f32::from(base[channel])) * amount)
                    .round()
                    .clamp(0.0, 255.0) as u8;
            }
            image.set_pixel(x as u32, y as u32, out);
        }
    }
}

/// A destructive filter: the ones the Filter menu applies straight to a layer's pixels.
#[derive(Clone, Debug, PartialEq)]
pub enum Filter {
    GaussianBlur {
        radius: f64,
    },
    MotionBlur {
        angle: f64,
        distance: f64,
    },
    AddNoise {
        amount: f64,
        gaussian: bool,
        monochromatic: bool,
        seed: u32,
    },
    Grain {
        amount: f64,
        size: f64,
        roughness: f64,
        seed: u32,
    },
    LensCorrection {
        k: f64,
    },
    Vignette {
        amount: f64,
        midpoint: f64,
        roundness: f64,
        feather: f64,
        highlights: f64,
        color: Rgba,
    },
    TonalContrast {
        amount: f64,
        shadows: f64,
        midtones: f64,
        highlights: f64,
        radius: f64,
    },
    Dither(DitherSettings),
}

impl Filter {
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::GaussianBlur { .. } => "高斯模糊",
            Self::MotionBlur { .. } => "动感模糊",
            Self::AddNoise { .. } => "添加杂色",
            Self::Grain { .. } => "颗粒",
            Self::LensCorrection { .. } => "镜头校正",
            Self::Vignette { .. } => "暗角",
            Self::TonalContrast { .. } => "色调对比",
            Self::Dither(_) => "抖动",
        }
    }

    /// Runs over a whole layer buffer.
    ///
    /// # Errors
    ///
    /// Fails when the filter needs a kernel this build does not have (Dither without clang).
    pub fn apply(&self, image: &mut PremulImage) -> Result<(), String> {
        match self {
            Self::GaussianBlur { radius } => {
                *image = blurred(image, radius.max(0.0));
                Ok(())
            }
            Self::MotionBlur { angle, distance } => {
                motion_blur(image, *angle, *distance);
                Ok(())
            }
            Self::AddNoise {
                amount,
                gaussian,
                monochromatic,
                seed,
            } => {
                ffi::add_noise(image, *amount as f32, *gaussian, *monochromatic, *seed);
                Ok(())
            }
            Self::Grain {
                amount,
                size,
                roughness,
                seed,
            } => {
                ffi::grain(image, *amount, *size, *roughness, *seed, [0.0, 0.0], 1.0);
                Ok(())
            }
            Self::LensCorrection { k } => {
                *image = ffi::lens_correct(image, *k);
                Ok(())
            }
            Self::Vignette {
                amount,
                midpoint,
                roundness,
                feather,
                highlights,
                color,
            } => {
                let frame = RectI::new(0, 0, i64::from(image.width()), i64::from(image.height()));
                ffi::colored_vignette(
                    image,
                    frame,
                    false,
                    *amount,
                    *midpoint,
                    *roundness,
                    *feather,
                    *highlights,
                    *color,
                );
                Ok(())
            }
            Self::TonalContrast {
                amount,
                shadows,
                midtones,
                highlights,
                radius,
            } => {
                let soft = blurred(image, *radius);
                ffi::tonal_contrast(image, &soft, *amount, *shadows, *midtones, *highlights);
                Ok(())
            }
            Self::Dither(settings) => {
                if ffi::dither(image, *settings) {
                    Ok(())
                } else if ffi::dither_available() {
                    Err("the dither kernel ran out of memory".to_owned())
                } else {
                    Err(
                        "this build has no dither kernel: install clang and libblocksruntime-dev, \
                         then rebuild (see BUILD.md)"
                            .to_owned(),
                    )
                }
            }
        }
    }
}

/// Inverts every channel, keeping alpha: 0 becomes 1, so transparency is untouched.
#[allow(clippy::needless_range_loop)] // channels are addressed by number
pub fn invert(image: &mut PremulImage) {
    for pixel in image.as_mut_slice().chunks_exact_mut(4) {
        let alpha = pixel[3];
        if alpha == 0 {
            continue;
        }
        for channel in 0..3 {
            pixel[channel] = alpha - pixel[channel];
        }
    }
}

/// Hue/Saturation, following Photoshop's behavior, including Colorize.
#[allow(clippy::needless_range_loop)] // the four channels of one pixel, in order
pub fn hue_saturation(
    image: &mut PremulImage,
    hue_degrees: f64,
    saturation: f64,
    lightness: f64,
    colorize: bool,
) {
    let hue_shift = hue_degrees / 360.0;
    for pixel in image.as_mut_slice().chunks_exact_mut(4) {
        let alpha = f64::from(pixel[3]) / 255.0;
        if alpha <= 0.0 {
            continue;
        }
        let color = [
            f64::from(pixel[0]) / 255.0 / alpha,
            f64::from(pixel[1]) / 255.0 / alpha,
            f64::from(pixel[2]) / 255.0 / alpha,
        ];
        let (mut hue, mut saturation_value, mut lightness_value) = rgb_to_hsl(color);
        if colorize {
            hue = ((hue_degrees + 180.0) / 360.0).rem_euclid(1.0);
            saturation_value = ((saturation + 100.0) / 200.0).clamp(0.0, 1.0);
        } else {
            hue = (hue + hue_shift).rem_euclid(1.0);
            saturation_value = (saturation_value * (1.0 + saturation / 100.0)).clamp(0.0, 1.0);
        }
        lightness_value = if lightness >= 0.0 {
            lightness_value + (1.0 - lightness_value) * (lightness / 100.0)
        } else {
            lightness_value * (1.0 + lightness / 100.0)
        };
        let adjusted = hsl_to_rgb(hue, saturation_value, lightness_value.clamp(0.0, 1.0));
        for channel in 0..3 {
            let value = adjusted[channel].clamp(0.0, 1.0) * alpha * 255.0;
            pixel[channel] = value.round().clamp(0.0, 255.0) as u8;
        }
    }
}

/// Exposure: stops of light, an offset, then a gamma curve, all in linear light.
#[allow(clippy::needless_range_loop)] // the three color channels of one pixel, in order
pub fn apply_exposure(image: &mut PremulImage, stops: f64, offset: f64, gamma: f64) {
    let multiplier = 2.0_f64.powf(stops);
    let gamma = if gamma.abs() < 1e-6 { 1e-6 } else { gamma };
    for pixel in image.as_mut_slice().chunks_exact_mut(4) {
        let alpha = f64::from(pixel[3]) / 255.0;
        if alpha <= 0.0 {
            continue;
        }
        for channel in 0..3 {
            let straight = f64::from(pixel[channel]) / 255.0 / alpha;
            let linear = srgb_to_linear(straight.clamp(0.0, 1.0)) * multiplier + offset;
            let value = linear_to_srgb(linear.clamp(0.0, 1.0).powf(1.0 / gamma));
            pixel[channel] = (value * alpha * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
}

/// A 256-step gradient between two straight sRGB colors.
#[must_use]
#[allow(clippy::needless_range_loop)] // a 256-step table written one channel at a time
pub fn gradient_table(shadows: [u8; 3], highlights: [u8; 3], reversed: bool) -> [u8; 768] {
    let mut table = [0_u8; 768];
    for step in 0..256 {
        let t = step as f64 / 255.0;
        let t = if reversed { 1.0 - t } else { t };
        for channel in 0..3 {
            let from = f64::from(shadows[channel]);
            let to = f64::from(highlights[channel]);
            table[step * 3 + channel] = (from + (to - from) * t).round().clamp(0.0, 255.0) as u8;
        }
    }
    table
}

/// A Gaussian blur implemented as three box passes, which is O(1) per pixel per pass and
/// indistinguishable from a true Gaussian at these radii. The radius is Photoshop's: σ = r / 2.
#[must_use]
pub fn blurred(image: &PremulImage, radius: f64) -> PremulImage {
    if radius <= 0.0 || image.is_empty() {
        return image.clone();
    }
    let sigma = radius / 2.0;
    // Three box passes of this width have the same standard deviation as the Gaussian.
    let box_radius = (((4.0 * sigma * sigma + 1.0).sqrt() - 1.0) / 2.0)
        .round()
        .max(1.0) as u32;
    let mut current = image.clone();
    for _ in 0..3 {
        current = box_pass(&current, box_radius);
    }
    current
}

fn box_pass(image: &PremulImage, radius: u32) -> PremulImage {
    let width = i64::from(image.width());
    let height = i64::from(image.height());
    let radius_i64 = i64::from(radius);
    let window = radius_i64 * 2 + 1;
    let mut horizontal = PremulImage::new(image.width(), image.height());
    for y in 0..height {
        let mut sums = [0_i64; 4];
        for offset in -radius_i64..=radius_i64 {
            let x = offset.clamp(0, width - 1) as u32;
            let pixel = image.pixel(x, y as u32).unwrap_or([0; 4]);
            for channel in 0..4 {
                sums[channel] += i64::from(pixel[channel]);
            }
        }
        for x in 0..width {
            let mut out = [0_u8; 4];
            for channel in 0..4 {
                out[channel] = (sums[channel] / window).clamp(0, 255) as u8;
            }
            horizontal.set_pixel(x as u32, y as u32, out);
            let leaving = (x - radius_i64).clamp(0, width - 1) as u32;
            let entering = (x + radius_i64 + 1).clamp(0, width - 1) as u32;
            let leaving_pixel = image.pixel(leaving, y as u32).unwrap_or([0; 4]);
            let entering_pixel = image.pixel(entering, y as u32).unwrap_or([0; 4]);
            for channel in 0..4 {
                sums[channel] +=
                    i64::from(entering_pixel[channel]) - i64::from(leaving_pixel[channel]);
            }
        }
    }
    let mut out = PremulImage::new(image.width(), image.height());
    for x in 0..width {
        let mut sums = [0_i64; 4];
        for offset in -radius_i64..=radius_i64 {
            let y = offset.clamp(0, height - 1) as u32;
            let pixel = horizontal.pixel(x as u32, y).unwrap_or([0; 4]);
            for channel in 0..4 {
                sums[channel] += i64::from(pixel[channel]);
            }
        }
        for y in 0..height {
            let mut pixel = [0_u8; 4];
            for channel in 0..4 {
                pixel[channel] = (sums[channel] / window).clamp(0, 255) as u8;
            }
            out.set_pixel(x as u32, y as u32, pixel);
            let leaving = (y - radius_i64).clamp(0, height - 1) as u32;
            let entering = (y + radius_i64 + 1).clamp(0, height - 1) as u32;
            let leaving_pixel = horizontal.pixel(x as u32, leaving).unwrap_or([0; 4]);
            let entering_pixel = horizontal.pixel(x as u32, entering).unwrap_or([0; 4]);
            for channel in 0..4 {
                sums[channel] +=
                    i64::from(entering_pixel[channel]) - i64::from(leaving_pixel[channel]);
            }
        }
    }
    out
}

/// A straight motion blur: every pixel averages along a line through it.
pub fn motion_blur(image: &mut PremulImage, angle_degrees: f64, distance: f64) {
    let steps = distance.round().clamp(1.0, 128.0) as i64;
    if steps <= 1 {
        return;
    }
    let source = image.clone();
    let radians = angle_degrees.to_radians();
    let (sin, cos) = radians.sin_cos();
    let width = i64::from(image.width());
    let height = i64::from(image.height());
    for y in 0..height {
        for x in 0..width {
            let mut sums = [0_i64; 4];
            for step in 0..steps {
                let t = step as f64 / (steps - 1) as f64 - 0.5;
                let sample_x = (x as f64 + cos * t * distance).round() as i64;
                let sample_y = (y as f64 + sin * t * distance).round() as i64;
                let pixel = source
                    .pixel(
                        sample_x.clamp(0, width - 1) as u32,
                        sample_y.clamp(0, height - 1) as u32,
                    )
                    .unwrap_or([0; 4]);
                for channel in 0..4 {
                    sums[channel] += i64::from(pixel[channel]);
                }
            }
            let mut out = [0_u8; 4];
            for channel in 0..4 {
                out[channel] = (sums[channel] / steps).clamp(0, 255) as u8;
            }
            image.set_pixel(x as u32, y as u32, out);
        }
    }
}

fn rgb_to_hsl(color: [f64; 3]) -> (f64, f64, f64) {
    let max = color[0].max(color[1]).max(color[2]);
    let min = color[0].min(color[1]).min(color[2]);
    let lightness = (max + min) / 2.0;
    if (max - min).abs() < 1e-12 {
        return (0.0, 0.0, lightness);
    }
    let delta = max - min;
    let saturation = if lightness > 0.5 {
        delta / (2.0 - max - min)
    } else {
        delta / (max + min)
    };
    let hue = if (max - color[0]).abs() < 1e-12 {
        ((color[1] - color[2]) / delta).rem_euclid(6.0)
    } else if (max - color[1]).abs() < 1e-12 {
        (color[2] - color[0]) / delta + 2.0
    } else {
        (color[0] - color[1]) / delta + 4.0
    } / 6.0;
    (hue.rem_euclid(1.0), saturation.clamp(0.0, 1.0), lightness)
}

fn hsl_to_rgb(hue: f64, saturation: f64, lightness: f64) -> [f64; 3] {
    if saturation.abs() < 1e-12 {
        return [lightness; 3];
    }
    let q = if lightness < 0.5 {
        lightness * (1.0 + saturation)
    } else {
        lightness + saturation - lightness * saturation
    };
    let p = 2.0 * lightness - q;
    [
        hue_to_channel(p, q, hue + 1.0 / 3.0),
        hue_to_channel(p, q, hue),
        hue_to_channel(p, q, hue - 1.0 / 3.0),
    ]
}

fn hue_to_channel(p: f64, q: f64, t: f64) -> f64 {
    let t = t.rem_euclid(1.0);
    if t < 1.0 / 6.0 {
        p + (q - p) * 6.0 * t
    } else if t < 0.5 {
        q
    } else if t < 2.0 / 3.0 {
        p + (q - p) * (2.0 / 3.0 - t) * 6.0
    } else {
        p
    }
}

fn srgb_to_linear(value: f64) -> f64 {
    if value <= 0.040_45 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(value: f64) -> f64 {
    if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
}

/// Degrees wrapped into −180…180, which is how the dialogs' sliders behave.
#[must_use]
pub fn wrap_degrees(value: f64) -> f64 {
    let value = value % 360.0;
    if value > 180.0 {
        value - 360.0
    } else if value < -180.0 {
        value + 360.0
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invert_keeps_alpha() {
        let mut image = PremulImage::filled(1, 1, [200, 100, 0, 255]);
        invert(&mut image);
        assert_eq!(image.pixel(0, 0), Some([55, 155, 255, 255]));
        let mut half = PremulImage::new(1, 1);
        half.set_pixel(0, 0, [100, 50, 0, 128]);
        invert(&mut half);
        let pixel = half.pixel(0, 0).expect("pixel");
        assert_eq!(pixel[3], 128);
        assert_eq!(pixel[0], 28);
    }

    #[test]
    #[allow(clippy::needless_range_loop)] // channels are addressed by number
    fn levels_lookup_is_identity_by_default() {
        let settings = LevelsSettings::default();
        let tables = settings.lookup_tables();
        for value in 0..256 {
            assert!((tables[value] - value as f32).abs() < 1e-3);
        }
        assert!(settings.is_identity());
    }

    #[test]
    fn levels_gamma_brightens_the_midtones() {
        let settings = LevelsSettings {
            ranges: [
                LevelRange {
                    gamma: 2.0,
                    ..LevelRange::default()
                },
                LevelRange::default(),
                LevelRange::default(),
                LevelRange::default(),
            ],
        };
        let tables = settings.lookup_tables();
        assert!(tables[128] > 160.0, "got {}", tables[128]);
        assert!(tables[0].abs() < 1e-3);
        assert!((tables[255] - 255.0).abs() < 1e-3);
    }

    #[test]
    fn curve_with_a_midpoint_bends_the_lut() {
        let mut curves = CurvesSettings::default();
        curves.channels[0] = CurveChannel {
            points: vec![[0.0, 0.0], [128.0, 180.0], [255.0, 255.0]],
        };
        let tables = curves.lookup_tables();
        assert!(tables[128] > 170.0 && tables[128] < 190.0);
        assert!(tables[0].abs() < 1e-3);
        assert!((tables[255] - 255.0).abs() < 1e-3);
        let mut straight = CurvesSettings::default();
        straight.channels[0] = CurveChannel {
            points: vec![[0.0, 0.0], [128.0, 128.0], [255.0, 255.0]],
        };
        for value in 0..256 {
            assert!((straight.lookup_tables()[value] - value as f32).abs() < 1.5);
        }
    }

    #[test]
    fn hue_saturation_shifts_hue() {
        let mut image = PremulImage::filled(1, 1, [255, 0, 0, 255]);
        hue_saturation(&mut image, 120.0, 0.0, 0.0, false);
        let pixel = image.pixel(0, 0).expect("pixel");
        assert!(pixel[1] > 240 && pixel[0] < 15, "got {pixel:?}");
    }

    #[test]
    fn hue_saturation_colorize_paints_a_flat_gray() {
        let mut image = PremulImage::filled(1, 1, [128, 128, 128, 255]);
        hue_saturation(&mut image, 0.0, 100.0, 0.0, true);
        let pixel = image.pixel(0, 0).expect("pixel");
        // Colorize maps hue 0 to 180°, which is cyan: no red, full green and blue.
        assert!(
            pixel[0] < 20 && pixel[1] > 235 && pixel[2] > 235,
            "colorize should paint cyan: {pixel:?}"
        );
    }

    #[test]
    fn exposure_doubles_linear_light() {
        let mut image = PremulImage::filled(1, 1, [128, 128, 128, 255]);
        apply_exposure(&mut image, 1.0, 0.0, 1.0);
        let pixel = image.pixel(0, 0).expect("pixel");
        let expected = linear_to_srgb(srgb_to_linear(128.0 / 255.0) * 2.0) * 255.0;
        assert!((f64::from(pixel[0]) - expected).abs() < 2.0);
    }

    #[test]
    fn blur_softens_an_edge() {
        let mut image = PremulImage::new(16, 16);
        for y in 0..16 {
            for x in 0..8 {
                image.set_pixel(x, y, [255, 255, 255, 255]);
            }
        }
        let soft = blurred(&image, 4.0);
        let edge = soft.pixel(8, 8).expect("pixel");
        assert!(
            edge[0] > 0 && edge[0] < 255,
            "edge should be partial: {edge:?}"
        );
        assert_eq!(soft.pixel(0, 8).expect("pixel")[0], 255);
    }

    #[test]
    fn motion_blur_averages_along_its_angle() {
        let mut image = PremulImage::new(16, 3);
        image.set_pixel(4, 1, [255, 255, 255, 255]);
        motion_blur(&mut image, 0.0, 5.0);
        let spread = image.pixel(6, 1).expect("pixel");
        assert!(spread[0] > 0, "the streak should reach x=6: {spread:?}");
    }

    #[test]
    fn gradient_map_writes_the_table() {
        let table = gradient_table([0, 0, 0], [255, 0, 0], false);
        assert_eq!(&table[0..3], &[0, 0, 0]);
        assert_eq!(&table[255 * 3..255 * 3 + 3], &[255, 0, 0]);
        let reversed = gradient_table([0, 0, 0], [255, 0, 0], true);
        assert_eq!(&reversed[0..3], &[255, 0, 0]);
    }

    #[test]
    fn adjustment_limited_to_coverage_leaves_the_rest_alone() {
        let mut image = PremulImage::filled(4, 1, [10, 10, 10, 255]);
        let mut coverage = GrayImage::new(4, 1, 0);
        coverage.set_pixel(2, 0, 255);
        apply_adjustment(&Adjustment::Invert, &mut image, Some(&coverage));
        assert_eq!(image.pixel(0, 0), Some([10, 10, 10, 255]));
        assert_eq!(image.pixel(2, 0), Some([245, 245, 245, 255]));
    }

    #[test]
    fn filter_reports_a_missing_dither_kernel_clearly() {
        let mut image = PremulImage::filled(8, 8, [128, 128, 128, 255]);
        let result = Filter::Dither(DitherSettings::default()).apply(&mut image);
        if ffi::dither_available() {
            assert!(result.is_ok());
        } else {
            assert!(result.unwrap_err().contains("libblocksruntime-dev"));
        }
    }

    #[test]
    fn degrees_wrap_into_the_signed_range() {
        assert!((wrap_degrees(370.0) - 10.0).abs() < 1e-9);
        assert!((wrap_degrees(-190.0) - 170.0).abs() < 1e-9);
    }
}

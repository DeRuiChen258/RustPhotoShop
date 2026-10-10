//! Layer blend modes.
//!
//! All 24 modes of the original application, in Photoshop's order, with the same names as the
//! `.comp` manifest uses. The math is the standard PDF/CSS compositing formula, which is what
//! the original implements on the GPU:
//!
//! ```text
//! Co = as·(1−ab)·Cs + as·ab·B(Cb,Cs) + (1−as)·ab·Cb
//! Ao = as + ab·(1−as)
//! ```

use crate::core::pixel::{Rgba, over};

/// A layer blend mode, named exactly as the project format spells it.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, Default,
)]
pub enum BlendMode {
    #[serde(rename = "Normal")]
    #[default]
    Normal,
    #[serde(rename = "Darken")]
    Darken,
    #[serde(rename = "Multiply")]
    Multiply,
    #[serde(rename = "Color Burn")]
    ColorBurn,
    #[serde(rename = "Linear Burn")]
    LinearBurn,
    #[serde(rename = "Lighten")]
    Lighten,
    #[serde(rename = "Screen")]
    Screen,
    #[serde(rename = "Color Dodge")]
    ColorDodge,
    #[serde(rename = "Linear Dodge (Add)")]
    LinearDodge,
    #[serde(rename = "Overlay")]
    Overlay,
    #[serde(rename = "Soft Light")]
    SoftLight,
    #[serde(rename = "Hard Light")]
    HardLight,
    #[serde(rename = "Vivid Light")]
    VividLight,
    #[serde(rename = "Linear Light")]
    LinearLight,
    #[serde(rename = "Pin Light")]
    PinLight,
    #[serde(rename = "Hard Mix")]
    HardMix,
    #[serde(rename = "Difference")]
    Difference,
    #[serde(rename = "Exclusion")]
    Exclusion,
    #[serde(rename = "Subtract")]
    Subtract,
    #[serde(rename = "Divide")]
    Divide,
    #[serde(rename = "Hue")]
    Hue,
    #[serde(rename = "Saturation")]
    Saturation,
    #[serde(rename = "Color")]
    Color,
    #[serde(rename = "Luminosity")]
    Luminosity,
}

impl BlendMode {
    /// Every mode, in Photoshop's menu order, with the group breaks of the original.
    pub const GROUPS: &'static [&'static [Self]] = &[
        &[Self::Normal],
        &[
            Self::Darken,
            Self::Multiply,
            Self::ColorBurn,
            Self::LinearBurn,
        ],
        &[
            Self::Lighten,
            Self::Screen,
            Self::ColorDodge,
            Self::LinearDodge,
        ],
        &[
            Self::Overlay,
            Self::SoftLight,
            Self::HardLight,
            Self::VividLight,
            Self::LinearLight,
            Self::PinLight,
            Self::HardMix,
        ],
        &[
            Self::Difference,
            Self::Exclusion,
            Self::Subtract,
            Self::Divide,
        ],
        &[Self::Hue, Self::Saturation, Self::Color, Self::Luminosity],
    ];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Darken => "Darken",
            Self::Multiply => "Multiply",
            Self::ColorBurn => "Color Burn",
            Self::LinearBurn => "Linear Burn",
            Self::Lighten => "Lighten",
            Self::Screen => "Screen",
            Self::ColorDodge => "Color Dodge",
            Self::LinearDodge => "Linear Dodge (Add)",
            Self::Overlay => "Overlay",
            Self::SoftLight => "Soft Light",
            Self::HardLight => "Hard Light",
            Self::VividLight => "Vivid Light",
            Self::LinearLight => "Linear Light",
            Self::PinLight => "Pin Light",
            Self::HardMix => "Hard Mix",
            Self::Difference => "Difference",
            Self::Exclusion => "Exclusion",
            Self::Subtract => "Subtract",
            Self::Divide => "Divide",
            Self::Hue => "Hue",
            Self::Saturation => "Saturation",
            Self::Color => "Color",
            Self::Luminosity => "Luminosity",
        }
    }
}

/// Composites one premultiplied source pixel over a premultiplied backdrop pixel.
#[must_use]
#[allow(clippy::needless_range_loop)] // color channels are addressed by number here
pub fn blend_pixel(mode: BlendMode, backdrop: Rgba, source: Rgba, opacity: f32) -> Rgba {
    if opacity <= 0.0 || source[3] == 0 {
        return backdrop;
    }
    if mode == BlendMode::Normal {
        return over(backdrop, source, opacity);
    }
    let alpha_backdrop = f32::from(backdrop[3]) / 255.0;
    let alpha_source = f32::from(source[3]) / 255.0 * opacity.clamp(0.0, 1.0);
    if alpha_backdrop == 0.0 && alpha_source == 0.0 {
        return backdrop;
    }
    // Straight (unpremultiplied) colors; a transparent pixel contributes no color.
    let backdrop_color = unpremultiply(backdrop, f32::from(backdrop[3]) / 255.0);
    let source_color = unpremultiply(source, f32::from(source[3]) / 255.0);
    let blended = blend_color(mode, backdrop_color, source_color);
    let mut out = [0_u8; 4];
    for channel in 0..3 {
        let value = alpha_source * (1.0 - alpha_backdrop) * source_color[channel]
            + alpha_source * alpha_backdrop * blended[channel]
            + (1.0 - alpha_source) * alpha_backdrop * backdrop_color[channel];
        out[channel] = (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    }
    let alpha = alpha_source + alpha_backdrop * (1.0 - alpha_source);
    out[3] = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
    out
}

fn unpremultiply(pixel: Rgba, alpha: f32) -> [f32; 3] {
    if alpha <= 0.0 {
        return [0.0; 3];
    }
    [
        (f32::from(pixel[0]) / 255.0 / alpha).clamp(0.0, 1.0),
        (f32::from(pixel[1]) / 255.0 / alpha).clamp(0.0, 1.0),
        (f32::from(pixel[2]) / 255.0 / alpha).clamp(0.0, 1.0),
    ]
}

/// The blend function `B(Cb, Cs)` for straight colors in 0–1.
#[must_use]
pub fn blend_color(mode: BlendMode, backdrop: [f32; 3], source: [f32; 3]) -> [f32; 3] {
    match mode {
        BlendMode::Normal => source,
        BlendMode::Hue | BlendMode::Saturation | BlendMode::Color | BlendMode::Luminosity => {
            non_separable(mode, backdrop, source)
        }
        _ => {
            let mut out = [0.0_f32; 3];
            for channel in 0..3 {
                out[channel] = separable(mode, backdrop[channel], source[channel]);
            }
            out
        }
    }
}

fn separable(mode: BlendMode, backdrop: f32, source: f32) -> f32 {
    let (cb, cs) = (backdrop.clamp(0.0, 1.0), source.clamp(0.0, 1.0));
    let value = match mode {
        BlendMode::Normal => cs,
        BlendMode::Multiply => cb * cs,
        BlendMode::Screen => cb + cs - cb * cs,
        BlendMode::Darken => cb.min(cs),
        BlendMode::Lighten => cb.max(cs),
        BlendMode::ColorBurn => {
            if cs <= 0.0 {
                0.0
            } else {
                (1.0 - (1.0 - cb) / cs).max(0.0)
            }
        }
        BlendMode::LinearBurn => (cb + cs - 1.0).max(0.0),
        BlendMode::ColorDodge => {
            if cs >= 1.0 {
                1.0
            } else {
                (cb / (1.0 - cs)).min(1.0)
            }
        }
        BlendMode::LinearDodge => (cb + cs).min(1.0),
        BlendMode::Overlay => hard_light(cs, cb),
        BlendMode::SoftLight => soft_light(cb, cs),
        BlendMode::HardLight => hard_light(cb, cs),
        BlendMode::VividLight => {
            if cs <= 0.5 {
                separable(BlendMode::ColorBurn, cb, (2.0 * cs).clamp(0.0, 1.0))
            } else {
                separable(BlendMode::ColorDodge, cb, (2.0 * cs - 1.0).clamp(0.0, 1.0))
            }
        }
        BlendMode::LinearLight => (cb + 2.0 * cs - 1.0).clamp(0.0, 1.0),
        BlendMode::PinLight => {
            if cs <= 0.5 {
                cb.min(2.0 * cs)
            } else {
                cb.max(2.0 * cs - 1.0)
            }
        }
        BlendMode::HardMix => f32::from(separable(BlendMode::VividLight, cb, cs) >= 0.5),
        BlendMode::Difference => (cb - cs).abs(),
        BlendMode::Exclusion => cb + cs - 2.0 * cb * cs,
        BlendMode::Subtract => (cb - cs).max(0.0),
        BlendMode::Divide => {
            if cs <= 0.0 {
                1.0
            } else {
                (cb / cs).min(1.0)
            }
        }
        BlendMode::Hue | BlendMode::Saturation | BlendMode::Color | BlendMode::Luminosity => cs,
    };
    value.clamp(0.0, 1.0)
}

fn hard_light(backdrop: f32, source: f32) -> f32 {
    if source <= 0.5 {
        separable(
            BlendMode::Multiply,
            backdrop,
            (2.0 * source).clamp(0.0, 1.0),
        )
    } else {
        separable(
            BlendMode::Screen,
            backdrop,
            (2.0 * source - 1.0).clamp(0.0, 1.0),
        )
    }
}

/// Photoshop's soft light, which is the W3C formula with the `sqrt` shoulder.
fn soft_light(backdrop: f32, source: f32) -> f32 {
    if source <= 0.5 {
        backdrop - (1.0 - 2.0 * source) * backdrop * (1.0 - backdrop)
    } else {
        let d = if backdrop <= 0.25 {
            ((16.0 * backdrop - 12.0) * backdrop + 4.0) * backdrop
        } else {
            backdrop.sqrt()
        };
        backdrop + (2.0 * source - 1.0) * (d - backdrop)
    }
}

fn non_separable(mode: BlendMode, backdrop: [f32; 3], source: [f32; 3]) -> [f32; 3] {
    match mode {
        BlendMode::Hue => set_luminosity(
            set_saturation(source, saturation(backdrop)),
            luminosity(backdrop),
        ),
        BlendMode::Saturation => set_luminosity(
            set_saturation(backdrop, saturation(source)),
            luminosity(backdrop),
        ),
        BlendMode::Color => set_luminosity(source, luminosity(backdrop)),
        BlendMode::Luminosity => set_luminosity(backdrop, luminosity(source)),
        _ => source,
    }
}

#[must_use]
pub fn luminosity(color: [f32; 3]) -> f32 {
    0.3 * color[0] + 0.59 * color[1] + 0.11 * color[2]
}

fn set_luminosity(color: [f32; 3], target: f32) -> [f32; 3] {
    let delta = target - luminosity(color);
    clip_color([color[0] + delta, color[1] + delta, color[2] + delta])
}

fn saturation(color: [f32; 3]) -> f32 {
    color[0].max(color[1]).max(color[2]) - color[0].min(color[1]).min(color[2])
}

fn set_saturation(color: [f32; 3], target: f32) -> [f32; 3] {
    let max = color[0].max(color[1]).max(color[2]);
    let min = color[0].min(color[1]).min(color[2]);
    if max <= min {
        return [0.0; 3];
    }
    let scale = target / (max - min);
    [
        (color[0] - min) * scale,
        (color[1] - min) * scale,
        (color[2] - min) * scale,
    ]
}

#[allow(clippy::needless_range_loop)] // channels are addressed by number
fn clip_color(color: [f32; 3]) -> [f32; 3] {
    let luminance = luminosity(color);
    let min = color[0].min(color[1]).min(color[2]);
    let max = color[0].max(color[1]).max(color[2]);
    let mut out = color;
    if min < 0.0 {
        let denominator = luminance - min;
        if denominator > 0.0 {
            for channel in 0..3 {
                out[channel] = luminance + (out[channel] - luminance) * luminance / denominator;
            }
        }
    }
    if max > 1.0 {
        let denominator = max - luminance;
        if denominator > 0.0 {
            for channel in 0..3 {
                out[channel] =
                    luminance + (out[channel] - luminance) * (1.0 - luminance) / denominator;
            }
        }
    }
    [
        out[0].clamp(0.0, 1.0),
        out[1].clamp(0.0, 1.0),
        out[2].clamp(0.0, 1.0),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opaque(red: u8, green: u8, blue: u8) -> Rgba {
        [red, green, blue, 255]
    }

    #[test]
    fn normal_mode_is_plain_source_over() {
        let out = blend_pixel(BlendMode::Normal, opaque(255, 0, 0), opaque(0, 0, 255), 1.0);
        assert_eq!(out, opaque(0, 0, 255));
    }

    #[test]
    fn multiply_darkens_and_screen_lightens() {
        let multiply = blend_pixel(
            BlendMode::Multiply,
            opaque(128, 128, 128),
            opaque(128, 128, 128),
            1.0,
        );
        assert_eq!(multiply[0], 64);
        let screen = blend_pixel(
            BlendMode::Screen,
            opaque(128, 128, 128),
            opaque(128, 128, 128),
            1.0,
        );
        assert_eq!(screen[0], 192);
    }

    #[test]
    fn difference_and_exclusion_match_known_values() {
        let difference = blend_pixel(
            BlendMode::Difference,
            opaque(200, 100, 50),
            opaque(50, 100, 200),
            1.0,
        );
        assert_eq!(difference, opaque(150, 0, 150));
        let exclusion = blend_pixel(
            BlendMode::Exclusion,
            opaque(128, 0, 255),
            opaque(128, 255, 0),
            1.0,
        );
        // Mid gray excluded with itself is mid gray: 2·0.502 − 2·0.502² = 0.5.
        assert_eq!(exclusion, opaque(127, 255, 255));
    }

    #[test]
    fn transparent_source_leaves_the_backdrop_alone() {
        let base = opaque(10, 20, 30);
        for mode in [
            BlendMode::Multiply,
            BlendMode::Luminosity,
            BlendMode::HardMix,
        ] {
            assert_eq!(blend_pixel(mode, base, [0, 0, 0, 0], 1.0), base);
        }
    }

    #[test]
    fn opacity_scales_the_source() {
        let out = blend_pixel(
            BlendMode::Normal,
            opaque(0, 0, 0),
            opaque(255, 255, 255),
            0.5,
        );
        assert!(out[0] >= 126 && out[0] <= 129, "got {:?}", out);
    }

    #[test]
    fn luminosity_mode_keeps_the_backdrop_hue() {
        let out = blend_pixel(
            BlendMode::Luminosity,
            opaque(200, 40, 40),
            opaque(128, 128, 128),
            1.0,
        );
        assert!(out[0] > out[1] && out[1] == out[2]);
        assert!(out[0] > 200, "should be brighter than the backdrop");
    }
}

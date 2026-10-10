//! The `.comp` project: a folder holding `manifest.json` and an `images/` directory of PNGs.
//!
//! The layout, the metadata and the limits follow `docs/project-format.md` in the macOS
//! project, so a project saved by either build opens in the other. Layer fields this build does
//! not render (text, effects, shapes, clipping links) are carried through untouched.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::core::adjustment::{
    Adjustment, ColorBalanceSettings, CurveChannel, CurvesSettings, ExposureSettings,
    GradientMapSettings, LevelRange, LevelsSettings,
};
use crate::core::blend::BlendMode;
use crate::core::document::{Document, Layer, LayerKind, Mask, Sampling, Transform, check_size};
use crate::core::pixel::{GrayImage, PremulImage};
use crate::io::image_io;

/// The manifest's `format` value, which both builds check before anything else.
pub const FORMAT: &str = "com.compositor.project";
/// The version new saves write.
pub const VERSION: i64 = 11;
/// The versions this build can read.
pub const SUPPORTED: std::ops::RangeInclusive<i64> = 1..=VERSION;
/// The manifest size limit the macOS build enforces.
const MAX_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;

/// Everything that can go wrong with a project file.
#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("{0} 不是 RustPhotoShop 工程（其中没有 manifest.json）")]
    NotAProject(PathBuf),
    #[error("该工程使用格式版本 {0}；本版本支持版本 1–11")]
    UnsupportedVersion(i64),
    #[error("工程元数据损坏：{0}")]
    Invalid(String),
    #[error("工程内的图像缺失或损坏：{0}")]
    MissingAsset(String),
    #[error("无法读写 {path}：{source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("工程超出支持的画布尺寸、图层数量或文件大小上限")]
    TooLarge,
}

impl ProjectError {
    fn io(path: &Path, source: std::io::Error) -> Self {
        Self::Io {
            path: path.to_path_buf(),
            source,
        }
    }
}

/// True when the path is a project package: a directory with a `manifest.json`.
#[must_use]
pub fn is_project(path: &Path) -> bool {
    path.is_dir() && path.join("manifest.json").is_file()
}

/// True when the path has the project's extension.
#[must_use]
pub fn has_project_extension(path: &Path) -> bool {
    image_io::extension(path).as_deref() == Some("comp")
}

// ---------------------------------------------------------------------------------------------
// The manifest, field for field, with unknown fields preserved.
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
struct Manifest {
    format: String,
    version: i64,
    #[serde(rename = "colorSpace", default = "default_color_space")]
    color_space: String,
    #[serde(rename = "documentID")]
    document_id: String,
    width: u32,
    height: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    resolution: Option<f64>,
    #[serde(
        rename = "activeLayerID",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    active_layer_id: Option<String>,
    layers: Vec<Record>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    guides: Option<Vec<GuideRecord>>,
    #[serde(flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Record {
    id: String,
    name: String,
    #[serde(rename = "isVisible", default = "yes")]
    is_visible: bool,
    transform: TransformRecord,
    #[serde(rename = "imageFile", default, skip_serializing_if = "Option::is_none")]
    image_file: Option<String>,
    #[serde(rename = "parentID", default, skip_serializing_if = "Option::is_none")]
    parent_id: Option<String>,
    #[serde(rename = "isGroup", default, skip_serializing_if = "Option::is_none")]
    is_group: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    opacity: Option<f64>,
    #[serde(rename = "blendMode", default, skip_serializing_if = "Option::is_none")]
    blend_mode: Option<BlendMode>,
    #[serde(rename = "maskFile", default, skip_serializing_if = "Option::is_none")]
    mask_file: Option<String>,
    #[serde(
        rename = "maskEnabled",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    mask_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    adjustment: Option<serde_json::Value>,
    #[serde(flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
struct TransformRecord {
    origin: [f64; 2],
    size: [f64; 2],
    #[serde(default)]
    rotation: f64,
    #[serde(rename = "flipX", default)]
    flip_x: bool,
    #[serde(rename = "flipY", default)]
    flip_y: bool,
    #[serde(default)]
    sampling: Sampling,
}

#[derive(Debug, Serialize, Deserialize)]
struct GuideRecord {
    id: String,
    axis: String,
    position: f64,
}

fn default_color_space() -> String {
    "sRGB".to_owned()
}

fn yes() -> bool {
    true
}

fn uuid_from(text: &str) -> Result<Uuid, ProjectError> {
    Uuid::parse_str(text).map_err(|_| ProjectError::Invalid(format!("{text} 不是合法的 UUID")))
}

fn id_text(id: Uuid) -> String {
    id.to_string().to_uppercase()
}

// ---------------------------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------------------------

/// Reads a project into a document.
///
/// # Errors
///
/// Fails when the package is missing, the metadata is damaged or a newer version, an image is
/// missing, or the sizes are outside the format's limits.
pub fn load(path: &Path) -> Result<Document, ProjectError> {
    if !is_project(path) {
        return Err(ProjectError::NotAProject(path.to_path_buf()));
    }
    let manifest_path = path.join("manifest.json");
    let metadata = std::fs::metadata(&manifest_path)
        .map_err(|error| ProjectError::io(&manifest_path, error))?;
    if metadata.len() > MAX_MANIFEST_BYTES {
        return Err(ProjectError::TooLarge);
    }
    let bytes =
        std::fs::read(&manifest_path).map_err(|error| ProjectError::io(&manifest_path, error))?;
    let manifest: Manifest = serde_json::from_slice(&bytes)
        .map_err(|error| ProjectError::Invalid(format!("manifest.json：{error}")))?;
    build_document(path, manifest)
}

/// Reads a project from manifest bytes plus a lookup for its assets, which is what the tests
/// and the round-trip checks use.
///
/// # Errors
///
/// Fails under the same conditions as [`load`].
pub fn load_from_parts(
    bytes: &[u8],
    mut asset: impl FnMut(&str) -> Option<Vec<u8>>,
) -> Result<Document, ProjectError> {
    let manifest: Manifest = serde_json::from_slice(bytes)
        .map_err(|error| ProjectError::Invalid(format!("manifest.json：{error}")))?;
    build_from(manifest, &mut asset)
}

fn build_document(root: &Path, manifest: Manifest) -> Result<Document, ProjectError> {
    // The manifest names its images by file name alone; they always live in `images/`.
    let images = root.join("images");
    build_from(manifest, &mut |name: &str| {
        if unsafe_name(name) {
            return None;
        }
        std::fs::read(images.join(name)).ok()
    })
}

/// True when an asset name tries to leave the package.
fn unsafe_name(name: &str) -> bool {
    name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || name.starts_with('.')
}

fn build_from(
    manifest: Manifest,
    asset: &mut impl FnMut(&str) -> Option<Vec<u8>>,
) -> Result<Document, ProjectError> {
    if manifest.format != FORMAT {
        return Err(ProjectError::Invalid(format!(
            "manifest 声明的格式为“{}”",
            manifest.format
        )));
    }
    if !SUPPORTED.contains(&manifest.version) {
        return Err(ProjectError::UnsupportedVersion(manifest.version));
    }
    check_size(manifest.width, manifest.height).map_err(|_| ProjectError::TooLarge)?;
    if manifest.layers.len() > 10_000 {
        return Err(ProjectError::TooLarge);
    }
    let mut document = Document::new(manifest.width, manifest.height);
    document.id = uuid_from(&manifest.document_id)?;
    document.resolution = manifest.resolution.unwrap_or(72.0);
    document.color_space = manifest.color_space.clone();
    document.extra = manifest.extra;

    let mut seen: BTreeMap<Uuid, ()> = BTreeMap::new();
    for record in &manifest.layers {
        let id = uuid_from(&record.id)?;
        if seen.insert(id, ()).is_some() {
            return Err(ProjectError::Invalid(format!(
                "图层 ID {} 重复出现",
                record.id
            )));
        }
        let is_group = record.is_group.unwrap_or(false);
        if is_group && record.image_file.is_some() {
            return Err(ProjectError::Invalid(format!(
                "文件夹 {} 中带有像素数据",
                record.name
            )));
        }
        let image = match &record.image_file {
            Some(name) => {
                let bytes = asset(name).ok_or_else(|| ProjectError::MissingAsset(name.clone()))?;
                Some(std::sync::Arc::new(
                    image_io::decode(&bytes, Path::new(name))
                        .map_err(|error| ProjectError::MissingAsset(error.to_string()))?,
                ))
            }
            None => None,
        };
        let mask = match &record.mask_file {
            Some(name) => {
                let bytes = asset(name).ok_or_else(|| ProjectError::MissingAsset(name.clone()))?;
                let decoded = image_io::decode(&bytes, Path::new(name))
                    .map_err(|error| ProjectError::MissingAsset(error.to_string()))?;
                // Masks are 8-bit grayscale without alpha; the red channel carries the coverage.
                let mut gray = GrayImage::new(decoded.width(), decoded.height(), 0);
                for y in 0..decoded.height() {
                    for x in 0..decoded.width() {
                        let pixel = decoded.pixel(x, y).unwrap_or([0; 4]);
                        gray.set_pixel(x, y, pixel[0]);
                    }
                }
                Some(Mask {
                    image: std::sync::Arc::new(gray),
                    linked: record
                        .extra
                        .get("maskLinked")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(true),
                    placement: record.extra.get("maskPlacement").and_then(parse_transform),
                })
            }
            None => None,
        };
        let (adjustment, adjustment_raw) = match (&record.adjustment, is_group) {
            (Some(value), false) => (parse_adjustment(value), Some(value.clone())),
            _ => (None, None),
        };
        let kind = if is_group {
            LayerKind::Group
        } else if record.adjustment.is_some() {
            LayerKind::Adjustment
        } else {
            LayerKind::Pixel
        };
        document.layers.push(Layer {
            id,
            name: record.name.clone(),
            kind,
            visible: record.is_visible,
            parent: match &record.parent_id {
                Some(parent) => Some(uuid_from(parent)?),
                None => None,
            },
            opacity: record.opacity.unwrap_or(1.0).clamp(0.0, 1.0),
            blend: record.blend_mode.unwrap_or_default(),
            transform: Transform {
                origin: record.transform.origin,
                size: record.transform.size,
                rotation: record.transform.rotation,
                flip_x: record.transform.flip_x,
                flip_y: record.transform.flip_y,
                sampling: record.transform.sampling,
            },
            image,
            mask,
            mask_enabled: record.mask_enabled.unwrap_or(true),
            adjustment,
            adjustment_raw,
            extra: record.extra.clone(),
        });
    }
    // Parent links must point at folders that exist, and no folder may contain itself.
    for layer in &document.layers {
        if let Some(parent) = layer.parent {
            if !document.is_group(parent) {
                return Err(ProjectError::Invalid(format!(
                    "{} 的父级不是文件夹",
                    layer.name
                )));
            }
            if parent == layer.id || document.is_ancestor(layer.id, parent) {
                return Err(ProjectError::Invalid(format!(
                    "{} 的层级形成环",
                    layer.name
                )));
            }
        }
    }
    document.active_layer = manifest
        .active_layer_id
        .as_deref()
        .and_then(|text| Uuid::parse_str(text).ok())
        .filter(|id| document.layer(*id).is_some())
        .or_else(|| document.layers.last().map(|layer| layer.id));
    document.guides = manifest
        .guides
        .unwrap_or_default()
        .into_iter()
        .filter_map(|guide| {
            Some(crate::core::document::Guide {
                id: Uuid::parse_str(&guide.id).ok()?,
                horizontal: guide.axis == "horizontal",
                position: guide.position,
            })
        })
        .collect();
    document.path = None;
    document.dirty = false;
    Ok(document)
}

/// Reads the adjustment record, returning `None` for kinds this build does not render (their
/// JSON is kept so it survives a save).
fn parse_adjustment(value: &serde_json::Value) -> Option<Adjustment> {
    let kind = value.get("kind")?.as_str()?;
    let number = |key: &str| value.get(key).and_then(serde_json::Value::as_f64);
    let flag = |key: &str| value.get(key).and_then(serde_json::Value::as_bool);
    Some(match kind {
        "Invert" => Adjustment::Invert,
        "Levels" => Adjustment::Levels(parse_levels(value)),
        "Curves" => Adjustment::Curves(parse_curves(value)),
        "Hue/Saturation" => Adjustment::HueSaturation {
            hue: number("hue").unwrap_or(0.0),
            saturation: number("saturation").unwrap_or(0.0),
            lightness: number("lightness").unwrap_or(0.0),
            colorize: flag("colorize").unwrap_or(false),
        },
        "Exposure" => Adjustment::Exposure(parse_exposure(value)),
        "Black & White" => Adjustment::BlackAndWhite {
            weights: parse_weights(value),
            tint: flag("bwTint").or_else(|| flag("tint")).unwrap_or(false),
            tint_hue: number("bwTintHue")
                .or_else(|| number("tintHue"))
                .unwrap_or(0.0),
            tint_saturation: number("bwTintSaturation")
                .or_else(|| number("tintSaturation"))
                .unwrap_or(0.0),
        },
        "Color Balance" => Adjustment::ColorBalance(parse_color_balance(value)),
        "Gradient Map" => Adjustment::GradientMap(parse_gradient_map(value)),
        "Gaussian Blur" => Adjustment::GaussianBlur {
            radius: number("blurRadius").unwrap_or(0.0),
        },
        "Motion Blur" => Adjustment::MotionBlur {
            angle: number("motionAngle").unwrap_or(0.0),
            distance: number("motionDistance").unwrap_or(0.0),
        },
        "Add Noise" => Adjustment::AddNoise {
            amount: number("noiseAmount").unwrap_or(0.0),
            gaussian: flag("noiseGaussian").unwrap_or(false),
            monochromatic: flag("noiseMonochromatic").unwrap_or(false),
            seed: number("noiseSeed").unwrap_or(0.0) as u32,
        },
        "Grain" => Adjustment::Grain(parse_grain(value)),
        _ => return None,
    })
}

/// Reads a stored transform, which the format writes the same way for layers and for masks.
fn parse_transform(value: &serde_json::Value) -> Option<Transform> {
    let pair = |key: &str| -> Option<[f64; 2]> {
        let values = value.get(key)?.as_array()?;
        Some([values.first()?.as_f64()?, values.get(1)?.as_f64()?])
    };
    Some(Transform {
        origin: pair("origin")?,
        size: pair("size")?,
        rotation: value
            .get("rotation")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.0),
        flip_x: value
            .get("flipX")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        flip_y: value
            .get("flipY")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        sampling: value
            .get("sampling")
            .and_then(|sampling| serde_json::from_value(sampling.clone()).ok())
            .unwrap_or_default(),
    })
}

/// Writes a transform back in the format's shape, for a mask that has been moved.
fn transform_to_json(transform: &Transform) -> serde_json::Value {
    serde_json::json!({
        "origin": transform.origin,
        "size": transform.size,
        "rotation": transform.rotation,
        "flipX": transform.flip_x,
        "flipY": transform.flip_y,
        "sampling": transform.sampling,
    })
}

fn parse_levels(value: &serde_json::Value) -> LevelsSettings {
    let mut settings = LevelsSettings::default();
    let Some(ranges) = value
        .get("levels")
        .and_then(|levels| levels.get("ranges"))
        .and_then(serde_json::Value::as_array)
    else {
        return settings;
    };
    for (index, range) in ranges.iter().take(4).enumerate() {
        let number = |key: &str, fallback: f64| {
            range
                .get(key)
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(fallback)
        };
        settings.ranges[index] = LevelRange {
            black: number("black", 0.0),
            gamma: number("gamma", 1.0),
            white: number("white", 255.0),
            output_black: number("outputBlack", 0.0),
            output_white: number("outputWhite", 255.0),
        };
    }
    settings
}

fn parse_curves(value: &serde_json::Value) -> CurvesSettings {
    let mut settings = CurvesSettings::default();
    let Some(channels) = value
        .get("curves")
        .and_then(|curves| curves.get("channels"))
        .and_then(serde_json::Value::as_array)
    else {
        return settings;
    };
    for (index, channel) in channels.iter().take(4).enumerate() {
        let Some(points) = channel.as_array() else {
            continue;
        };
        let mut parsed: Vec<[f64; 2]> = points
            .iter()
            .filter_map(|point| Some([point.get("x")?.as_f64()?, point.get("y")?.as_f64()?]))
            .collect();
        if parsed.len() >= 2 {
            parsed.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap_or(std::cmp::Ordering::Equal));
            settings.channels[index] = CurveChannel { points: parsed };
        }
    }
    settings
}

fn parse_exposure(value: &serde_json::Value) -> ExposureSettings {
    let block = value.get("exposureSettings").unwrap_or(value);
    let number = |key: &str, fallback: f64| {
        block
            .get(key)
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(fallback)
    };
    ExposureSettings {
        exposure: number("exposure", 0.0),
        offset: number("offset", 0.0),
        gamma: number("gamma", 1.0),
    }
}

fn parse_weights(value: &serde_json::Value) -> [f32; 6] {
    let block = value
        .get("blackWhiteSettings")
        .or_else(|| value.get("bwSettings"))
        .unwrap_or(value);
    let number = |key: &str| {
        block
            .get(key)
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.4) as f32
    };
    [
        number("red"),
        number("yellow"),
        number("green"),
        number("cyan"),
        number("blue"),
        number("magenta"),
    ]
}

fn parse_color_balance(value: &serde_json::Value) -> ColorBalanceSettings {
    let block = value.get("colorBalanceSettings").unwrap_or(value);
    let range = |prefix: &str| {
        let number = |suffix: &str| {
            block
                .get(format!("{prefix}{suffix}"))
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(0.0) as f32
        };
        [
            number("CyanRed"),
            number("MagentaGreen"),
            number("YellowBlue"),
        ]
    };
    ColorBalanceSettings {
        shadows: range("shadow"),
        midtones: range("mid"),
        highlights: range("highlight"),
        preserve_luminosity: block
            .get("preserveLuminosity")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true),
    }
}

fn parse_gradient_map(value: &serde_json::Value) -> GradientMapSettings {
    let block = value.get("gradientMapSettings").unwrap_or(value);
    let color = |key: &str| -> [u8; 3] {
        block
            .get(key)
            .and_then(serde_json::Value::as_array)
            .map(|channels| {
                [
                    channels
                        .first()
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(0) as u8,
                    channels
                        .get(1)
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(0) as u8,
                    channels
                        .get(2)
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(0) as u8,
                ]
            })
            .unwrap_or([0, 0, 0])
    };
    GradientMapSettings {
        shadows: color("shadows"),
        highlights: color("highlights"),
        reversed: block
            .get("reversed")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
    }
}

fn parse_grain(value: &serde_json::Value) -> crate::core::adjustment::GrainSettings {
    let block = value.get("grainSettings").unwrap_or(value);
    let number = |key: &str, fallback: f64| {
        block
            .get(key)
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(fallback)
    };
    crate::core::adjustment::GrainSettings {
        amount: number("amount", 0.0),
        size: number("size", 1.0),
        roughness: number("roughness", 0.0),
        seed: number("seed", 0.0) as u32,
    }
}

// ---------------------------------------------------------------------------------------------
// Saving
// ---------------------------------------------------------------------------------------------

/// Writes a document as a `.comp` package.
///
/// The package is built in a sibling folder first and only moved into place once every file is
/// written, so an interrupted save never leaves a half-written project behind.
///
/// # Errors
///
/// Fails when a size is out of range, an image cannot be encoded, or the package cannot be
/// written.
pub fn save(document: &mut Document, path: &Path) -> Result<(), ProjectError> {
    check_size(document.width, document.height).map_err(|_| ProjectError::TooLarge)?;
    let manifest = build_manifest(document)?;
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| ProjectError::Invalid(error.to_string()))?;
    if manifest_bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(ProjectError::TooLarge);
    }

    let staging = staging_path(path);
    if staging.exists() {
        std::fs::remove_dir_all(&staging).map_err(|error| ProjectError::io(&staging, error))?;
    }
    let images = staging.join("images");
    std::fs::create_dir_all(&images).map_err(|error| ProjectError::io(&images, error))?;

    for layer in &document.layers {
        let name = id_text(layer.id);
        if let Some(image) = &layer.image {
            let file = images.join(format!("{name}.png"));
            image_io::save_png(image, &file)
                .map_err(|error| ProjectError::Invalid(error.to_string()))?;
        }
        if let Some(mask) = &layer.mask {
            let mut gray = PremulImage::new(mask.image.width(), mask.image.height());
            for y in 0..mask.image.height() {
                for x in 0..mask.image.width() {
                    let value = mask.image.pixel(x, y);
                    gray.set_pixel(x, y, [value, value, value, 255]);
                }
            }
            let file = images.join(format!("{name}.mask.png"));
            image_io::save_png(&gray, &file)
                .map_err(|error| ProjectError::Invalid(error.to_string()))?;
        }
    }

    // Finder's Space-bar preview, which the macOS build also writes.
    let quick_look = staging.join("QuickLook");
    std::fs::create_dir_all(&quick_look).map_err(|error| ProjectError::io(&quick_look, error))?;
    let preview = crate::core::composite::composite_document(document);
    let scale = (512.0 / f64::from(preview.width().max(preview.height()))).min(1.0);
    let preview = preview.scale_to(
        (f64::from(preview.width()) * scale).round().max(1.0) as u32,
        (f64::from(preview.height()) * scale).round().max(1.0) as u32,
    );
    let preview_path = quick_look.join("Preview.jpg");
    image_io::save_jpeg(&preview, &preview_path, 85, [255, 255, 255, 255])
        .map_err(|error| ProjectError::Invalid(error.to_string()))?;

    let manifest_path = staging.join("manifest.json");
    std::fs::write(&manifest_path, &manifest_bytes)
        .map_err(|error| ProjectError::io(&manifest_path, error))?;

    swap_into_place(&staging, path)?;
    document.path = Some(path.to_path_buf());
    document.dirty = false;
    Ok(())
}

/// Where a save is built before it replaces the old package.
fn staging_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".saving-{}", std::process::id()));
    path.with_file_name(name)
}

/// Moves the finished package over the old one, keeping the old around until the move worked.
fn swap_into_place(staging: &Path, path: &Path) -> Result<(), ProjectError> {
    if path.exists() {
        let mut backup_name = path.file_name().unwrap_or_default().to_os_string();
        backup_name.push(".previous");
        let backup = path.with_file_name(backup_name);
        if backup.exists() {
            std::fs::remove_dir_all(&backup).map_err(|error| ProjectError::io(&backup, error))?;
        }
        std::fs::rename(path, &backup).map_err(|error| ProjectError::io(path, error))?;
        match std::fs::rename(staging, path) {
            Ok(()) => {
                let _ = std::fs::remove_dir_all(&backup);
                Ok(())
            }
            Err(error) => {
                // Put the old project back before reporting the failure.
                let _ = std::fs::rename(&backup, path);
                Err(ProjectError::io(path, error))
            }
        }
    } else {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|error| ProjectError::io(parent, error))?;
        }
        std::fs::rename(staging, path).map_err(|error| ProjectError::io(path, error))
    }
}

fn build_manifest(document: &Document) -> Result<Manifest, ProjectError> {
    let mut layers = Vec::new();
    for id in document.save_order() {
        let Some(layer) = document.layer(id) else {
            continue;
        };
        let name = id_text(layer.id);
        let mut extra = layer.extra.clone();
        if let Some(mask) = &layer.mask {
            extra.insert(
                "maskLinked".to_owned(),
                serde_json::Value::Bool(mask.linked),
            );
            // The record's own transform is the layer's; a mask only carries its own when it has
            // been moved apart from it.
            if let Some(placement) = mask.placement {
                extra.insert("maskPlacement".to_owned(), transform_to_json(&placement));
            } else {
                extra.remove("maskPlacement");
            }
        }
        layers.push(Record {
            id: name.clone(),
            name: layer.name.clone(),
            is_visible: layer.visible,
            transform: TransformRecord {
                origin: layer.transform.origin,
                size: layer.transform.size,
                rotation: layer.transform.rotation,
                flip_x: layer.transform.flip_x,
                flip_y: layer.transform.flip_y,
                sampling: layer.transform.sampling,
            },
            image_file: layer.image.as_ref().map(|_| format!("{name}.png")),
            parent_id: layer.parent.map(id_text),
            is_group: (layer.kind == LayerKind::Group).then_some(true),
            opacity: Some(layer.opacity),
            blend_mode: Some(layer.blend),
            mask_file: layer.mask.as_ref().map(|_| format!("{name}.mask.png")),
            mask_enabled: layer.mask.as_ref().map(|_| layer.mask_enabled),
            adjustment: match (&layer.adjustment, &layer.adjustment_raw) {
                (Some(adjustment), raw) => Some(write_adjustment(adjustment, raw.as_ref())),
                (None, Some(raw)) => Some(raw.clone()),
                (None, None) => None,
            },
            extra,
        });
    }
    Ok(Manifest {
        format: FORMAT.to_owned(),
        version: VERSION,
        color_space: if document.color_space.is_empty() {
            default_color_space()
        } else {
            document.color_space.clone()
        },
        document_id: id_text(document.id),
        width: document.width,
        height: document.height,
        resolution: Some(document.resolution),
        active_layer_id: document.active_layer.map(id_text),
        layers,
        guides: if document.guides.is_empty() {
            None
        } else {
            Some(
                document
                    .guides
                    .iter()
                    .map(|guide| GuideRecord {
                        id: id_text(guide.id),
                        axis: if guide.horizontal {
                            "horizontal".to_owned()
                        } else {
                            "vertical".to_owned()
                        },
                        position: guide.position,
                    })
                    .collect(),
            )
        },
        extra: document.extra.clone(),
    })
}

/// Turns an adjustment into its manifest record, keeping any sub-object the macOS build wrote
/// that this build does not model.
fn write_adjustment(adjustment: &Adjustment, raw: Option<&serde_json::Value>) -> serde_json::Value {
    let mut record = match raw {
        Some(serde_json::Value::Object(map)) => serde_json::Value::Object(map.clone()),
        _ => serde_json::json!({}),
    };
    let object = record.as_object_mut().expect("an object was built");
    object.insert(
        "kind".to_owned(),
        serde_json::Value::String(adjustment.kind_name().to_owned()),
    );
    // The format always carries identity Levels and Curves blocks alongside a kind's own
    // settings, so the other kinds keep working if the layer's kind is changed later.
    if !object.contains_key("levels") {
        let ranges: Vec<serde_json::Value> = (0..4).map(|_| identity_level_range()).collect();
        object.insert(
            "levels".to_owned(),
            serde_json::json!({
                "channel": "RGB",
                "ranges": ranges,
            }),
        );
    }
    if !object.contains_key("curves") {
        object.insert(
            "curves".to_owned(),
            serde_json::json!({
                "channel": "RGB",
                "channels": [
                    identity_curve(), identity_curve(), identity_curve(), identity_curve()
                ],
            }),
        );
    }
    match adjustment {
        Adjustment::Invert => {}
        Adjustment::Levels(settings) => {
            let ranges: Vec<serde_json::Value> = settings
                .ranges
                .iter()
                .map(|range| {
                    serde_json::json!({
                        "black": range.black,
                        "gamma": range.gamma,
                        "white": range.white,
                        "outputBlack": range.output_black,
                        "outputWhite": range.output_white,
                    })
                })
                .collect();
            object.insert(
                "levels".to_owned(),
                serde_json::json!({ "channel": "RGB", "ranges": ranges }),
            );
        }
        Adjustment::Curves(settings) => {
            let channels: Vec<serde_json::Value> = settings
                .channels
                .iter()
                .map(|channel| {
                    serde_json::Value::Array(
                        channel
                            .points
                            .iter()
                            .map(|point| serde_json::json!({ "x": point[0], "y": point[1] }))
                            .collect(),
                    )
                })
                .collect();
            object.insert(
                "curves".to_owned(),
                serde_json::json!({ "channel": "RGB", "channels": channels }),
            );
        }
        Adjustment::HueSaturation {
            hue,
            saturation,
            lightness,
            colorize,
        } => {
            object.insert("hue".to_owned(), serde_json::json!(hue));
            object.insert("saturation".to_owned(), serde_json::json!(saturation));
            object.insert("lightness".to_owned(), serde_json::json!(lightness));
            object.insert("colorize".to_owned(), serde_json::json!(colorize));
        }
        Adjustment::Exposure(settings) => {
            object.insert(
                "exposureSettings".to_owned(),
                serde_json::json!({
                    "exposure": settings.exposure,
                    "offset": settings.offset,
                    "gamma": settings.gamma,
                }),
            );
        }
        Adjustment::BlackAndWhite {
            weights,
            tint,
            tint_hue,
            tint_saturation,
        } => {
            object.insert(
                "blackWhiteSettings".to_owned(),
                serde_json::json!({
                    "red": weights[0],
                    "yellow": weights[1],
                    "green": weights[2],
                    "cyan": weights[3],
                    "blue": weights[4],
                    "magenta": weights[5],
                }),
            );
            object.insert("bwTint".to_owned(), serde_json::json!(tint));
            object.insert("bwTintHue".to_owned(), serde_json::json!(tint_hue));
            object.insert(
                "bwTintSaturation".to_owned(),
                serde_json::json!(tint_saturation),
            );
        }
        Adjustment::ColorBalance(settings) => {
            object.insert(
                "colorBalanceSettings".to_owned(),
                serde_json::json!({
                    "shadowCyanRed": settings.shadows[0],
                    "shadowMagentaGreen": settings.shadows[1],
                    "shadowYellowBlue": settings.shadows[2],
                    "midCyanRed": settings.midtones[0],
                    "midMagentaGreen": settings.midtones[1],
                    "midYellowBlue": settings.midtones[2],
                    "highlightCyanRed": settings.highlights[0],
                    "highlightMagentaGreen": settings.highlights[1],
                    "highlightYellowBlue": settings.highlights[2],
                    "preserveLuminosity": settings.preserve_luminosity,
                }),
            );
        }
        Adjustment::GradientMap(settings) => {
            object.insert(
                "gradientMapSettings".to_owned(),
                serde_json::json!({
                    "shadows": settings.shadows,
                    "highlights": settings.highlights,
                    "reversed": settings.reversed,
                }),
            );
        }
        Adjustment::GaussianBlur { radius } => {
            object.insert("blurRadius".to_owned(), serde_json::json!(radius));
        }
        Adjustment::MotionBlur { angle, distance } => {
            object.insert("motionAngle".to_owned(), serde_json::json!(angle));
            object.insert("motionDistance".to_owned(), serde_json::json!(distance));
        }
        Adjustment::AddNoise {
            amount,
            gaussian,
            monochromatic,
            seed,
        } => {
            object.insert("noiseAmount".to_owned(), serde_json::json!(amount));
            object.insert("noiseGaussian".to_owned(), serde_json::json!(gaussian));
            object.insert(
                "noiseMonochromatic".to_owned(),
                serde_json::json!(monochromatic),
            );
            object.insert("noiseSeed".to_owned(), serde_json::json!(seed));
        }
        Adjustment::Grain(settings) => {
            object.insert(
                "grainSettings".to_owned(),
                serde_json::json!({
                    "amount": settings.amount,
                    "size": settings.size,
                    "roughness": settings.roughness,
                    "seed": settings.seed,
                }),
            );
        }
    }
    record
}

fn identity_level_range() -> serde_json::Value {
    serde_json::json!({
        "black": 0.0,
        "gamma": 1.0,
        "white": 255.0,
        "outputBlack": 0.0,
        "outputWhite": 255.0,
    })
}

fn identity_curve() -> serde_json::Value {
    serde_json::json!([{ "x": 0.0, "y": 0.0 }, { "x": 255.0, "y": 255.0 }])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::adjustment::{LevelRange, LevelsSettings};
    use crate::core::pixel::GrayImage;
    use std::sync::Arc;

    fn sample_document() -> Document {
        let mut document = Document::new(16, 12);
        document.resolution = 144.0;
        let mut bottom = PremulImage::filled(16, 12, [200, 30, 30, 255]);
        bottom.set_pixel(0, 0, [0, 0, 0, 0]);
        document.add_layer("Background", Arc::new(bottom), None);
        let mut top = PremulImage::new(6, 6);
        for y in 0..6 {
            for x in 0..6 {
                top.set_pixel(x, y, [0, 0, 255, 255]);
            }
        }
        let top_id = document.add_layer("Patch", Arc::new(top), None);
        let layer = document.layer_mut(top_id).expect("layer");
        layer.transform.origin = [4.0, 3.0];
        layer.opacity = 0.75;
        layer.blend = BlendMode::Multiply;
        layer.mask = Some(Mask {
            image: Arc::new(GrayImage::new(6, 6, 200)),
            linked: true,
            placement: None,
        });
        document
    }

    #[test]
    fn round_trip_keeps_the_document() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("Round-Trip.comp");
        let mut document = sample_document();
        save(&mut document, &path).expect("save");
        assert!(is_project(&path));
        let loaded = load(&path).expect("load");
        assert_eq!(loaded.width, document.width);
        assert_eq!(loaded.height, document.height);
        assert_eq!(loaded.resolution, document.resolution);
        assert_eq!(loaded.layers.len(), document.layers.len());
        for (original, restored) in document.layers.iter().zip(loaded.layers.iter()) {
            assert_eq!(original.id, restored.id);
            assert_eq!(original.name, restored.name);
            assert_eq!(original.opacity, restored.opacity);
            assert_eq!(original.blend, restored.blend);
            assert_eq!(original.transform.origin, restored.transform.origin);
            assert_eq!(original.mask.is_some(), restored.mask.is_some());
        }
        // The image bytes survive exactly, premultiplied as they are stored.
        let original = document.layers[0].image.clone().expect("pixels");
        let restored = loaded.layers[0].image.clone().expect("pixels");
        assert_eq!(original.as_slice(), restored.as_slice());
    }

    #[test]
    fn a_saved_project_loads_like_the_macos_layout() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("Layout.comp");
        let mut document = sample_document();
        save(&mut document, &path).expect("save");
        assert!(path.join("manifest.json").is_file());
        assert!(path.join("images").is_dir());
        assert!(path.join("QuickLook/Preview.jpg").is_file());
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path.join("manifest.json")).expect("read"))
                .expect("json");
        assert_eq!(manifest["format"], "com.compositor.project");
        assert_eq!(manifest["version"], VERSION);
        assert_eq!(manifest["colorSpace"], "sRGB");
        let first = &manifest["layers"][0];
        // imageFile is a bare file name, as the macOS store resolves it inside images/.
        let image_file = first["imageFile"].as_str().expect("image file");
        assert!(!image_file.contains('/'), "got {image_file}");
        assert!(path.join("images").join(image_file).is_file());
        assert_eq!(first["name"], "Background");
        assert_eq!(first["blendMode"], "Normal");
        // Layers run bottom to top, so the patch is second.
        let patch = &manifest["layers"][1];
        assert_eq!(patch["name"], "Patch");
        assert_eq!(patch["blendMode"], "Multiply");
        assert_eq!(patch["opacity"], 0.75);
        assert_eq!(patch["isVisible"], true);
        assert_eq!(manifest["resolution"], 144.0);
    }

    #[test]
    fn groups_keep_their_shape_and_order() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("Groups.comp");
        let mut document = sample_document();
        let top = document.layers[1].id;
        let group = document.group_layers(&[top], "Folder").expect("group");
        save(&mut document, &path).expect("save");
        let loaded = load(&path).expect("load");
        assert_eq!(loaded.layer(top).expect("child").parent, Some(group));
        assert!(loaded.is_group(group));
        assert_eq!(
            loaded.save_order(),
            loaded
                .children(None)
                .into_iter()
                .flat_map(|id| {
                    loaded
                        .subtree(id)
                        .into_iter()
                        .map(|layer| layer.id)
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn unknown_layer_fields_are_preserved() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("Effects.comp");
        let mut document = sample_document();
        document.layers[0].extra.insert(
            "effects".to_owned(),
            serde_json::json!({ "stroke": { "size": 3.0 } }),
        );
        document
            .extra
            .insert("futureField".to_owned(), serde_json::json!([1, 2, 3]));
        save(&mut document, &path).expect("save");
        let loaded = load(&path).expect("load");
        assert_eq!(
            loaded.layers[0].extra.get("effects"),
            Some(&serde_json::json!({ "stroke": { "size": 3.0 } }))
        );
        assert_eq!(
            loaded.extra.get("futureField"),
            Some(&serde_json::json!([1, 2, 3]))
        );
    }

    #[test]
    fn adjustment_layers_round_trip_their_settings() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("Levels.comp");
        let mut document = sample_document();
        let mut settings = LevelsSettings::default();
        settings.ranges[0].gamma = 1.4;
        settings.ranges[1] = LevelRange {
            black: 10.0,
            white: 240.0,
            ..LevelRange::default()
        };
        crate::core::ops::add_adjustment_layer(&mut document, Adjustment::Levels(settings));
        save(&mut document, &path).expect("save");
        let loaded = load(&path).expect("load");
        let layer = loaded.layers.last().expect("adjustment layer");
        assert_eq!(layer.kind, LayerKind::Adjustment);
        match layer.adjustment.as_ref().expect("settings") {
            Adjustment::Levels(restored) => {
                assert!((restored.ranges[0].gamma - 1.4).abs() < 1e-9);
                assert!((restored.ranges[1].black - 10.0).abs() < 1e-9);
                assert!((restored.ranges[1].white - 240.0).abs() < 1e-9);
            }
            other => panic!("wrong kind: {other:?}"),
        }
    }

    #[test]
    fn a_newer_version_is_refused() {
        let manifest = serde_json::json!({
            "format": FORMAT,
            "version": VERSION + 1,
            "colorSpace": "sRGB",
            "documentID": "0C5E7A91-3B2D-4F6A-8E1C-9D0B7A6F5E4D",
            "width": 4,
            "height": 4,
            "layers": [],
        });
        let error = load_from_parts(manifest.to_string().as_bytes(), |_| None).unwrap_err();
        assert!(
            matches!(error, ProjectError::UnsupportedVersion(_)),
            "{error}"
        );
    }

    #[test]
    fn a_missing_image_is_reported_by_name() {
        let manifest = serde_json::json!({
            "format": FORMAT,
            "version": VERSION,
            "colorSpace": "sRGB",
            "documentID": "0C5E7A91-3B2D-4F6A-8E1C-9D0B7A6F5E4D",
            "width": 4,
            "height": 4,
            "layers": [{
                "id": "6F1D3C2A-0B7E-4E8A-9C4D-2A1B3C4D5E6F",
                "name": "Missing",
                "isVisible": true,
                "imageFile": "6F1D3C2A-0B7E-4E8A-9C4D-2A1B3C4D5E6F.png",
                "transform": { "origin": [0, 0], "size": [4, 4] }
            }],
        });
        let error = load_from_parts(manifest.to_string().as_bytes(), |_| None).unwrap_err();
        assert!(matches!(error, ProjectError::MissingAsset(_)), "{error}");
        assert!(error.to_string().contains(".png"), "{error}");
    }

    #[test]
    fn a_parent_that_is_not_a_folder_is_refused() {
        let manifest = serde_json::json!({
            "format": FORMAT,
            "version": VERSION,
            "colorSpace": "sRGB",
            "documentID": "0C5E7A91-3B2D-4F6A-8E1C-9D0B7A6F5E4D",
            "width": 4,
            "height": 4,
            "layers": [{
                "id": "6F1D3C2A-0B7E-4E8A-9C4D-2A1B3C4D5E6F",
                "name": "Child",
                "isVisible": true,
                "parentID": "11111111-1111-4111-8111-111111111111",
                "transform": { "origin": [0, 0], "size": [4, 4] }
            }],
        });
        let error = load_from_parts(manifest.to_string().as_bytes(), |_| None).unwrap_err();
        assert!(matches!(error, ProjectError::Invalid(_)), "{error}");
    }

    #[test]
    fn saving_over_an_existing_project_replaces_it() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("Replace.comp");
        let mut document = sample_document();
        save(&mut document, &path).expect("first save");
        document.layers[0].name = "Renamed".to_owned();
        save(&mut document, &path).expect("second save");
        let loaded = load(&path).expect("load");
        assert_eq!(loaded.layers[0].name, "Renamed");
        // No staging or backup folders are left behind.
        let leftovers: Vec<_> = std::fs::read_dir(directory.path())
            .expect("read dir")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .filter(|name| name.contains("saving") || name.contains("previous"))
            .collect();
        assert!(leftovers.is_empty(), "left behind: {leftovers:?}");
    }

    #[test]
    fn unsafe_asset_names_are_read_as_missing() {
        assert!(unsafe_name("../secrets.png"));
        assert!(unsafe_name("images/../../etc/passwd"));
        assert!(unsafe_name(".hidden.png"));
        assert!(!unsafe_name("6F1D3C2A-0B7E-4E8A-9C4D-2A1B3C4D5E6F.png"));
    }
}

//! Editing operations.
//!
//! Every function here mutates a [`Document`] and leaves the undo history to the caller, so a
//! command and its undo step always sit together in one place (see `app::state::AppState`).
//! Pixels are premultiplied throughout, and painting is limited by the active selection and by
//! the layer's mask, exactly like the original.

use std::sync::Arc;

use uuid::Uuid;

use crate::core::adjustment::{Adjustment, Filter, apply_adjustment};
use crate::core::document::{Document, DocumentError, Layer, LayerKind, Mask, check_size};
use crate::core::geometry::RectI;
use crate::core::pixel::{GrayImage, PremulImage, Rgba, over};
use crate::core::selection::Selection;
use crate::ffi;

/// What went wrong while editing.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum OpError {
    #[error("{0}")]
    Document(#[from] DocumentError),
    #[error("请先选择图层")]
    NoActiveLayer,
    #[error("该图层没有可编辑的像素")]
    NoPixels,
    #[error("当前没有选区")]
    NoSelection,
    #[error("该操作要求图层像素与画布对齐（图层移动或旋转后不支持，可先拼合）")]
    SelectionDoesNotLineUp,
    #[error("内核未能完成：{0}")]
    Kernel(&'static str),
    #[error("{0}")]
    Filter(String),
}

/// Where a brush stroke goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaintTarget {
    Pixels,
    Mask,
}

/// The brush's settings, as the tool header shows them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushSettings {
    /// Diameter in document pixels.
    pub size: f64,
    /// 0 is a soft brush, 1 a hard edge.
    pub hardness: f64,
    /// 0–1.
    pub opacity: f64,
    /// Straight sRGB.
    pub color: Rgba,
    /// Erase mode paints transparency instead of color.
    pub erase: bool,
    pub target: PaintTarget,
}

impl Default for BrushSettings {
    fn default() -> Self {
        Self {
            size: 32.0,
            hardness: 0.8,
            opacity: 1.0,
            color: [0, 0, 0, 255],
            erase: false,
            target: PaintTarget::Pixels,
        }
    }
}

/// The magic wand's settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WandSettings {
    /// 0–255: how far a pixel's channels may differ from the sampled color.
    pub tolerance: u8,
    /// False selects every matching pixel in the layer, not just the connected region.
    pub contiguous: bool,
    /// Radius of the averaged sample under the click.
    pub radius: u32,
}

impl Default for WandSettings {
    fn default() -> Self {
        Self {
            tolerance: 32,
            contiguous: true,
            radius: 0,
        }
    }
}

/// The healing brush's modes, in the order `HealPixels.h` numbers them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HealMode {
    ContentAware,
    CreateTexture,
    ProximityMatch,
}

impl HealMode {
    #[must_use]
    pub fn index(self) -> i32 {
        match self {
            Self::ContentAware => 0,
            Self::CreateTexture => 1,
            Self::ProximityMatch => 2,
        }
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::ContentAware => "Content-Aware",
            Self::CreateTexture => "Create Texture",
            Self::ProximityMatch => "Proximity Match",
        }
    }
}

fn premultiplied(color: Rgba) -> Rgba {
    let alpha = u32::from(color[3]);
    [
        ((u32::from(color[0]) * alpha + 127) / 255) as u8,
        ((u32::from(color[1]) * alpha + 127) / 255) as u8,
        ((u32::from(color[2]) * alpha + 127) / 255) as u8,
        color[3],
    ]
}

/// Turns a selection into coverage in a layer's own pixel space.
///
/// Returns `None` when there is no selection, or when the layer's transform moves or turns its
/// pixels — the mask would not line up, and the caller then edits the whole layer instead.
/// A layer mask, when there is one, is folded in.
#[must_use]
pub fn coverage_for_layer(
    document: &Document,
    layer_id: Uuid,
    selection: Option<&Selection>,
) -> Option<GrayImage> {
    let layer = document.layer(layer_id)?;
    let image = layer.image.as_ref()?;
    let mut coverage = match selection {
        Some(selection) => {
            if !layer.is_identity()
                || selection.width() != image.width()
                || selection.height() != image.height()
            {
                return None;
            }
            selection.mask().clone()
        }
        None => GrayImage::new(image.width(), image.height(), 255),
    };
    if layer.mask_enabled
        && let Some(mask) = &layer.mask
    {
        for y in 0..coverage.height() {
            for x in 0..coverage.width() {
                let source_x = (x as f64 * f64::from(mask.image.width())
                    / f64::from(coverage.width()))
                .floor();
                let source_y = (y as f64 * f64::from(mask.image.height())
                    / f64::from(coverage.height()))
                .floor();
                let value = mask.image.pixel(source_x as u32, source_y as u32);
                let combined = (u32::from(coverage.pixel(x, y)) * u32::from(value) + 127) / 255;
                coverage.set_pixel(x, y, combined as u8);
            }
        }
    }
    Some(coverage)
}

/// Paints one brush stamp at a document point.
///
/// Returns `false` when the stamp misses the layer, the selection or the mask.
pub fn stamp_brush(
    document: &mut Document,
    layer_id: Uuid,
    point: [f64; 2],
    settings: &BrushSettings,
    selection: Option<&Selection>,
) -> bool {
    let Some(layer) = document.layer(layer_id).cloned() else {
        return false;
    };
    let image_size = layer.image_size();
    if image_size[0] <= 0.0 || image_size[1] <= 0.0 {
        return false;
    }
    let Some(local) = layer.transform.document_to_image(point, image_size) else {
        return false;
    };
    let scale = layer.transform.scale_factors(image_size);
    let radius = (settings.size / 2.0 * (scale[0] + scale[1]) / 2.0).max(0.5);
    let coverage = coverage_for_layer(document, layer_id, selection);

    if settings.target == PaintTarget::Mask && layer.mask.is_some() {
        let Some(mask) = layer.mask.as_ref() else {
            return false;
        };
        let mut gray = (*mask.image).clone();
        let mut buffer = PremulImage::new(gray.width(), gray.height());
        for y in 0..gray.height() {
            for x in 0..gray.width() {
                let value = gray.pixel(x, y);
                buffer.set_pixel(x, y, [value, value, value, 255]);
            }
        }
        let mask_coverage = coverage.as_ref().map(|coverage| {
            let mut scaled = GrayImage::new(gray.width(), gray.height(), 255);
            for y in 0..gray.height() {
                for x in 0..gray.width() {
                    let source_x =
                        (x as f64 * f64::from(coverage.width()) / f64::from(gray.width())).floor();
                    let source_y = (y as f64 * f64::from(coverage.height())
                        / f64::from(gray.height()))
                    .floor();
                    scaled.set_pixel(x, y, coverage.pixel(source_x as u32, source_y as u32));
                }
            }
            scaled
        });
        let painted = stamp(&mut buffer, local, radius, settings, mask_coverage.as_ref());
        for y in 0..gray.height() {
            for x in 0..gray.width() {
                let pixel = buffer.pixel(x, y).unwrap_or([0; 4]);
                gray.set_pixel(x, y, pixel[0]);
            }
        }
        if let Some(layer) = document.layer_mut(layer_id) {
            layer.mask = Some(Mask {
                image: Arc::new(gray),
                linked: layer.mask.as_ref().is_some_and(|mask| mask.linked),
                placement: layer.mask.as_ref().and_then(|mask| mask.placement),
            });
        }
        document.dirty = true;
        return painted;
    }

    let Some(image) = layer.image.clone() else {
        return false;
    };
    let mut updated = (*image).clone();
    let painted = stamp(&mut updated, local, radius, settings, coverage.as_ref());
    if painted {
        if let Some(layer) = document.layer_mut(layer_id) {
            layer.image = Some(Arc::new(updated));
        }
        document.dirty = true;
    }
    painted
}

/// Paints a run of stamps between two document points, which is how a drag becomes a stroke.
pub fn stroke_segment(
    document: &mut Document,
    layer_id: Uuid,
    from: [f64; 2],
    to: [f64; 2],
    settings: &BrushSettings,
    selection: Option<&Selection>,
) {
    let distance = ((to[0] - from[0]).powi(2) + (to[1] - from[1]).powi(2)).sqrt();
    let spacing = (settings.size * 0.15).max(0.75);
    let steps = (distance / spacing).ceil().max(1.0) as u32;
    for step in 0..=steps {
        let t = f64::from(step) / f64::from(steps);
        let point = [
            from[0] + (to[0] - from[0]) * t,
            from[1] + (to[1] - from[1]) * t,
        ];
        stamp_brush(document, layer_id, point, settings, selection);
    }
}

/// Draws one soft round stamp onto a buffer. `coverage` is in the buffer's pixel space.
fn stamp(
    image: &mut PremulImage,
    local: [f64; 2],
    radius: f64,
    settings: &BrushSettings,
    coverage: Option<&GrayImage>,
) -> bool {
    if image.is_empty() {
        return false;
    }
    let hardness = settings.hardness.clamp(0.0, 1.0);
    let opacity = settings.opacity.clamp(0.0, 1.0) as f32;
    if opacity <= 0.0 {
        return false;
    }
    let color = premultiplied(settings.color);
    let x0 = ((local[0] - radius).floor() as i64).max(0);
    let y0 = ((local[1] - radius).floor() as i64).max(0);
    let x1 = ((local[0] + radius).ceil() as i64).min(i64::from(image.width()));
    let y1 = ((local[1] + radius).ceil() as i64).min(i64::from(image.height()));
    let inner = radius * hardness;
    let mut painted = false;
    for y in y0..y1 {
        for x in x0..x1 {
            let dx = x as f64 + 0.5 - local[0];
            let dy = y as f64 + 0.5 - local[1];
            let distance = (dx * dx + dy * dy).sqrt();
            if distance > radius {
                continue;
            }
            let falloff = if distance <= inner {
                1.0
            } else {
                1.0 - (distance - inner) / (radius - inner).max(1e-6)
            };
            let mut amount = opacity * falloff.clamp(0.0, 1.0) as f32;
            if let Some(coverage) = coverage {
                amount *= f32::from(coverage.pixel(x as u32, y as u32)) / 255.0;
            }
            if amount <= 0.0 {
                continue;
            }
            let base = image.pixel(x as u32, y as u32).unwrap_or([0; 4]);
            let out = if settings.erase {
                let keep = 1.0 - amount;
                [
                    (f32::from(base[0]) * keep).round() as u8,
                    (f32::from(base[1]) * keep).round() as u8,
                    (f32::from(base[2]) * keep).round() as u8,
                    (f32::from(base[3]) * keep).round() as u8,
                ]
            } else {
                over(base, color, amount)
            };
            image.set_pixel(x as u32, y as u32, out);
            painted = true;
        }
    }
    painted
}

/// Fills the selection (or the whole layer) with one color.
///
/// # Errors
///
/// Fails when the layer is missing or has no pixels.
pub fn fill(
    document: &mut Document,
    layer_id: Uuid,
    color: Rgba,
    selection: Option<&Selection>,
) -> Result<(), OpError> {
    let coverage = coverage_for_layer(document, layer_id, selection);
    let Some(layer) = document.layer_mut(layer_id) else {
        return Err(OpError::NoActiveLayer);
    };
    let Some(image) = layer.image.clone() else {
        return Err(OpError::NoPixels);
    };
    let mut updated = (*image).clone();
    let source = premultiplied(color);
    let width = updated.width();
    let height = updated.height();
    for y in 0..height {
        for x in 0..width {
            let amount = coverage
                .as_ref()
                .map_or(1.0, |mask| f32::from(mask.pixel(x, y)) / 255.0);
            if amount <= 0.0 {
                continue;
            }
            let base = updated.pixel(x, y).unwrap_or([0; 4]);
            updated.set_pixel(x, y, over(base, source, amount));
        }
    }
    if let Some(layer) = document.layer_mut(layer_id) {
        layer.image = Some(Arc::new(updated));
    }
    document.dirty = true;
    Ok(())
}

/// The magic wand: selects the pixels around a point that match it.
///
/// The kernel runs on the layer's own pixels; the result is mapped back onto the canvas, so the
/// selection is always in document coordinates.
///
/// # Errors
///
/// Fails when the layer has no pixels, or when the point misses them.
pub fn magic_wand_selection(
    document: &Document,
    layer_id: Uuid,
    point: [f64; 2],
    settings: &WandSettings,
) -> Result<Selection, OpError> {
    let layer = document.layer(layer_id).ok_or(OpError::NoActiveLayer)?;
    let image = layer.image.as_ref().ok_or(OpError::NoPixels)?;
    let image_size = layer.image_size();
    let local = layer
        .transform
        .document_to_image(point, image_size)
        .ok_or(OpError::NoPixels)?;
    let mut mask = GrayImage::new(image.width(), image.height(), 0);
    let selected = ffi::magic_wand(
        image,
        (local[0] as u32, local[1] as u32),
        settings.radius,
        settings.tolerance,
        settings.contiguous,
        &mut mask,
    );
    if selected <= 0 {
        return Ok(Selection::from_mask(GrayImage::new(
            document.width,
            document.height,
            0,
        )));
    }
    if layer.is_identity() && image.width() == document.width && image.height() == document.height {
        return Ok(Selection::from_mask(mask));
    }
    Ok(Selection::from_mask(map_mask_to_document(
        document, layer, &mask,
    )))
}

/// Places a layer-space mask onto the canvas, through the layer's transform.
fn map_mask_to_document(document: &Document, layer: &Layer, mask: &GrayImage) -> GrayImage {
    let mut out = GrayImage::new(document.width, document.height, 0);
    let image_size = [f64::from(mask.width()), f64::from(mask.height())];
    let bounds = layer
        .transform
        .document_bounds()
        .intersect(&document.canvas_rect());
    for y in bounds.y0..bounds.y1 {
        for x in bounds.x0..bounds.x1 {
            let point = [x as f64 + 0.5, y as f64 + 0.5];
            if let Some(local) = layer.transform.document_to_image(point, image_size) {
                let source_x =
                    (local[0].floor() as i64).clamp(0, i64::from(mask.width()) - 1) as u32;
                let source_y =
                    (local[1].floor() as i64).clamp(0, i64::from(mask.height()) - 1) as u32;
                out.set_pixel(x as u32, y as u32, mask.pixel(source_x, source_y));
            }
        }
    }
    out
}

/// Select ▸ Load Layer Alpha: makes a selection out of a layer's own coverage.
///
/// # Errors
///
/// Fails when the layer has no pixels.
pub fn load_layer_alpha(document: &Document, layer_id: Uuid) -> Result<Selection, OpError> {
    let layer = document.layer(layer_id).ok_or(OpError::NoActiveLayer)?;
    let image = layer.image.as_ref().ok_or(OpError::NoPixels)?;
    let mut gray = GrayImage::new(image.width(), image.height(), 0);
    ffi::extract_alpha(image, &mut gray);
    if layer.is_identity() && image.width() == document.width && image.height() == document.height {
        return Ok(Selection::from_mask(gray));
    }
    Ok(Selection::from_mask(map_mask_to_document(
        document, layer, &gray,
    )))
}

/// Erases the selected pixels of a layer: the Delete key with a selection up.
///
/// # Errors
///
/// Fails when the layer has no pixels, or the selection does not line up with them.
pub fn delete_selected_pixels(
    document: &mut Document,
    layer_id: Uuid,
    selection: &Selection,
) -> Result<(), OpError> {
    let coverage = coverage_for_layer(document, layer_id, Some(selection))
        .ok_or(OpError::SelectionDoesNotLineUp)?;
    let Some(layer) = document.layer_mut(layer_id) else {
        return Err(OpError::NoActiveLayer);
    };
    let Some(image) = layer.image.clone() else {
        return Err(OpError::NoPixels);
    };
    let mut updated = (*image).clone();
    // Keeping the pixels the selection leaves out is the same as multiplying by its inverse.
    let mut outside = coverage;
    for value in outside.as_mut_slice() {
        *value = 255 - *value;
    }
    updated.multiply_coverage(outside.as_slice());
    if let Some(layer) = document.layer_mut(layer_id) {
        layer.image = Some(Arc::new(updated));
    }
    document.dirty = true;
    Ok(())
}

/// Image ▸ Trim: crops the canvas to the picture the composite actually covers.
///
/// # Errors
///
/// Fails when the composite is empty, or already fills the canvas.
pub fn trim(document: &mut Document) -> Result<(), OpError> {
    let flattened = crate::core::composite::composite_document(document);
    let bounds = ffi::alpha_bounds(&flattened).ok_or(OpError::Kernel("没有可见像素可供修整"))?;
    if bounds.x0 <= 0
        && bounds.y0 <= 0
        && bounds.x1 >= i64::from(document.width)
        && bounds.y1 >= i64::from(document.height)
    {
        return Err(OpError::Kernel("图像已经填满画布"));
    }
    crop(document, bounds)
}

/// The healing brush: covers a round spot with texture copied from nearby pixels.
///
/// # Errors
///
/// Fails when the layer has no pixels, or when its transform moves its pixels.
pub fn heal(
    document: &mut Document,
    layer_id: Uuid,
    point: [f64; 2],
    radius: f64,
    mode: HealMode,
    seed: u32,
) -> Result<(), OpError> {
    let Some(layer) = document.layer(layer_id).cloned() else {
        return Err(OpError::NoActiveLayer);
    };
    let image_size = layer.image_size();
    let Some(local) = layer.transform.document_to_image(point, image_size) else {
        return Err(OpError::NoPixels);
    };
    if !layer.is_identity() {
        return Err(OpError::SelectionDoesNotLineUp);
    }
    let Some(image) = layer.image.clone() else {
        return Err(OpError::NoPixels);
    };
    let width = image.width();
    let height = image.height();
    let mut coverage = GrayImage::new(width, height, 0);
    let radius = radius.max(1.0);
    for y in 0..height {
        for x in 0..width {
            let dx = x as f64 + 0.5 - local[0];
            let dy = y as f64 + 0.5 - local[1];
            if (dx * dx + dy * dy).sqrt() <= radius {
                coverage.set_pixel(x, y, 255);
            }
        }
    }
    let mut updated = (*image).clone();
    if !ffi::heal(&mut updated, &coverage, 1.0, mode.index(), seed) {
        return Err(OpError::Kernel("污点修复无法读取周边像素"));
    }
    if let Some(layer) = document.layer_mut(layer_id) {
        layer.image = Some(Arc::new(updated));
    }
    document.dirty = true;
    Ok(())
}

/// Crops the canvas, keeping every layer's pixels where they were.
///
/// # Errors
///
/// Fails when the rectangle does not overlap the canvas.
pub fn crop(document: &mut Document, rect: RectI) -> Result<(), OpError> {
    let rect = rect.intersect(&document.canvas_rect());
    if rect.is_empty() {
        return Err(OpError::Document(DocumentError::OutsideCanvas));
    }
    let (dx, dy) = (-rect.x0, -rect.y0);
    document.width = rect.width() as u32;
    document.height = rect.height() as u32;
    for layer in &mut document.layers {
        layer.transform.origin[0] += dx as f64;
        layer.transform.origin[1] += dy as f64;
    }
    for guide in &mut document.guides {
        if guide.horizontal {
            guide.position += dy as f64;
        } else {
            guide.position += dx as f64;
        }
    }
    document.dirty = true;
    Ok(())
}

/// Mirrors the whole canvas: every layer keeps its place in the mirrored picture.
pub fn flip_canvas(document: &mut Document, horizontal: bool) {
    let width = f64::from(document.width);
    let height = f64::from(document.height);
    for layer in &mut document.layers {
        let center = layer.transform.center();
        if horizontal {
            let mirrored = width - center[0];
            layer.transform.origin[0] = mirrored - layer.transform.size[0] / 2.0;
            layer.transform.flip_x = !layer.transform.flip_x;
        } else {
            let mirrored = height - center[1];
            layer.transform.origin[1] = mirrored - layer.transform.size[1] / 2.0;
            layer.transform.flip_y = !layer.transform.flip_y;
        }
        layer.transform.rotation = -layer.transform.rotation;
    }
    document.dirty = true;
}

/// Mirrors one layer about its own center.
///
/// # Errors
///
/// Fails when the layer does not exist.
pub fn flip_layer(
    document: &mut Document,
    layer_id: Uuid,
    horizontal: bool,
) -> Result<(), OpError> {
    let Some(layer) = document.layer_mut(layer_id) else {
        return Err(OpError::NoActiveLayer);
    };
    if horizontal {
        layer.transform.flip_x = !layer.transform.flip_x;
    } else {
        layer.transform.flip_y = !layer.transform.flip_y;
    }
    layer.transform.rotation = -layer.transform.rotation;
    document.dirty = true;
    Ok(())
}

/// Turns the canvas a quarter turn, pixels and all.
///
/// # Errors
///
/// Fails when the turned canvas would be out of range.
pub fn rotate_canvas_90(document: &mut Document, clockwise: bool) -> Result<(), OpError> {
    check_size(document.height, document.width)?;
    let width = f64::from(document.width);
    let height = f64::from(document.height);
    for layer in &mut document.layers {
        if let Some(image) = &layer.image {
            let turned = if clockwise {
                image.rotate_90_cw()
            } else {
                image.rotate_90_cw().rotate_90_cw().rotate_90_cw()
            };
            layer.image = Some(Arc::new(turned));
        }
        if let Some(mask) = &layer.mask {
            layer.mask = Some(Mask {
                image: Arc::new(rotate_gray_90(&mask.image, clockwise)),
                linked: mask.linked,
                placement: mask.placement,
            });
        }
        let center = layer.transform.center();
        let (new_center, delta) = if clockwise {
            ([height - center[1], center[0]], 90.0)
        } else {
            ([center[1], width - center[0]], -90.0)
        };
        let new_size = [layer.transform.size[1], layer.transform.size[0]];
        layer.transform.origin = [
            new_center[0] - new_size[0] / 2.0,
            new_center[1] - new_size[1] / 2.0,
        ];
        layer.transform.size = new_size;
        layer.transform.rotation += delta;
    }
    // The canvas swaps its sides: the old height becomes the width.
    document.width = height as u32;
    document.height = width as u32;
    document.dirty = true;
    Ok(())
}

fn rotate_gray_90(image: &GrayImage, clockwise: bool) -> GrayImage {
    let mut out = GrayImage::new(image.height(), image.width(), 0);
    for y in 0..image.height() {
        for x in 0..image.width() {
            let value = image.pixel(x, y);
            if clockwise {
                out.set_pixel(image.height() - 1 - y, x, value);
            } else {
                out.set_pixel(y, image.width() - 1 - x, value);
            }
        }
    }
    out
}

/// Runs an adjustment over one layer's pixels, limited by the selection and the layer's mask.
///
/// # Errors
///
/// Fails when the layer is missing, has no pixels, or (with a selection) does not line up.
pub fn apply_adjustment_to_layer(
    document: &mut Document,
    layer_id: Uuid,
    adjustment: &Adjustment,
    selection: Option<&Selection>,
) -> Result<(), OpError> {
    let coverage = match selection {
        None => None,
        Some(selection) => Some(
            coverage_for_layer(document, layer_id, Some(selection))
                .ok_or(OpError::SelectionDoesNotLineUp)?,
        ),
    };
    let Some(layer) = document.layer_mut(layer_id) else {
        return Err(OpError::NoActiveLayer);
    };
    let Some(image) = layer.image.clone() else {
        return Err(OpError::NoPixels);
    };
    let mut updated = (*image).clone();
    apply_adjustment(adjustment, &mut updated, coverage.as_ref());
    if let Some(layer) = document.layer_mut(layer_id) {
        layer.image = Some(Arc::new(updated));
    }
    document.dirty = true;
    Ok(())
}

/// Runs a destructive filter over one layer's pixels, limited by the selection.
///
/// # Errors
///
/// Fails when the layer has no pixels, the selection does not line up, or the filter needs a
/// kernel this build lacks.
pub fn apply_filter_to_layer(
    document: &mut Document,
    layer_id: Uuid,
    filter: &Filter,
    selection: Option<&Selection>,
) -> Result<(), OpError> {
    let coverage = match selection {
        None => None,
        Some(selection) => Some(
            coverage_for_layer(document, layer_id, Some(selection))
                .ok_or(OpError::SelectionDoesNotLineUp)?,
        ),
    };
    let Some(layer) = document.layer_mut(layer_id) else {
        return Err(OpError::NoActiveLayer);
    };
    let Some(image) = layer.image.clone() else {
        return Err(OpError::NoPixels);
    };
    let mut updated = (*image).clone();
    match &coverage {
        None => filter.apply(&mut updated).map_err(OpError::Filter)?,
        Some(mask) => {
            let mut processed = updated.clone();
            filter.apply(&mut processed).map_err(OpError::Filter)?;
            updated.lerp_with_coverage(&processed, mask.as_slice());
        }
    }
    if let Some(layer) = document.layer_mut(layer_id) {
        layer.image = Some(Arc::new(updated));
    }
    document.dirty = true;
    Ok(())
}

/// Adds an adjustment layer above the active one, covering the canvas.
pub fn add_adjustment_layer(document: &mut Document, adjustment: Adjustment) -> Uuid {
    let mut layer = Layer::new_pixel(adjustment.display_label(), Arc::new(PremulImage::new(1, 1)));
    layer.kind = LayerKind::Adjustment;
    layer.image = None;
    layer.adjustment = Some(adjustment);
    layer.transform = crate::core::geometry::Transform::full(document.width, document.height);
    let position = document
        .active_layer
        .and_then(|id| document.index_of(id))
        .map_or(document.layers.len(), |index| index + 1);
    document.insert_layer(layer, position)
}

/// Replaces an adjustment layer's settings.
///
/// # Errors
///
/// Fails when the layer is missing or is not an adjustment layer.
pub fn update_adjustment_layer(
    document: &mut Document,
    layer_id: Uuid,
    adjustment: Adjustment,
) -> Result<(), OpError> {
    let Some(layer) = document.layer_mut(layer_id) else {
        return Err(OpError::NoActiveLayer);
    };
    if layer.kind != LayerKind::Adjustment {
        return Err(OpError::NoPixels);
    }
    layer.name = adjustment.display_label().to_owned();
    layer.adjustment = Some(adjustment);
    document.dirty = true;
    Ok(())
}

/// Gives a layer a mask. `reveal` starts it white (everything visible), otherwise black.
///
/// # Errors
///
/// Fails when the layer does not exist.
pub fn add_mask(document: &mut Document, layer_id: Uuid, reveal: bool) -> Result<(), OpError> {
    let canvas = (document.width, document.height);
    let Some(layer) = document.layer_mut(layer_id) else {
        return Err(OpError::NoActiveLayer);
    };
    let size = layer.image_size();
    let (width, height) = if size[0] >= 1.0 && size[1] >= 1.0 {
        (size[0] as u32, size[1] as u32)
    } else {
        canvas
    };
    layer.mask = Some(Mask {
        image: Arc::new(GrayImage::new(width, height, if reveal { 255 } else { 0 })),
        linked: true,
        placement: None,
    });
    layer.mask_enabled = true;
    document.dirty = true;
    Ok(())
}

/// Removes a layer's mask.
///
/// # Errors
///
/// Fails when the layer does not exist.
pub fn remove_mask(document: &mut Document, layer_id: Uuid) -> Result<(), OpError> {
    let Some(layer) = document.layer_mut(layer_id) else {
        return Err(OpError::NoActiveLayer);
    };
    layer.mask = None;
    document.dirty = true;
    Ok(())
}

/// Turns a mask inside out.
///
/// # Errors
///
/// Fails when the layer has no mask.
pub fn invert_mask(document: &mut Document, layer_id: Uuid) -> Result<(), OpError> {
    let Some(layer) = document.layer_mut(layer_id) else {
        return Err(OpError::NoActiveLayer);
    };
    let Some(mask) = &layer.mask else {
        return Err(OpError::NoPixels);
    };
    let mut inverted = (*mask.image).clone();
    for value in inverted.as_mut_slice() {
        *value = 255 - *value;
    }
    layer.mask = Some(Mask {
        image: Arc::new(inverted),
        linked: mask.linked,
        placement: mask.placement,
    });
    document.dirty = true;
    Ok(())
}

/// Bakes a mask into the pixels and drops it, as Apply Layer Mask does.
///
/// # Errors
///
/// Fails when the layer has no mask.
pub fn apply_mask(document: &mut Document, layer_id: Uuid) -> Result<(), OpError> {
    let Some(layer) = document.layer_mut(layer_id) else {
        return Err(OpError::NoActiveLayer);
    };
    let Some(mask) = &layer.mask else {
        return Err(OpError::NoPixels);
    };
    let mask = mask.image.clone();
    let Some(image) = layer.image.clone() else {
        layer.mask = None;
        return Ok(());
    };
    let mut updated = (*image).clone();
    let width = updated.width();
    let height = updated.height();
    for y in 0..height {
        for x in 0..width {
            let source_x = (x as f64 * f64::from(mask.width()) / f64::from(width)).floor();
            let source_y = (y as f64 * f64::from(mask.height()) / f64::from(height)).floor();
            let weight = u32::from(mask.pixel(source_x as u32, source_y as u32));
            if weight == 255 {
                continue;
            }
            let pixel = updated.pixel(x, y).unwrap_or([0; 4]);
            let mut scaled = [0_u8; 4];
            for channel in 0..4 {
                scaled[channel] = ((u32::from(pixel[channel]) * weight + 127) / 255) as u8;
            }
            updated.set_pixel(x, y, scaled);
        }
    }
    if let Some(layer) = document.layer_mut(layer_id) {
        layer.image = Some(Arc::new(updated));
        layer.mask = None;
    }
    document.dirty = true;
    Ok(())
}

/// Edit ▸ Fill ▸ Content-Aware: replaces the selected pixels with texture from around them.
///
/// # Errors
///
/// Fails without a selection, without pixels, or when the selection does not line up with the
/// layer's pixels.
pub fn content_aware_fill(
    document: &mut Document,
    layer_id: Uuid,
    selection: &Selection,
) -> Result<(), OpError> {
    let coverage = coverage_for_layer(document, layer_id, Some(selection))
        .ok_or(OpError::SelectionDoesNotLineUp)?;
    let Some(layer) = document.layer_mut(layer_id) else {
        return Err(OpError::NoActiveLayer);
    };
    let Some(image) = layer.image.clone() else {
        return Err(OpError::NoPixels);
    };
    let mut updated = (*image).clone();
    if !ffi::content_aware_fill(&mut updated, &coverage) {
        return Err(OpError::Kernel("内容感知填充找不到可用的纹理来源"));
    }
    if let Some(layer) = document.layer_mut(layer_id) {
        layer.image = Some(Arc::new(updated));
    }
    document.dirty = true;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::blend::BlendMode;

    fn document_with_red_layer(size: u32) -> (Document, Uuid) {
        let mut document = Document::new(size, size);
        let id = document.add_layer(
            "Background",
            Arc::new(PremulImage::filled(size, size, [255, 0, 0, 255])),
            None,
        );
        (document, id)
    }

    fn pixels(document: &Document, layer: Uuid) -> Arc<PremulImage> {
        document
            .layer(layer)
            .expect("layer")
            .image
            .clone()
            .expect("pixels")
    }

    #[test]
    fn brush_paints_the_clicked_pixel() {
        let (mut document, layer) = document_with_red_layer(64);
        let settings = BrushSettings {
            size: 10.0,
            hardness: 1.0,
            color: [0, 0, 255, 255],
            ..BrushSettings::default()
        };
        assert!(stamp_brush(
            &mut document,
            layer,
            [32.0, 32.0],
            &settings,
            None
        ));
        let image = pixels(&document, layer);
        assert_eq!(image.pixel(32, 32), Some([0, 0, 255, 255]));
        assert_eq!(image.pixel(0, 0), Some([255, 0, 0, 255]));
    }

    #[test]
    fn brushing_outside_the_layer_changes_nothing() {
        let (mut document, layer) = document_with_red_layer(32);
        assert!(!stamp_brush(
            &mut document,
            layer,
            [-10.0, -10.0],
            &BrushSettings::default(),
            None
        ));
        assert_eq!(pixels(&document, layer).pixel(0, 0), Some([255, 0, 0, 255]));
    }

    #[test]
    fn a_stroke_draws_a_line_between_its_ends() {
        let (mut document, layer) = document_with_red_layer(64);
        let settings = BrushSettings {
            size: 6.0,
            hardness: 1.0,
            color: [0, 255, 0, 255],
            ..BrushSettings::default()
        };
        stroke_segment(
            &mut document,
            layer,
            [8.0, 8.0],
            [56.0, 8.0],
            &settings,
            None,
        );
        let image = pixels(&document, layer);
        assert_eq!(image.pixel(8, 8).expect("pixel")[1], 255);
        assert_eq!(image.pixel(32, 8).expect("pixel")[1], 255);
        assert_eq!(image.pixel(8, 32).expect("pixel")[1], 0);
    }

    #[test]
    fn painting_stays_inside_the_selection() {
        let (mut document, layer) = document_with_red_layer(32);
        let selection = Selection::rectangle(32, 32, RectI::new(0, 0, 16, 32));
        let settings = BrushSettings {
            size: 30.0,
            hardness: 1.0,
            color: [0, 0, 255, 255],
            ..BrushSettings::default()
        };
        stamp_brush(
            &mut document,
            layer,
            [16.0, 16.0],
            &settings,
            Some(&selection),
        );
        let image = pixels(&document, layer);
        assert_eq!(image.pixel(8, 16).expect("pixel")[2], 255);
        assert_eq!(image.pixel(24, 16).expect("pixel")[2], 0);
    }

    #[test]
    fn erasing_takes_alpha_away() {
        let (mut document, layer) = document_with_red_layer(32);
        let settings = BrushSettings {
            size: 16.0,
            hardness: 1.0,
            erase: true,
            ..BrushSettings::default()
        };
        stamp_brush(&mut document, layer, [16.0, 16.0], &settings, None);
        let image = pixels(&document, layer);
        assert_eq!(image.pixel(16, 16).expect("pixel")[3], 0);
        assert_eq!(image.pixel(0, 0).expect("pixel")[3], 255);
    }

    #[test]
    fn fill_covers_the_selection_only() {
        let (mut document, layer) = document_with_red_layer(8);
        let selection = Selection::rectangle(8, 8, RectI::new(0, 0, 4, 8));
        fill(&mut document, layer, [0, 255, 0, 255], Some(&selection)).expect("fill");
        let image = pixels(&document, layer);
        assert_eq!(image.pixel(1, 1), Some([0, 255, 0, 255]));
        assert_eq!(image.pixel(5, 5), Some([255, 0, 0, 255]));
    }

    #[test]
    fn crop_moves_the_layers_with_the_canvas() {
        let (mut document, layer) = document_with_red_layer(32);
        crop(&mut document, RectI::new(8, 8, 24, 24)).expect("crop");
        assert_eq!((document.width, document.height), (16, 16));
        assert_eq!(
            document.layer(layer).expect("layer").transform.origin,
            [-8.0, -8.0]
        );
    }

    #[test]
    fn flipping_the_canvas_mirrors_a_full_canvas_layer_in_place() {
        let (mut document, layer) = document_with_red_layer(16);
        flip_canvas(&mut document, true);
        let transform = document.layer(layer).expect("layer").transform;
        assert!(transform.flip_x);
        assert_eq!(transform.origin[0], 0.0);
    }

    #[test]
    fn rotating_the_canvas_swaps_its_sides() {
        let mut document = Document::new(10, 4);
        document.add_layer(
            "Layer",
            Arc::new(PremulImage::filled(10, 4, [1, 2, 3, 255])),
            None,
        );
        rotate_canvas_90(&mut document, true).expect("rotate");
        assert_eq!((document.width, document.height), (4, 10));
        let image = document.layers[0].image.clone().expect("pixels");
        assert_eq!((image.width(), image.height()), (4, 10));
        assert_eq!(image.pixel(3, 0), Some([1, 2, 3, 255]));
    }

    #[test]
    fn adjustments_hit_the_layer_under_the_selection() {
        let (mut document, layer) = document_with_red_layer(8);
        let selection = Selection::rectangle(8, 8, RectI::new(0, 0, 4, 8));
        apply_adjustment_to_layer(&mut document, layer, &Adjustment::Invert, Some(&selection))
            .expect("invert");
        let image = pixels(&document, layer);
        assert_eq!(image.pixel(1, 1), Some([0, 255, 255, 255]));
        assert_eq!(image.pixel(5, 5), Some([255, 0, 0, 255]));
    }

    #[test]
    fn filters_change_the_pixels() {
        let (mut document, layer) = document_with_red_layer(16);
        apply_filter_to_layer(
            &mut document,
            layer,
            &Filter::AddNoise {
                amount: 80.0,
                gaussian: false,
                monochromatic: false,
                seed: 3,
            },
            None,
        )
        .expect("noise");
        assert_ne!(pixels(&document, layer).pixel(8, 8), Some([255, 0, 0, 255]));
    }

    #[test]
    fn masks_can_be_added_inverted_and_applied() {
        let (mut document, layer) = document_with_red_layer(8);
        add_mask(&mut document, layer, false).expect("mask");
        assert!(document.layer(layer).expect("layer").mask.is_some());
        invert_mask(&mut document, layer).expect("invert");
        assert_eq!(
            document
                .layer(layer)
                .expect("layer")
                .mask
                .as_ref()
                .expect("mask")
                .image
                .pixel(0, 0),
            255
        );
        apply_mask(&mut document, layer).expect("apply");
        assert!(document.layer(layer).expect("layer").mask.is_none());
        assert_eq!(pixels(&document, layer).pixel(0, 0), Some([255, 0, 0, 255]));
    }

    #[test]
    fn a_hidden_mask_does_not_hide_pixels() {
        let (mut document, layer) = document_with_red_layer(4);
        add_mask(&mut document, layer, false).expect("mask");
        let canvas = crate::core::composite::composite_document(&document);
        assert_eq!(canvas.pixel(1, 1), Some([0, 0, 0, 0]));
        document.layer_mut(layer).expect("layer").mask_enabled = false;
        let canvas = crate::core::composite::composite_document(&document);
        assert_eq!(canvas.pixel(1, 1), Some([255, 0, 0, 255]));
    }

    #[test]
    fn the_wand_selects_matching_pixels() {
        let mut document = Document::new(16, 16);
        let mut image = PremulImage::filled(16, 16, [255, 0, 0, 255]);
        for y in 0..16 {
            for x in 8..16 {
                image.set_pixel(x, y, [0, 0, 255, 255]);
            }
        }
        let layer = document.add_layer("Layer", Arc::new(image), None);
        let selection =
            magic_wand_selection(&document, layer, [1.0, 1.0], &WandSettings::default())
                .expect("wand");
        assert_eq!(selection.coverage_at(1, 1), 255);
        assert_eq!(selection.coverage_at(12, 12), 0);
    }

    #[test]
    fn loading_layer_alpha_gives_a_selection() {
        let mut document = Document::new(8, 8);
        let mut image = PremulImage::new(8, 8);
        for y in 0..8 {
            for x in 0..4 {
                image.set_pixel(x, y, [10, 10, 10, 255]);
            }
        }
        let layer = document.add_layer("Layer", Arc::new(image), None);
        let selection = load_layer_alpha(&document, layer).expect("alpha");
        assert_eq!(selection.bounds(), Some(RectI::new(0, 0, 4, 8)));
    }

    #[test]
    fn deleting_pixels_clears_the_selection() {
        let (mut document, layer) = document_with_red_layer(8);
        let selection = Selection::rectangle(8, 8, RectI::new(0, 0, 4, 8));
        delete_selected_pixels(&mut document, layer, &selection).expect("delete");
        let image = pixels(&document, layer);
        assert_eq!(image.pixel(1, 1).expect("pixel")[3], 0);
        assert_eq!(image.pixel(5, 5).expect("pixel")[3], 255);
    }

    #[test]
    fn trim_crops_to_the_pixels() {
        let mut document = Document::new(16, 16);
        let mut image = PremulImage::new(16, 16);
        for y in 4..12 {
            for x in 4..12 {
                image.set_pixel(x, y, [0, 0, 255, 255]);
            }
        }
        document.add_layer("Layer", Arc::new(image), None);
        trim(&mut document).expect("trim");
        assert_eq!((document.width, document.height), (8, 8));
    }

    #[test]
    fn an_adjustment_layer_is_inserted_above_the_active_one() {
        let (mut document, _) = document_with_red_layer(4);
        let id = add_adjustment_layer(&mut document, Adjustment::Invert);
        assert_eq!(document.active_layer, Some(id));
        assert_eq!(
            document.layer(id).expect("layer").kind,
            LayerKind::Adjustment
        );
        let canvas = crate::core::composite::composite_document(&document);
        assert_eq!(canvas.pixel(1, 1), Some([0, 255, 255, 255]));
    }

    #[test]
    fn a_layer_without_pixels_says_so() {
        let mut document = Document::new(4, 4);
        let mut layer = Layer::new_pixel("Empty", Arc::new(PremulImage::new(4, 4)));
        layer.image = None;
        layer.blend = BlendMode::Normal;
        let id = document.insert_layer(layer, 0);
        assert_eq!(
            apply_adjustment_to_layer(&mut document, id, &Adjustment::Invert, None).unwrap_err(),
            OpError::NoPixels
        );
    }

    #[test]
    fn healing_a_spot_on_a_solid_image_keeps_it_solid() {
        let (mut document, layer) = document_with_red_layer(32);
        heal(
            &mut document,
            layer,
            [16.0, 16.0],
            4.0,
            HealMode::ContentAware,
            5,
        )
        .expect("heal");
        let image = pixels(&document, layer);
        let healed = image.pixel(16, 16).expect("pixel");
        assert_eq!(healed[3], 255);
        assert!(healed[0] > 200, "the flat red should survive: {healed:?}");
    }
}

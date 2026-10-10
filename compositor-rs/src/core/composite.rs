//! Compositing the layer stack down to a picture.
//!
//! The rules follow the original application:
//!
//! - layers draw bottom to top; a folder's children draw where the folder sits in the stack
//! - a folder is **pass-through**: its opacity multiplies into every layer inside it, and its
//!   mask multiplies into their coverage, rather than being composited as a unit
//! - a hidden layer hides its children
//! - an adjustment layer changes everything composited below it inside its folder, limited to
//!   its own mask and opacity

use std::sync::Arc;

use uuid::Uuid;

use crate::core::adjustment::apply_adjustment;
use crate::core::blend::blend_pixel;
use crate::core::document::{Document, Layer, LayerKind, RectI, Sampling, Transform};
use crate::core::pixel::Rgba;
use crate::core::pixel::{GrayImage, PremulImage};

/// A mask that must be applied to everything drawn inside a folder, with the transform that
/// places it on the canvas.
#[derive(Clone)]
struct MaskRef {
    transform: Transform,
    image: Arc<GrayImage>,
}

/// Composites every visible layer into a canvas-sized image with a transparent background.
#[must_use]
pub fn composite_document(document: &Document) -> PremulImage {
    let mut canvas = PremulImage::new(document.width, document.height);
    let masks = Vec::new();
    composite_children(document, None, &mut canvas, 1.0, &masks);
    canvas
}

/// Composites a document over one color, which is what JPEG export and the checkerboard-free
/// preview want.
#[must_use]
pub fn composite_over(document: &Document, background: Rgba) -> PremulImage {
    let mut canvas = PremulImage::filled(document.width, document.height, background);
    let masks = Vec::new();
    composite_children(document, None, &mut canvas, 1.0, &masks);
    canvas
}

/// Composites two stacked layers on their own, for Merge Down. The result is cropped to what
/// the pair covers, clipped to the canvas.
#[must_use]
pub fn composite_layer_pair(
    document: &Document,
    lower: Uuid,
    upper: Uuid,
) -> Option<(PremulImage, RectI)> {
    let mut canvas = PremulImage::new(document.width, document.height);
    let masks = Vec::new();
    for id in [lower, upper] {
        let layer = document.layer(id)?;
        draw_pixel_layer(document, layer, &mut canvas, 1.0, &masks);
    }
    let mut bounds = RectI {
        x0: i64::MAX,
        y0: i64::MAX,
        x1: i64::MIN,
        y1: i64::MIN,
    };
    for id in [lower, upper] {
        let layer = document.layer(id)?;
        if let Some(image) = &layer.image {
            let _ = image;
            let layer_bounds = layer.transform.document_bounds();
            bounds.x0 = bounds.x0.min(layer_bounds.x0);
            bounds.y0 = bounds.y0.min(layer_bounds.y0);
            bounds.x1 = bounds.x1.max(layer_bounds.x1);
            bounds.y1 = bounds.y1.max(layer_bounds.y1);
        }
    }
    let bounds = bounds.intersect(&document.canvas_rect());
    if bounds.is_empty() {
        return None;
    }
    let cropped = canvas.crop(
        bounds.x0,
        bounds.y0,
        bounds.width() as u32,
        bounds.height() as u32,
    )?;
    Some((cropped, bounds))
}

/// Walks one level of the tree. `masks` are the folders' masks above this level.
fn composite_children(
    document: &Document,
    parent: Option<Uuid>,
    canvas: &mut PremulImage,
    opacity: f64,
    masks: &[MaskRef],
) {
    for id in document.children(parent) {
        let Some(layer) = document.layer(id) else {
            continue;
        };
        if !layer.visible {
            continue;
        }
        match layer.kind {
            LayerKind::Group => {
                let mut inner = masks.to_vec();
                if let Some(mask) = folder_mask(layer) {
                    inner.push(mask);
                }
                composite_children(document, Some(id), canvas, opacity * layer.opacity, &inner);
            }
            LayerKind::Adjustment => {
                apply_adjustment_layer(layer, canvas, opacity, masks);
            }
            LayerKind::Pixel => {
                draw_pixel_layer(document, layer, canvas, opacity, masks);
            }
        }
    }
}

/// A folder's mask, as the `MaskRef` that gets passed down to its contents.
fn folder_mask(layer: &Layer) -> Option<MaskRef> {
    let mask = layer.mask.as_ref()?;
    if !layer.mask_enabled {
        return None;
    }
    Some(MaskRef {
        transform: mask.placement.unwrap_or(layer.transform),
        image: mask.image.clone(),
    })
}

/// Draws one pixel layer onto the canvas.
fn draw_pixel_layer(
    document: &Document,
    layer: &Layer,
    canvas: &mut PremulImage,
    inherited_opacity: f64,
    masks: &[MaskRef],
) {
    let Some(image) = &layer.image else {
        return;
    };
    // A layer's own opacity multiplies into what its ancestors hand down.
    let opacity = inherited_opacity * layer.opacity;
    if opacity <= 0.0 {
        return;
    }
    let own_mask = layer
        .mask
        .as_ref()
        .filter(|_| layer.mask_enabled)
        .map(|mask| MaskRef {
            transform: mask.placement.unwrap_or(layer.transform),
            image: mask.image.clone(),
        });
    let mut active = masks.to_vec();
    if let Some(mask) = own_mask {
        active.push(mask);
    }

    // A layer that sits exactly on the canvas and has nothing masking it can be copied straight
    // across, which is the common case and much faster than sampling.
    let offset = [layer.transform.origin[0], layer.transform.origin[1]];
    let integer_offset = offset[0].fract() == 0.0 && offset[1].fract() == 0.0;
    if layer.is_identity() && integer_offset && active.is_empty() {
        let destination = [offset[0] as i64, offset[1] as i64];
        if layer.blend == crate::core::blend::BlendMode::Normal && opacity >= 1.0 {
            canvas.blit(image, destination[0], destination[1]);
        } else {
            for y in 0..image.height() {
                for x in 0..image.width() {
                    let Some(pixel) = image.pixel(x, y) else {
                        continue;
                    };
                    let target_x = destination[0] + i64::from(x);
                    let target_y = destination[1] + i64::from(y);
                    if !(0..i64::from(canvas.width())).contains(&target_x)
                        || !(0..i64::from(canvas.height())).contains(&target_y)
                    {
                        continue;
                    }
                    let base = canvas
                        .pixel(target_x as u32, target_y as u32)
                        .unwrap_or([0; 4]);
                    let out = blend_pixel(layer.blend, base, pixel, opacity as f32);
                    canvas.set_pixel(target_x as u32, target_y as u32, out);
                }
            }
        }
        return;
    }

    let image_size = [f64::from(image.width()), f64::from(image.height())];
    let bounds = layer
        .transform
        .document_bounds()
        .intersect(&document.canvas_rect());
    if bounds.is_empty() {
        return;
    }
    for y in bounds.y0..bounds.y1 {
        for x in bounds.x0..bounds.x1 {
            let point = [x as f64 + 0.5, y as f64 + 0.5];
            let Some(local) = layer.transform.document_to_image(point, image_size) else {
                continue;
            };
            let sample = sample_image(image, local, layer.transform.sampling);
            if sample[3] == 0 {
                continue;
            }
            let mut amount = opacity;
            if !active.is_empty() {
                amount *= coverage_at(&active, point);
                if amount <= 0.0 {
                    continue;
                }
            }
            let base = canvas.pixel(x as u32, y as u32).unwrap_or([0; 4]);
            let out = blend_pixel(layer.blend, base, sample, amount as f32);
            canvas.set_pixel(x as u32, y as u32, out);
        }
    }
}

/// Applies an adjustment layer to everything below it, limited by its mask and opacity.
fn apply_adjustment_layer(
    layer: &Layer,
    canvas: &mut PremulImage,
    inherited_opacity: f64,
    masks: &[MaskRef],
) {
    let Some(adjustment) = &layer.adjustment else {
        return;
    };
    let opacity = inherited_opacity * layer.opacity;
    if opacity <= 0.0 {
        return;
    }
    let mut active = masks.to_vec();
    if let Some(mask) = layer.mask.as_ref().filter(|_| layer.mask_enabled) {
        active.push(MaskRef {
            transform: mask.placement.unwrap_or(layer.transform),
            image: mask.image.clone(),
        });
    }
    let mut processed = canvas.clone();
    apply_adjustment(adjustment, &mut processed, None);
    if active.is_empty() && (opacity - 1.0).abs() < f64::EPSILON {
        *canvas = processed;
        return;
    }
    let whole = opacity.clamp(0.0, 1.0) as f32;
    let width = canvas.width();
    for y in 0..canvas.height() {
        for x in 0..width {
            let point = [x as f64 + 0.5, y as f64 + 0.5];
            let amount = if active.is_empty() {
                opacity
            } else {
                opacity * coverage_at(&active, point)
            };
            if amount <= 0.0 {
                continue;
            }
            let base = canvas.pixel(x, y).unwrap_or([0; 4]);
            let target = processed.pixel(x, y).unwrap_or(base);
            let mut out = [0_u8; 4];
            for channel in 0..4 {
                let amount = if active.is_empty() {
                    whole
                } else {
                    (amount as f32).clamp(0.0, 1.0)
                };
                out[channel] = (f32::from(base[channel])
                    + (f32::from(target[channel]) - f32::from(base[channel])) * amount)
                    .round()
                    .clamp(0.0, 255.0) as u8;
            }
            canvas.set_pixel(x, y, out);
        }
    }
}

/// How much of an edit a point receives, as 0–1: the product of every mask covering it.
fn coverage_at(masks: &[MaskRef], point: [f64; 2]) -> f64 {
    let mut coverage = 1.0;
    for mask in masks {
        let size = [
            f64::from(mask.image.width()),
            f64::from(mask.image.height()),
        ];
        let Some(local) = mask.transform.document_to_image(point, size) else {
            return 0.0;
        };
        let x = (local[0].floor() as i64).clamp(0, i64::from(mask.image.width()) - 1) as u32;
        let y = (local[1].floor() as i64).clamp(0, i64::from(mask.image.height()) - 1) as u32;
        coverage *= f64::from(mask.image.pixel(x, y)) / 255.0;
        if coverage <= 0.0 {
            return 0.0;
        }
    }
    coverage
}

/// How much of an edit a point receives, for callers outside this module (the tools limit a
/// stroke to the same coverage the renderer uses).
#[must_use]
pub fn layer_coverage_at(document: &Document, layer: &Layer, point: [f64; 2]) -> f64 {
    let mut masks = Vec::new();
    let mut parent = layer.parent;
    let mut guard = 0;
    while let Some(id) = parent {
        guard += 1;
        if guard > 64 {
            break;
        }
        let Some(group) = document.layer(id) else {
            break;
        };
        if group.visible
            && let Some(mask) = folder_mask(group)
        {
            masks.push(mask);
        }
        parent = group.parent;
    }
    if let Some(mask) = layer.mask.as_ref().filter(|_| layer.mask_enabled) {
        masks.push(MaskRef {
            transform: mask.placement.unwrap_or(layer.transform),
            image: mask.image.clone(),
        });
    }
    coverage_at(&masks, point)
}

fn sample_image(image: &PremulImage, local: [f64; 2], sampling: Sampling) -> Rgba {
    let x = local[0] - 0.5;
    let y = local[1] - 0.5;
    match sampling {
        Sampling::Nearest => {
            let sx = (local[0].floor() as i64).clamp(0, i64::from(image.width()) - 1) as u32;
            let sy = (local[1].floor() as i64).clamp(0, i64::from(image.height()) - 1) as u32;
            image.pixel(sx, sy).unwrap_or([0; 4])
        }
        Sampling::Smooth | Sampling::HighQuality => {
            let x0 = x.floor() as i64;
            let y0 = y.floor() as i64;
            let fx = (x - x0 as f64) as f32;
            let fy = (y - y0 as f64) as f32;
            let mut out = [0_u8; 4];
            for channel in 0..4 {
                let mut value = 0.0_f32;
                for (dx, dy, weight) in [
                    (0, 0, (1.0 - fx) * (1.0 - fy)),
                    (1, 0, fx * (1.0 - fy)),
                    (0, 1, (1.0 - fx) * fy),
                    (1, 1, fx * fy),
                ] {
                    if weight <= 0.0 {
                        continue;
                    }
                    let sx = (x0 + dx).clamp(0, i64::from(image.width()) - 1) as u32;
                    let sy = (y0 + dy).clamp(0, i64::from(image.height()) - 1) as u32;
                    let pixel = image.pixel(sx, sy).unwrap_or([0; 4]);
                    value += f32::from(pixel[channel]) * weight;
                }
                out[channel] = value.round().clamp(0.0, 255.0) as u8;
            }
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::blend::BlendMode;

    fn document_with_layers() -> Document {
        let mut document = Document::new(4, 4);
        document.add_layer(
            "Red",
            Arc::new(PremulImage::filled(4, 4, [255, 0, 0, 255])),
            None,
        );
        document.add_layer(
            "Blue",
            Arc::new(PremulImage::filled(2, 2, [0, 0, 255, 255])),
            None,
        );
        let blue = document.layers[1].id;
        let layer = document.layer_mut(blue).expect("layer");
        layer.transform.origin = [1.0, 1.0];
        layer.transform.size = [2.0, 2.0];
        document
    }

    #[test]
    fn later_layers_draw_on_top() {
        let document = document_with_layers();
        let canvas = composite_document(&document);
        assert_eq!(canvas.pixel(0, 0), Some([255, 0, 0, 255]));
        assert_eq!(canvas.pixel(2, 2), Some([0, 0, 255, 255]));
    }

    #[test]
    fn hidden_layers_are_skipped() {
        let mut document = document_with_layers();
        let top = document.layers[1].id;
        document.layer_mut(top).expect("layer").visible = false;
        let canvas = composite_document(&document);
        assert_eq!(canvas.pixel(2, 2), Some([255, 0, 0, 255]));
    }

    #[test]
    fn layer_opacity_blends_with_the_backdrop() {
        let mut document = document_with_layers();
        let top = document.layers[1].id;
        document.layer_mut(top).expect("layer").opacity = 0.5;
        let canvas = composite_document(&document);
        let pixel = canvas.pixel(2, 2).expect("pixel");
        assert!((127..=129).contains(&pixel[2]), "got {pixel:?}");
    }

    #[test]
    fn folder_opacity_multiplies_into_children() {
        let mut document = document_with_layers();
        let top = document.layers[1].id;
        let group = document.group_layers(&[top], "Folder").expect("group");
        document.layer_mut(group).expect("group").opacity = 0.5;
        document.layer_mut(top).expect("layer").opacity = 0.5;
        let canvas = composite_document(&document);
        let pixel = canvas.pixel(2, 2).expect("pixel");
        // 25% of the blue layer.
        assert!((63..=65).contains(&pixel[2]), "got {pixel:?}");
    }

    #[test]
    fn hiding_a_folder_hides_its_children() {
        let mut document = document_with_layers();
        let top = document.layers[1].id;
        let group = document.group_layers(&[top], "Folder").expect("group");
        document.layer_mut(group).expect("group").visible = false;
        let canvas = composite_document(&document);
        assert_eq!(canvas.pixel(2, 2), Some([255, 0, 0, 255]));
    }

    #[test]
    fn masks_limit_a_layer() {
        let mut document = document_with_layers();
        let top = document.layers[1].id;
        let mask = GrayImage::new(2, 2, 0);
        {
            let layer = document.layer_mut(top).expect("layer");
            layer.mask = Some(crate::core::document::Mask {
                image: Arc::new(mask),
                linked: true,
                placement: None,
            });
        }
        let canvas = composite_document(&document);
        assert_eq!(canvas.pixel(2, 2), Some([255, 0, 0, 255]));
        assert_eq!(canvas.pixel(1, 1), Some([255, 0, 0, 255]));
    }

    #[test]
    fn a_disabled_mask_does_not_hide_anything() {
        let mut document = document_with_layers();
        let top = document.layers[1].id;
        {
            let layer = document.layer_mut(top).expect("layer");
            layer.mask = Some(crate::core::document::Mask {
                image: Arc::new(GrayImage::new(2, 2, 0)),
                linked: true,
                placement: None,
            });
            layer.mask_enabled = false;
        }
        let canvas = composite_document(&document);
        assert_eq!(canvas.pixel(2, 2), Some([0, 0, 255, 255]));
    }

    #[test]
    fn an_adjustment_layer_changes_what_is_below_it() {
        let mut document = document_with_layers();
        let mut layer = Layer::new_pixel("Invert", Arc::new(PremulImage::new(4, 4)));
        layer.kind = LayerKind::Adjustment;
        layer.image = None;
        layer.adjustment = Some(crate::core::adjustment::Adjustment::Invert);
        layer.transform = Transform::full(4, 4);
        document.layers.push(layer);
        let canvas = composite_document(&document);
        // The adjustment covers the whole canvas, so it inverts the blue patch too.
        assert_eq!(canvas.pixel(0, 0), Some([0, 255, 255, 255]));
        assert_eq!(canvas.pixel(2, 2), Some([255, 255, 0, 255]));
    }

    #[test]
    fn merging_a_pair_keeps_their_look() {
        let mut document = document_with_layers();
        let top = document.layers[1].id;
        document.layers[1].blend = BlendMode::Normal;
        let (merged, bounds) = composite_layer_pair(&document, document.layers[0].id, top)
            .expect("merge produces pixels");
        assert_eq!(bounds, RectI::new(0, 0, 4, 4));
        assert_eq!(merged.pixel(0, 0), Some([255, 0, 0, 255]));
        assert_eq!(merged.pixel(2, 2), Some([0, 0, 255, 255]));
    }
}

//! The document model: a canvas, a stack of layers and the project's metadata.
//!
//! The shape mirrors the macOS application's `ProjectManifest` so a `.comp` project written by
//! either build opens in the other. Layer records that this build understands but does not
//! render (text, layer effects, shapes, clipping links) are kept in [`Layer::extra`] and
//! written back untouched, so editing a project here never destroys that data.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use uuid::Uuid;

use crate::core::adjustment::Adjustment;
use crate::core::blend::BlendMode;
use crate::core::pixel::{GrayImage, PremulImage};

pub use crate::core::geometry::{RectI, Sampling, Transform};

/// The canvas's alignment guides, saved with the project.
#[derive(Clone, Debug, PartialEq)]
pub struct Guide {
    pub id: Uuid,
    pub horizontal: bool,
    pub position: f64,
}

/// A layer's mask: 8-bit coverage, white revealing and black hiding.
#[derive(Clone, Debug)]
pub struct Mask {
    pub image: Arc<GrayImage>,
    /// A linked mask follows its layer's transform; an unlinked one keeps its own.
    pub linked: bool,
    pub placement: Option<Transform>,
}

/// What a layer holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerKind {
    /// Raster pixels.
    Pixel,
    /// A folder; it has no pixels of its own.
    Group,
    /// An adjustment applied to everything composited below it.
    Adjustment,
}

/// One entry in the stack.
#[derive(Clone, Debug)]
pub struct Layer {
    pub id: Uuid,
    pub name: String,
    pub kind: LayerKind,
    pub visible: bool,
    pub parent: Option<Uuid>,
    /// 0–1; a folder's opacity dims everything inside it.
    pub opacity: f64,
    pub blend: BlendMode,
    pub transform: Transform,
    pub image: Option<Arc<PremulImage>>,
    pub mask: Option<Mask>,
    pub mask_enabled: bool,
    /// Set for adjustment layers this build renders.
    pub adjustment: Option<Adjustment>,
    /// The manifest's `adjustment` object, kept verbatim for kinds this build does not render
    /// and re-emitted on save.
    pub adjustment_raw: Option<serde_json::Value>,
    /// Manifest fields this build preserves but does not render (`effects`, `text`, `shape`,
    /// `maskSourceID`, …).
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl Layer {
    /// A pixel layer covering the whole canvas.
    #[must_use]
    pub fn new_pixel(name: impl Into<String>, image: Arc<PremulImage>) -> Self {
        let size = [f64::from(image.width()), f64::from(image.height())];
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            kind: LayerKind::Pixel,
            visible: true,
            parent: None,
            opacity: 1.0,
            blend: BlendMode::Normal,
            transform: Transform {
                origin: [0.0, 0.0],
                size,
                rotation: 0.0,
                flip_x: false,
                flip_y: false,
                sampling: Sampling::HighQuality,
            },
            image: Some(image),
            mask: None,
            mask_enabled: true,
            adjustment: None,
            adjustment_raw: None,
            extra: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn image_size(&self) -> [f64; 2] {
        self.image.as_ref().map_or([0.0, 0.0], |image| {
            [f64::from(image.width()), f64::from(image.height())]
        })
    }

    /// True while the layer does not move or scale its pixels.
    #[must_use]
    pub fn is_identity(&self) -> bool {
        let size = self.image_size();
        self.transform.is_identity_for(size) && self.transform.origin == [0.0, 0.0]
    }

    #[must_use]
    pub fn has_renderable_mask(&self) -> bool {
        self.mask.is_some() && self.mask_enabled
    }
}

/// What went wrong while editing the stack.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DocumentError {
    #[error("找不到图层")]
    MissingLayer,
    #[error("该图层没有像素")]
    NoPixels,
    #[error("该图层是文件夹")]
    IsGroup,
    #[error("下方没有可合并的图层")]
    NothingBelow,
    #[error("画布尺寸 {width}×{height} 超出支持范围")]
    BadSize { width: u32, height: u32 },
    #[error("矩形与画布没有重叠区域")]
    OutsideCanvas,
    #[error("该调整图层使用了本版本无法渲染的调整类型")]
    UnsupportedAdjustment,
}

/// The largest canvas the original application opens; kept so both builds agree on limits.
pub const MAX_SIDE: u32 = 30_000;
/// The original's document budget, in megapixels.
pub const MAX_MEGAPIXELS: u64 = 100;

/// A project: canvas size, layers in bottom-to-top order and session metadata.
#[derive(Clone, Debug)]
pub struct Document {
    pub id: Uuid,
    pub width: u32,
    pub height: u32,
    /// Pixels per inch, as the project format stores it.
    pub resolution: f64,
    pub color_space: String,
    /// Bottom to top; a group's children follow it in the array, as the format requires.
    pub layers: Vec<Layer>,
    pub active_layer: Option<Uuid>,
    pub guides: Vec<Guide>,
    /// Where the project lives on disk, if it has been saved.
    pub path: Option<PathBuf>,
    /// Unchanged and unsaved edits.
    pub dirty: bool,
    /// Manifest fields this build does not model, written back on save.
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl Document {
    /// An empty canvas with no layers.
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            id: Uuid::new_v4(),
            width,
            height,
            resolution: 72.0,
            color_space: "sRGB".to_owned(),
            layers: Vec::new(),
            active_layer: None,
            guides: Vec::new(),
            path: None,
            dirty: false,
            extra: BTreeMap::new(),
        }
    }

    /// A new document with one full-canvas layer, as File ▸ New creates.
    #[must_use]
    pub fn with_background(width: u32, height: u32, background: [u8; 4]) -> Self {
        let mut document = Self::new(width, height);
        if background[3] > 0 {
            let image = Arc::new(PremulImage::filled(width, height, background));
            let layer = Layer::new_pixel("Background", image);
            document.active_layer = Some(layer.id);
            document.layers.push(layer);
        }
        document
    }

    #[must_use]
    pub fn canvas_rect(&self) -> RectI {
        RectI::new(0, 0, i64::from(self.width), i64::from(self.height))
    }

    #[must_use]
    pub fn index_of(&self, id: Uuid) -> Option<usize> {
        self.layers.iter().position(|layer| layer.id == id)
    }

    #[must_use]
    pub fn layer(&self, id: Uuid) -> Option<&Layer> {
        self.layers.iter().find(|layer| layer.id == id)
    }

    pub fn layer_mut(&mut self, id: Uuid) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|layer| layer.id == id)
    }

    /// The layer the user is working on.
    #[must_use]
    pub fn active(&self) -> Option<&Layer> {
        self.active_layer.and_then(|id| self.layer(id))
    }

    pub fn active_mut(&mut self) -> Option<&mut Layer> {
        let id = self.active_layer?;
        self.layer_mut(id)
    }

    /// The ids of a group's direct children, in stack order.
    #[must_use]
    pub fn children(&self, parent: Option<Uuid>) -> Vec<Uuid> {
        self.layers
            .iter()
            .filter(|layer| layer.parent == parent)
            .map(|layer| layer.id)
            .collect()
    }

    #[must_use]
    pub fn is_group(&self, id: Uuid) -> bool {
        self.layer(id)
            .is_some_and(|layer| layer.kind == LayerKind::Group)
    }

    /// True when `ancestor` is somewhere above `id` in the tree.
    #[must_use]
    pub fn is_ancestor(&self, ancestor: Uuid, id: Uuid) -> bool {
        let mut current = self.layer(id).and_then(|layer| layer.parent);
        let mut guard = 0;
        while let Some(parent) = current {
            if parent == ancestor {
                return true;
            }
            guard += 1;
            if guard > 64 {
                return false;
            }
            current = self.layer(parent).and_then(|layer| layer.parent);
        }
        false
    }

    /// The layers a group contains, including nested ones, deepest last.
    #[must_use]
    pub fn descendants(&self, group: Uuid) -> Vec<Uuid> {
        let mut out = Vec::new();
        let mut stack = self.children(Some(group));
        while let Some(id) = stack.pop() {
            out.push(id);
            if self.is_group(id) {
                stack.extend(self.children(Some(id)));
            }
        }
        out
    }

    /// Adds a pixel layer just above `above` (or on top when `None`), and selects it.
    pub fn add_layer(
        &mut self,
        name: impl Into<String>,
        image: Arc<PremulImage>,
        above: Option<Uuid>,
    ) -> Uuid {
        let mut layer = Layer::new_pixel(name, image);
        layer.parent = above.and_then(|id| self.layer(id)).and_then(|l| l.parent);
        let position = match above.and_then(|id| self.index_of(id)) {
            Some(index) => index + 1,
            None => self.layers.len(),
        };
        let id = layer.id;
        self.layers.insert(position, layer);
        self.active_layer = Some(id);
        self.dirty = true;
        id
    }

    /// Inserts a layer that was built elsewhere (loading a project, pasting).
    pub fn insert_layer(&mut self, layer: Layer, index: usize) -> Uuid {
        let id = layer.id;
        let index = index.min(self.layers.len());
        self.layers.insert(index, layer);
        self.active_layer = Some(id);
        self.dirty = true;
        id
    }

    /// Inserts a subtree: `layer` plus everything it contains, in order.
    pub fn insert_subtree(&mut self, layers: Vec<Layer>, index: usize) {
        let index = index.min(self.layers.len());
        for (offset, layer) in layers.into_iter().enumerate() {
            self.layers.insert(index + offset, layer);
        }
        self.dirty = true;
    }

    /// Removes a layer (and everything inside it) from the stack.
    pub fn remove_layer(&mut self, id: Uuid) -> Option<Layer> {
        let index = self.index_of(id)?;
        let mut removed = Vec::new();
        let mut to_remove = vec![id];
        while let Some(current) = to_remove.pop() {
            to_remove.extend(self.children(Some(current)));
            if let Some(position) = self.index_of(current) {
                removed.push(self.layers.remove(position));
            }
        }
        // Keep the selection on something that still exists.
        if self
            .active_layer
            .is_some_and(|active| self.layer(active).is_none())
        {
            self.active_layer = self
                .layers
                .get(index.min(self.layers.len().saturating_sub(1)))
                .or_else(|| self.layers.first())
                .map(|layer| layer.id);
        }
        self.dirty = true;
        removed.into_iter().next()
    }

    /// A layer and everything inside it, parents before children — the order the project file
    /// wants, because the format reads the tree back with `parentID`.
    #[must_use]
    pub fn subtree(&self, id: Uuid) -> Vec<Layer> {
        let mut out = Vec::new();
        let Some(layer) = self.layer(id) else {
            return out;
        };
        out.push(layer.clone());
        for child in self.children(Some(id)) {
            out.extend(self.subtree(child));
        }
        out
    }

    /// Every layer, parents before children, roots bottom to top: the order to write to disk.
    #[must_use]
    pub fn save_order(&self) -> Vec<Uuid> {
        let mut out = Vec::new();
        for id in self.children(None) {
            out.extend(self.subtree(id).into_iter().map(|layer| layer.id));
        }
        out
    }

    /// Copies a layer, and everything inside it, directly above the original.
    pub fn duplicate_layer(&mut self, id: Uuid) -> Option<Uuid> {
        let index = self.index_of(id)?;
        let subtree = self.subtree(id);
        let mut id_map: BTreeMap<Uuid, Uuid> = BTreeMap::new();
        for layer in &subtree {
            id_map.insert(layer.id, Uuid::new_v4());
        }
        let copies: Vec<Layer> = subtree
            .into_iter()
            .map(|source| {
                let mut copy = source.clone();
                let original_id = copy.id;
                copy.id = id_map[&original_id];
                copy.parent = copy
                    .parent
                    .map(|parent| *id_map.get(&parent).unwrap_or(&parent));
                if original_id == id {
                    copy.name = format!("{} copy", source.name);
                }
                copy
            })
            .collect();
        let new_id = copies.first().map(|layer| layer.id)?;
        let position = index + 1;
        for (offset, layer) in copies.into_iter().enumerate() {
            self.layers.insert(position + offset, layer);
        }
        self.active_layer = Some(new_id);
        self.dirty = true;
        Some(new_id)
    }

    /// Moves a layer to a new position in the flat stack.
    pub fn move_layer(&mut self, id: Uuid, target: usize) -> bool {
        let Some(index) = self.index_of(id) else {
            return false;
        };
        let target = target.min(self.layers.len().saturating_sub(1));
        if index == target {
            return false;
        }
        let layer = self.layers.remove(index);
        self.layers.insert(target, layer);
        self.dirty = true;
        true
    }

    /// Moves a layer one step up (`+1`) or down (`-1`) among its siblings.
    pub fn nudge_layer(&mut self, id: Uuid, delta: i32) -> bool {
        let Some(index) = self.index_of(id) else {
            return false;
        };
        let parent = self.layers[index].parent;
        let siblings = self.children(parent);
        let Some(sibling_index) = siblings.iter().position(|sibling| *sibling == id) else {
            return false;
        };
        let new_index = sibling_index as i64 + i64::from(delta);
        if new_index < 0 || new_index >= siblings.len() as i64 {
            return false;
        }
        let target_id = siblings[new_index as usize];
        let target = self.index_of(target_id).expect("sibling exists");
        self.layers.swap(index, target);
        self.dirty = true;
        true
    }

    /// Puts the selected layers into a new folder above the topmost of them.
    pub fn group_layers(&mut self, ids: &[Uuid], name: impl Into<String>) -> Option<Uuid> {
        let mut indices: Vec<usize> = ids.iter().filter_map(|id| self.index_of(*id)).collect();
        if indices.is_empty() {
            return None;
        }
        indices.sort_unstable();
        indices.dedup();
        let parent = self.layers[indices[0]].parent;
        // Remove the members, keeping their relative order; layers below them keep their index,
        // so the folder lands exactly where the lowest member sat.
        let mut children = Vec::new();
        for index in indices.iter().rev() {
            children.push(self.layers.remove(*index));
        }
        children.reverse();
        let position = indices[0];
        let group = Layer {
            id: Uuid::new_v4(),
            name: name.into(),
            kind: LayerKind::Group,
            visible: true,
            parent,
            opacity: 1.0,
            blend: BlendMode::Normal,
            transform: Transform::full(self.width, self.height),
            image: None,
            mask: None,
            mask_enabled: true,
            adjustment: None,
            adjustment_raw: None,
            extra: BTreeMap::new(),
        };
        let group_id = group.id;
        for child in &mut children {
            child.parent = Some(group_id);
        }
        let position = position.min(self.layers.len());
        self.layers.insert(position, group);
        for (offset, child) in children.into_iter().enumerate() {
            self.layers.insert(position + 1 + offset, child);
        }
        self.active_layer = Some(group_id);
        self.dirty = true;
        Some(group_id)
    }

    /// Lifts a group's children out, one level up.
    pub fn ungroup(&mut self, id: Uuid) -> bool {
        let Some(group) = self.layer(id).cloned() else {
            return false;
        };
        if group.kind != LayerKind::Group {
            return false;
        }
        let children = self.children(Some(id));
        let first_child = children.first().copied();
        let index = self.index_of(id).unwrap_or(0);
        let mut lifted = Vec::new();
        for child in children {
            if let Some(mut layer) = self.layer(child).cloned() {
                layer.parent = group.parent;
                lifted.push(layer);
            }
        }
        self.remove_layer(id);
        let position = index.min(self.layers.len());
        for (offset, layer) in lifted.into_iter().enumerate() {
            self.layers.insert(position + offset, layer);
        }
        self.active_layer = first_child.or_else(|| self.layers.last().map(|layer| layer.id));
        self.dirty = true;
        true
    }

    /// Merges a layer into the one below it, keeping the lower layer's place in the stack.
    ///
    /// # Errors
    ///
    /// Fails when there is nothing below, when either layer is a folder, or when the pair is
    /// empty.
    pub fn merge_down(&mut self, id: Uuid) -> Result<Uuid, DocumentError> {
        let index = self.index_of(id).ok_or(DocumentError::MissingLayer)?;
        let parent = self.layers[index].parent;
        let siblings_below: Vec<usize> = (0..index)
            .filter(|position| self.layers[*position].parent == parent)
            .collect();
        let Some(&lower_index) = siblings_below.last() else {
            return Err(DocumentError::NothingBelow);
        };
        if self.layers[index].kind == LayerKind::Group
            || self.layers[lower_index].kind == LayerKind::Group
        {
            return Err(DocumentError::IsGroup);
        }
        let upper_id = self.layers[index].id;
        let lower_id = self.layers[lower_index].id;
        let merged = crate::core::composite::composite_layer_pair(self, lower_id, upper_id);
        let Some(merged) = merged else {
            return Err(DocumentError::NoPixels);
        };
        let (image, bounds) = merged;
        let lower = &mut self.layers[lower_index];
        lower.image = Some(Arc::new(image));
        lower.kind = LayerKind::Pixel;
        lower.adjustment = None;
        lower.adjustment_raw = None;
        lower.transform.origin = [bounds.x0 as f64, bounds.y0 as f64];
        lower.transform.size = [bounds.width() as f64, bounds.height() as f64];
        lower.transform.rotation = 0.0;
        lower.transform.flip_x = false;
        lower.transform.flip_y = false;
        lower.opacity = 1.0;
        lower.blend = BlendMode::Normal;
        self.remove_layer(upper_id);
        self.active_layer = Some(lower_id);
        self.dirty = true;
        Ok(lower_id)
    }

    /// Collapses the whole stack into one full-canvas layer.
    pub fn flatten(&mut self) -> Uuid {
        let flattened = crate::core::composite::composite_document(self);
        let layer = Layer::new_pixel("Background", Arc::new(flattened));
        let id = layer.id;
        self.layers.clear();
        self.layers.push(layer);
        self.active_layer = Some(id);
        self.dirty = true;
        id
    }

    /// Changes the canvas size, keeping layer pixels where they are (`anchor` is 0–1 in each
    /// axis, so 0.5 keeps the center).
    ///
    /// # Errors
    ///
    /// Fails when the requested size is out of range.
    pub fn resize_canvas(
        &mut self,
        width: u32,
        height: u32,
        anchor: [f64; 2],
    ) -> Result<(), DocumentError> {
        check_size(width, height)?;
        let dx = ((f64::from(width) - f64::from(self.width)) * anchor[0]).round();
        let dy = ((f64::from(height) - f64::from(self.height)) * anchor[1]).round();
        self.width = width;
        self.height = height;
        for layer in &mut self.layers {
            layer.transform.origin[0] += dx;
            layer.transform.origin[1] += dy;
        }
        for guide in &mut self.guides {
            if guide.horizontal {
                guide.position += dy;
            } else {
                guide.position += dx;
            }
        }
        self.dirty = true;
        Ok(())
    }

    /// Scales the whole document, pixels and canvas together.
    ///
    /// # Errors
    ///
    /// Fails when the requested size is out of range.
    pub fn resize_image(
        &mut self,
        width: u32,
        height: u32,
        resample: bool,
    ) -> Result<(), DocumentError> {
        check_size(width, height)?;
        let scale_x = f64::from(width) / f64::from(self.width.max(1));
        let scale_y = f64::from(height) / f64::from(self.height.max(1));
        for layer in &mut self.layers {
            layer.transform.origin[0] *= scale_x;
            layer.transform.origin[1] *= scale_y;
            layer.transform.size[0] *= scale_x;
            layer.transform.size[1] *= scale_y;
            if resample {
                if let Some(image) = &layer.image {
                    let target_width = (f64::from(image.width()) * scale_x).round().max(1.0) as u32;
                    let target_height =
                        (f64::from(image.height()) * scale_y).round().max(1.0) as u32;
                    let scaled = image.scale_to(target_width, target_height);
                    layer.image = Some(Arc::new(scaled));
                }
                if let Some(mask) = &layer.mask {
                    let target_width =
                        (f64::from(mask.image.width()) * scale_x).round().max(1.0) as u32;
                    let target_height =
                        (f64::from(mask.image.height()) * scale_y).round().max(1.0) as u32;
                    layer.mask = Some(Mask {
                        image: Arc::new(resample_gray(&mask.image, target_width, target_height)),
                        linked: mask.linked,
                        placement: mask.placement,
                    });
                }
            }
        }
        self.width = width;
        self.height = height;
        self.dirty = true;
        Ok(())
    }
}

/// Nearest-neighbor resample for masks, which keeps them exactly 0 or 255 where they were.
fn resample_gray(image: &GrayImage, width: u32, height: u32) -> GrayImage {
    let mut out = GrayImage::new(width, height, 0);
    if image.width() == 0 || image.height() == 0 {
        return out;
    }
    for y in 0..height {
        let source_y = (f64::from(y) * f64::from(image.height()) / f64::from(height))
            .floor()
            .min(f64::from(image.height() - 1)) as u32;
        for x in 0..width {
            let source_x = (f64::from(x) * f64::from(image.width()) / f64::from(width))
                .floor()
                .min(f64::from(image.width() - 1)) as u32;
            out.set_pixel(x, y, image.pixel(source_x, source_y));
        }
    }
    out
}

/// Checks a requested canvas size against the same limits the macOS build applies.
///
/// # Errors
///
/// Fails when a side is zero, over [`MAX_SIDE`], or the total is over [`MAX_MEGAPIXELS`].
pub fn check_size(width: u32, height: u32) -> Result<(), DocumentError> {
    if width == 0
        || height == 0
        || width > MAX_SIDE
        || height > MAX_SIDE
        || u64::from(width) * u64::from(height) > MAX_MEGAPIXELS * 1_000_000
    {
        return Err(DocumentError::BadSize { width, height });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document_with_two_layers() -> Document {
        let mut document = Document::new(8, 8);
        document.add_layer(
            "Bottom",
            Arc::new(PremulImage::filled(8, 8, [255, 0, 0, 255])),
            None,
        );
        // Premultiplied, as layer pixels always are: 50% blue.
        document.add_layer(
            "Top",
            Arc::new(PremulImage::filled(8, 8, [0, 0, 128, 128])),
            None,
        );
        document
    }

    #[test]
    fn add_and_nudge_layers() {
        let mut document = document_with_two_layers();
        assert_eq!(document.layers.len(), 2);
        let top = document.layers[1].id;
        assert!(document.nudge_layer(top, -1));
        assert_eq!(document.layers[0].id, top);
        assert!(!document.nudge_layer(top, -1));
    }

    #[test]
    fn duplicate_copies_pixels() {
        let mut document = document_with_two_layers();
        let id = document.layers[1].id;
        let copy = document.duplicate_layer(id).expect("copy");
        assert_eq!(document.layers.len(), 3);
        assert_eq!(document.index_of(copy), Some(2));
        assert!(document.layer(copy).expect("exists").name.contains("copy"));
    }

    #[test]
    fn remove_takes_children_with_it() {
        let mut document = document_with_two_layers();
        let bottom = document.layers[0].id;
        let top = document.layers[1].id;
        let group = document.group_layers(&[top], "Folder").expect("group");
        assert_eq!(document.layer(top).expect("layer").parent, Some(group));
        document.remove_layer(group);
        assert!(document.layer(top).is_none());
        assert_eq!(document.layers.len(), 1);
        assert_eq!(document.layers[0].id, bottom);
    }

    #[test]
    fn merge_down_keeps_the_lower_layer_in_place() {
        let mut document = document_with_two_layers();
        let top = document.layers[1].id;
        document.merge_down(top).expect("merge");
        assert_eq!(document.layers.len(), 1);
        let layer = &document.layers[0];
        let image = layer.image.as_ref().expect("pixels");
        let pixel = image.pixel(0, 0).expect("pixel");
        // Red backdrop with a half-transparent blue layer over it.
        assert_eq!(pixel[0], 127);
        assert_eq!(pixel[2], 128);
        assert_eq!(pixel[3], 255);
    }

    #[test]
    fn merge_down_needs_a_layer_below() {
        let mut document = Document::new(4, 4);
        document.add_layer("Only", Arc::new(PremulImage::new(4, 4)), None);
        let id = document.layers[0].id;
        assert_eq!(
            document.merge_down(id).unwrap_err(),
            DocumentError::NothingBelow
        );
    }

    #[test]
    fn flatten_collapses_everything() {
        let mut document = document_with_two_layers();
        let id = document.flatten();
        assert_eq!(document.layers.len(), 1);
        assert_eq!(document.active_layer, Some(id));
    }

    #[test]
    fn canvas_resize_moves_layers_by_the_anchor() {
        let mut document = document_with_two_layers();
        document.resize_canvas(20, 20, [0.5, 0.5]).expect("resize");
        assert_eq!(document.width, 20);
        assert_eq!(document.layers[0].transform.origin, [6.0, 6.0]);
    }

    #[test]
    fn image_resize_scales_pixels() {
        let mut document = document_with_two_layers();
        document.resize_image(4, 4, true).expect("resize");
        let image = document.layers[0].image.as_ref().expect("pixels");
        assert_eq!((image.width(), image.height()), (4, 4));
    }

    #[test]
    fn size_limits_are_enforced() {
        assert!(check_size(0, 10).is_err());
        assert!(check_size(MAX_SIDE + 1, 10).is_err());
        assert!(check_size(20_000, 20_000).is_err());
        assert!(check_size(1920, 1080).is_ok());
    }
}

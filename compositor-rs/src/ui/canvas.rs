//! The canvas: the picture, the selection overlay, and every pointer interaction that edits.
//!
//! The composited picture lives in one GPU texture that is only re-uploaded when the document
//! changes (`AppState::revision`), so panning and zooming stay smooth even on large documents.

use eframe::egui;

use crate::app::state::{AppState, Tool};
use crate::core::geometry::RectI;
use crate::core::selection::SelectionMode;
use crate::ui::palette;

/// Screen pixels per document pixel that Fit never goes below, so a huge canvas stays visible.
const MIN_FIT_ZOOM: f32 = 0.02;

/// Cached textures for the canvas: the picture and the selection overlay.
#[derive(Default)]
pub struct CanvasCache {
    picture: Option<egui::TextureHandle>,
    picture_revision: u64,
    picture_size: [u32; 2],
    overlay: Option<egui::TextureHandle>,
    overlay_revision: u64,
    overlay_present: bool,
}

impl CanvasCache {
    /// Drops every texture, which is what the tests do between runs.
    pub fn clear(&mut self) {
        self.picture = None;
        self.overlay = None;
        self.picture_revision = u64::MAX;
        self.overlay_revision = u64::MAX;
        self.overlay_present = false;
    }

    /// The picture's texture, rebuilt when the document changed size or content.
    fn picture(
        &mut self,
        ctx: &egui::Context,
        state: &AppState,
        zoom: f32,
    ) -> Option<egui::TextureId> {
        let document = state.document.as_ref()?;
        let size = [document.width, document.height];
        let stale = self.picture_revision != state.revision
            || self.picture_size != size
            || self.picture.is_none();
        if stale {
            let picture = crate::core::composite::composite_document(document);
            let image = egui::ColorImage::from_rgba_premultiplied(
                [picture.width() as usize, picture.height() as usize],
                picture.as_slice(),
            );
            let options = if zoom >= 1.0 {
                egui::TextureOptions::NEAREST
            } else {
                egui::TextureOptions::LINEAR
            };
            match self.picture.as_mut() {
                Some(texture) => texture.set(image, options),
                None => {
                    self.picture = Some(ctx.load_texture("rustphotoshop-picture", image, options))
                }
            }
            self.picture_revision = state.revision;
            self.picture_size = size;
        }
        self.picture.as_ref().map(egui::TextureHandle::id)
    }

    /// A texture tinting the selected pixels, or `None` when nothing is selected.
    fn selection_overlay(
        &mut self,
        ctx: &egui::Context,
        state: &AppState,
    ) -> Option<egui::TextureId> {
        let selection = state.selection.as_ref()?;
        let has_selection = self.overlay_present;
        if !has_selection || self.overlay_revision != state.revision {
            let width = selection.width() as usize;
            let height = selection.height() as usize;
            let mut pixels = vec![egui::Color32::TRANSPARENT; width * height];
            for (index, weight) in selection.coverage().iter().enumerate() {
                if *weight == 0 {
                    continue;
                }
                let alpha = (u32::from(*weight) * 90 / 255) as u8;
                pixels[index] = egui::Color32::from_rgba_premultiplied(60, 70, 130, alpha);
            }
            let image = egui::ColorImage::new([width, height], pixels);
            match self.overlay.as_mut() {
                Some(texture) => texture.set(image, egui::TextureOptions::NEAREST),
                None => {
                    self.overlay = Some(ctx.load_texture(
                        "rustphotoshop-selection",
                        image,
                        egui::TextureOptions::NEAREST,
                    ));
                }
            }
            self.overlay_revision = state.revision;
            self.overlay_present = true;
        }
        self.overlay.as_ref().map(egui::TextureHandle::id)
    }
}

/// How the document maps onto the canvas widget this frame.
#[derive(Clone, Copy, Debug)]
pub struct View {
    /// Screen position of the document's top-left corner.
    pub origin: egui::Pos2,
    /// Screen pixels per document pixel.
    pub zoom: f32,
}

impl View {
    #[must_use]
    pub fn to_screen(&self, point: [f64; 2]) -> egui::Pos2 {
        egui::pos2(
            self.origin.x + point[0] as f32 * self.zoom,
            self.origin.y + point[1] as f32 * self.zoom,
        )
    }

    #[must_use]
    pub fn to_document(&self, point: egui::Pos2) -> [f64; 2] {
        [
            f64::from((point.x - self.origin.x) / self.zoom),
            f64::from((point.y - self.origin.y) / self.zoom),
        ]
    }

    #[must_use]
    pub fn document_rect(&self, width: u32, height: u32) -> egui::Rect {
        egui::Rect::from_min_max(
            self.origin,
            egui::pos2(
                self.origin.x + width as f32 * self.zoom,
                self.origin.y + height as f32 * self.zoom,
            ),
        )
    }
}

/// Draws the canvas and handles everything the pointer does on it.
pub fn draw(state: &mut AppState, cache: &mut CanvasCache, ui: &mut egui::Ui) {
    let available = ui.available_size();
    let (response, painter) = ui.allocate_painter(available, egui::Sense::click_and_drag());
    let rect = response.rect;
    painter.rect_filled(rect, 0.0, palette::BACKDROP);

    let Some((width, height)) = state.canvas_size() else {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "未打开文档\n\n请用「文件 ▸ 新建」或「文件 ▸ 打开」",
            egui::FontId::proportional(18.0),
            egui::Color32::from_gray(180),
        );
        return;
    };

    let view = compute_view(state, rect, width, height);
    paint_picture(state, cache, &painter, rect, view, width, height);
    paint_view_extras(state, &painter, view, width, height);
    handle_pointer(state, &response, view, rect);
}

/// Works out where the document sits this frame, fitting it on the first layout pass.
fn compute_view(state: &mut AppState, rect: egui::Rect, width: u32, height: u32) -> View {
    if state.view.fit_pending && rect.width() > 1.0 && rect.height() > 1.0 {
        let zoom = ((rect.width() - 48.0) / width as f32)
            .min((rect.height() - 48.0) / height as f32)
            .clamp(MIN_FIT_ZOOM, 8.0);
        state.view.zoom = zoom;
        state.view.top_left = [
            (width as f32 - rect.width() / zoom) / 2.0,
            (height as f32 - rect.height() / zoom) / 2.0,
        ];
        state.view.fit_pending = false;
    }
    let zoom = state.view.zoom.max(MIN_FIT_ZOOM);
    View {
        origin: egui::pos2(
            rect.min.x - state.view.top_left[0] * zoom,
            rect.min.y - state.view.top_left[1] * zoom,
        ),
        zoom,
    }
}

/// The checkerboard, the picture and the selection overlay.
fn paint_picture(
    state: &AppState,
    cache: &mut CanvasCache,
    painter: &egui::Painter,
    rect: egui::Rect,
    view: View,
    width: u32,
    height: u32,
) {
    let picture_rect = view.document_rect(width, height);
    let clipped = painter.with_clip_rect(rect);
    paint_checkerboard(&clipped, picture_rect.intersect(rect));
    if let Some(texture) = cache.picture(painter.ctx(), state, view.zoom) {
        clipped.image(
            texture,
            picture_rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    }
    if let Some(texture) = cache.selection_overlay(painter.ctx(), state) {
        clipped.image(
            texture,
            picture_rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    }
    clipped.rect_stroke(
        picture_rect,
        0.0,
        egui::Stroke::new(1.0, egui::Color32::from_gray(90)),
        egui::StrokeKind::Outside,
    );
}

/// The gray squares that show through transparent pixels.
fn paint_checkerboard(painter: &egui::Painter, rect: egui::Rect) {
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    painter.rect_filled(rect, 0.0, palette::CHECKER_LIGHT);
    const SQUARE: f32 = 8.0;
    let columns = (rect.width() / SQUARE).ceil() as i32;
    let rows = (rect.height() / SQUARE).ceil() as i32;
    for row in 0..rows {
        for column in 0..columns {
            if (row + column) % 2 == 0 {
                continue;
            }
            let min = egui::pos2(
                rect.min.x + column as f32 * SQUARE,
                rect.min.y + row as f32 * SQUARE,
            );
            let square = egui::Rect::from_min_size(min, egui::vec2(SQUARE, SQUARE));
            painter.rect_filled(square.intersect(rect), 0.0, palette::CHECKER_DARK);
        }
    }
}

/// The drag previews and the crop shading.
fn paint_view_extras(
    state: &AppState,
    painter: &egui::Painter,
    view: View,
    width: u32,
    height: u32,
) {
    // A crop drag dims everything it would remove.
    if let (Some(rect), Tool::Crop) = (state.drag_shape, state.tool) {
        let selection = egui::Rect::from_min_max(
            view.to_screen([rect.x0 as f64, rect.y0 as f64]),
            view.to_screen([rect.x1 as f64, rect.y1 as f64]),
        );
        let shading = egui::Color32::from_black_alpha(110);
        let picture = view.document_rect(width, height);
        painter.rect_filled(
            egui::Rect::from_min_max(picture.min, egui::pos2(picture.max.x, selection.min.y)),
            0.0,
            shading,
        );
        painter.rect_filled(
            egui::Rect::from_min_max(egui::pos2(picture.min.x, selection.max.y), picture.max),
            0.0,
            shading,
        );
        painter.rect_stroke(
            selection,
            0.0,
            egui::Stroke::new(1.0, palette::SELECTION_EDGE),
            egui::StrokeKind::Outside,
        );
    }
    if let (Some(rect), true) = (state.drag_shape, state.tool.is_marquee()) {
        let drag = egui::Rect::from_min_max(
            view.to_screen([rect.x0 as f64, rect.y0 as f64]),
            view.to_screen([rect.x1 as f64, rect.y1 as f64]),
        );
        painter.rect_filled(drag, 0.0, palette::SELECTION_FILL);
        painter.rect_stroke(
            drag,
            0.0,
            egui::Stroke::new(1.0, palette::SELECTION_EDGE),
            egui::StrokeKind::Outside,
        );
    }
    // A brush cursor: a circle the size the brush will paint.
    if let (Some(cursor), true) = (state.cursor, state.tool.is_brush()) {
        let center = view.to_screen(cursor);
        let radius = (state.brush.size as f32 / 2.0) * view.zoom;
        if radius >= 2.0 {
            painter.circle_stroke(
                center,
                radius,
                egui::Stroke::new(1.0, egui::Color32::from_white_alpha(200)),
            );
        } else {
            painter.circle_stroke(
                center,
                4.0,
                egui::Stroke::new(1.0, egui::Color32::from_black_alpha(200)),
            );
        }
    }
    if let (Some(cursor), Tool::Heal) = (state.cursor, state.tool) {
        let center = view.to_screen(cursor);
        painter.circle_stroke(
            center,
            (state.heal_radius as f32) * view.zoom,
            egui::Stroke::new(1.0, egui::Color32::from_white_alpha(200)),
        );
    }
}

/// Everything the pointer does: painting, selecting, cropping, panning, zooming and dropping.
fn handle_pointer(state: &mut AppState, response: &egui::Response, view: View, rect: egui::Rect) {
    let ctx = response.ctx.clone();
    state.cursor = response
        .hover_pos()
        .map(|position| view.to_document(position));
    let (scroll, zoom_delta, shift, alt, command, space) = ctx.input(|input| {
        (
            input.smooth_scroll_delta(),
            input.zoom_delta(),
            input.modifiers.shift,
            input.modifiers.alt,
            input.modifiers.command,
            input.key_down(egui::Key::Space),
        )
    });

    // Zooming keeps the point under the pointer still, which is what every editor does.
    let zoom_factor = if (zoom_delta - 1.0).abs() > 1e-3 {
        zoom_delta
    } else if command && scroll.y.abs() > 0.1 {
        1.0 + scroll.y * 0.002
    } else {
        1.0
    };
    if (zoom_factor - 1.0).abs() > 1e-4 {
        if let Some(pointer) = response.hover_pos() {
            let before = view.to_document(pointer);
            state.view.zoom = (state.view.zoom * zoom_factor).clamp(0.02, 32.0);
            let zoom = state.view.zoom;
            state.view.top_left = [
                before[0] as f32 - (pointer.x - rect.min.x) / zoom,
                before[1] as f32 - (pointer.y - rect.min.y) / zoom,
            ];
            state.view.fit_pending = false;
        }
    } else if !command && scroll != egui::Vec2::ZERO {
        // A plain scroll pans, as it does in the original.
        state.view.top_left[0] -= scroll.x / state.view.zoom;
        state.view.top_left[1] -= scroll.y / state.view.zoom;
    }

    let panning =
        space || state.tool == Tool::Hand || response.dragged_by(egui::PointerButton::Middle);
    if panning {
        let delta = response.drag_delta();
        if delta != egui::Vec2::ZERO {
            state.view.top_left[0] -= delta.x / state.view.zoom;
            state.view.top_left[1] -= delta.y / state.view.zoom;
            state.view.fit_pending = false;
            return;
        }
    }

    let pressed = response.drag_started_by(egui::PointerButton::Primary) || response.clicked();
    let dragging = response.dragged_by(egui::PointerButton::Primary);
    let released = response.drag_stopped_by(egui::PointerButton::Primary);
    let Some(point) = state.cursor else {
        return;
    };

    match state.tool {
        Tool::Brush | Tool::Eraser => {
            if pressed {
                state.begin_stroke(point);
            } else if dragging {
                state.continue_stroke(point);
            }
            if released {
                state.end_stroke();
            }
        }
        Tool::Eyedropper => {
            if pressed {
                state.pick_color(point);
            }
        }
        Tool::Heal => {
            if pressed {
                state.begin_stroke(point);
            }
        }
        Tool::Wand => {
            if pressed {
                state.wand_select(point, selection_mode(shift, alt));
            }
        }
        Tool::MarqueeRect | Tool::MarqueeEllipse => {
            if pressed {
                state.drag_shape = Some(RectI::from_size(
                    point[0].floor() as i64,
                    point[1].floor() as i64,
                    0,
                    0,
                ));
            } else if dragging && let Some(start) = state.drag_shape {
                let current = RectI::from_size(
                    start.x0,
                    start.y0,
                    (point[0].floor() as i64 - start.x0).unsigned_abs() as u32,
                    (point[1].floor() as i64 - start.y0).unsigned_abs() as u32,
                );
                state.drag_shape = Some(RectI::new(
                    start.x0.min(point[0].floor() as i64),
                    start.y0.min(point[1].floor() as i64),
                    start.x0.max(point[0].floor() as i64),
                    start.y0.max(point[1].floor() as i64),
                ));
                let _ = current;
            }
            if released && let Some(shape) = state.drag_shape.take() {
                state.marquee(
                    shape,
                    state.tool == Tool::MarqueeEllipse,
                    selection_mode(shift, alt),
                );
            }
        }
        Tool::Crop => {
            if pressed {
                state.drag_shape = Some(RectI::new(
                    point[0].floor() as i64,
                    point[1].floor() as i64,
                    point[0].floor() as i64,
                    point[1].floor() as i64,
                ));
            } else if dragging && let Some(start) = state.drag_shape {
                let mut x1 = point[0].floor() as i64;
                let mut y1 = point[1].floor() as i64;
                if shift {
                    // A square crop, measured from the corner that is being dragged.
                    let side = (x1 - start.x0).abs().min((y1 - start.y0).abs());
                    x1 = start.x0 + side * (x1 - start.x0).signum();
                    y1 = start.y0 + side * (y1 - start.y0).signum();
                }
                state.drag_shape = Some(RectI::new(
                    start.x0.min(x1),
                    start.y0.min(y1),
                    start.x0.max(x1),
                    start.y0.max(y1),
                ));
            }
            if released
                && let Some(shape) = state.drag_shape.take()
                && shape.width() >= 1
                && shape.height() >= 1
            {
                state.crop_to(shape);
            }
        }
        Tool::Move => {
            if response.drag_started_by(egui::PointerButton::Primary) {
                state.record("Move");
            }
            if dragging {
                let delta = response.drag_delta();
                if delta != egui::Vec2::ZERO {
                    let dx = f64::from(delta.x) / f64::from(state.view.zoom);
                    let dy = f64::from(delta.y) / f64::from(state.view.zoom);
                    let layer = state.active_layer_id();
                    if let (Some(layer), Some(document)) = (layer, state.document.as_mut())
                        && let Some(layer) = document.layer_mut(layer)
                    {
                        layer.transform.origin[0] += dx;
                        layer.transform.origin[1] += dy;
                        document.dirty = true;
                    }
                    state.touch();
                }
            }
        }
        Tool::Zoom => {
            if pressed {
                if alt {
                    state.view.zoom = (state.view.zoom / 1.25).max(0.02);
                } else {
                    state.view.zoom = (state.view.zoom * 1.25).min(32.0);
                }
                state.view.fit_pending = false;
            }
        }
        Tool::Hand => {}
    }
}

/// The modifier keys choose how a new shape joins the selection, as in every editor.
#[must_use]
pub fn selection_mode(shift: bool, alt: bool) -> SelectionMode {
    match (shift, alt) {
        (true, true) => SelectionMode::Intersect,
        (true, false) => SelectionMode::Add,
        (false, true) => SelectionMode::Subtract,
        (false, false) => SelectionMode::Replace,
    }
}

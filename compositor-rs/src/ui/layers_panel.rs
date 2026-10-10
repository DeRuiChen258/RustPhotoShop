//! The layers panel: the stack, plus the active layer's appearance.

use eframe::egui;
use uuid::Uuid;

use crate::app::state::{AdjustTarget, AppState, Dialog};
use crate::core::blend::BlendMode;
use crate::core::document::LayerKind;

/// Draws the panel down the right-hand side of the window.
pub fn draw(state: &mut AppState, ui: &mut egui::Ui) {
    egui::Panel::right("rustphotoshop-layers")
        .default_size(280.0)
        .min_size(220.0)
        .show(ui, |ui| {
            appearance(state, ui);
            ui.separator();
            actions(state, ui);
            ui.separator();
            list(state, ui);
        });
}

/// Blend mode, opacity and the mask controls for the active layer.
fn appearance(state: &mut AppState, ui: &mut egui::Ui) {
    let Some(active) = state.active_layer_id() else {
        ui.label("未选择图层");
        return;
    };
    let Some(layer) = state
        .document
        .as_ref()
        .and_then(|document| document.layer(active))
    else {
        return;
    };
    let mut blend = layer.blend;
    let mut opacity = layer.opacity;
    let visible = layer.visible;
    let mut mask_enabled = layer.mask_enabled;
    let has_mask = layer.mask.is_some();
    let kind = layer.kind;

    ui.horizontal(|ui| {
        ui.label("混合模式");
        egui::ComboBox::from_id_salt("blend-mode")
            .selected_text(blend.label())
            .width(150.0)
            .show_ui(ui, |ui| {
                for group in BlendMode::GROUPS {
                    for mode in *group {
                        ui.selectable_value(&mut blend, *mode, mode.label());
                    }
                    ui.separator();
                }
            });
    });
    if blend
        != state
            .active_layer()
            .map(|layer| layer.blend)
            .unwrap_or(blend)
    {
        state.set_layer_blend(active, blend);
    }

    ui.horizontal(|ui| {
        ui.label("不透明度");
        let changed = ui
            .add(egui::Slider::new(&mut opacity, 0.0..=1.0).fixed_decimals(2))
            .changed();
        if changed {
            state.set_layer_opacity(active, opacity);
        }
    });

    ui.horizontal(|ui| {
        let mut shown = visible;
        if ui.checkbox(&mut shown, "显示").changed() {
            state.set_layer_visible(active, shown);
        }
        if kind == LayerKind::Adjustment
            && ui.button("设置…").clicked()
            && let Some(adjustment) = state
                .document
                .as_ref()
                .and_then(|document| document.layer(active))
                .and_then(|layer| layer.adjustment.clone())
        {
            state.open_adjustment_dialog(adjustment);
        }
    });

    if has_mask {
        ui.horizontal(|ui| {
            if ui.checkbox(&mut mask_enabled, "蒙版").changed() {
                state.toggle_mask_enabled();
            }
            if ui.button("反相").clicked() {
                state.invert_mask();
            }
            if ui.button("应用").clicked() {
                state.apply_mask();
            }
            if ui.button("删除").clicked() {
                state.remove_mask();
            }
        });
    } else {
        ui.horizontal(|ui| {
            if ui.button("添加蒙版").clicked() {
                state.add_mask();
            }
        });
    }
}

/// The buttons that change the stack.
fn actions(state: &mut AppState, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        let has_layer = state.active_layer().is_some();
        if ui.button("＋").on_hover_text("新建图层").clicked() {
            state.add_layer();
        }
        if ui
            .add_enabled(has_layer, egui::Button::new("⧉"))
            .on_hover_text("复制图层")
            .clicked()
        {
            state.duplicate_layer();
        }
        if ui
            .add_enabled(has_layer, egui::Button::new("🗑"))
            .on_hover_text("删除图层")
            .clicked()
        {
            state.delete_layer();
        }
        if ui
            .add_enabled(has_layer, egui::Button::new("▾"))
            .on_hover_text("下移一层")
            .clicked()
            && let Some(id) = state.active_layer_id()
        {
            state.nudge_layer(id, -1);
        }
        if ui
            .add_enabled(has_layer, egui::Button::new("▴"))
            .on_hover_text("上移一层")
            .clicked()
            && let Some(id) = state.active_layer_id()
        {
            state.nudge_layer(id, 1);
        }
        if ui
            .add_enabled(has_layer, egui::Button::new("合并"))
            .on_hover_text("向下合并")
            .clicked()
        {
            state.merge_down();
        }
        if ui
            .add_enabled(has_layer, egui::Button::new("拼合"))
            .on_hover_text("拼合图像")
            .clicked()
        {
            state.flatten();
        }
    });
}

/// The stack itself, top layer first, the way every layer panel lists them.
fn list(state: &mut AppState, ui: &mut egui::Ui) {
    let Some(document) = state.document.as_ref() else {
        return;
    };
    // Depth-first from the roots, then reversed so the topmost layer is at the top.
    let mut rows: Vec<(Uuid, usize)> = Vec::new();
    fn walk(
        document: &crate::core::document::Document,
        parent: Option<Uuid>,
        depth: usize,
        rows: &mut Vec<(Uuid, usize)>,
    ) {
        for id in document.children(parent) {
            rows.push((id, depth));
            if document.is_group(id) {
                walk(document, Some(id), depth + 1, rows);
            }
        }
    }
    walk(document, None, 0, &mut rows);
    rows.reverse();

    let active = state.active_layer_id();
    let mut rename: Option<(Uuid, String)> = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (id, depth) in rows {
                let Some(layer) = state
                    .document
                    .as_ref()
                    .and_then(|document| document.layer(id))
                else {
                    continue;
                };
                let name = layer.name.clone();
                let visible = layer.visible;
                let is_group = layer.kind == LayerKind::Group;
                let kind = layer.kind;
                let opacity = layer.opacity;
                let blend = layer.blend;
                let has_mask = layer.mask.is_some();
                let swatch = layer_swatch(layer);

                let selected = active == Some(id);
                ui.horizontal(|ui| {
                    ui.add_space(depth as f32 * 12.0);
                    let mut shown = visible;
                    if ui
                        .add(egui::Button::new(if shown { "👁" } else { "─" }).frame(false))
                        .clicked()
                    {
                        shown = !shown;
                        state.set_layer_visible(id, shown);
                    }
                    let (rect, _response) =
                        ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 2.0, swatch);
                    let label = if is_group {
                        format!("📁 {name}")
                    } else if kind == LayerKind::Adjustment {
                        format!("⚙ {name}")
                    } else {
                        name.clone()
                    };
                    let response = ui.selectable_label(selected, label);
                    if response.clicked() {
                        state.select_layer(id);
                    }
                    if response.double_clicked() {
                        state.select_layer(id);
                        if kind == LayerKind::Adjustment {
                            if let Some(adjustment) = state
                                .document
                                .as_ref()
                                .and_then(|document| document.layer(id))
                                .and_then(|layer| layer.adjustment.clone())
                            {
                                state.open_adjustment_dialog(adjustment);
                            }
                        } else {
                            rename = Some((id, name.clone()));
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if has_mask {
                            ui.label("⧉mask");
                        }
                        if opacity < 0.999 {
                            ui.label(format!("{:.0}%", opacity * 100.0));
                        }
                        if blend != BlendMode::Normal {
                            ui.label(blend.label());
                        }
                    });
                });
            }
        });

    if let Some((id, current)) = rename {
        state.dialog = Some(Dialog::Message {
            title: "重命名图层".to_owned(),
            detail: format!("{id}\n{current}"),
            is_error: false,
        });
    }
    let _ = AdjustTarget::Pixels;
}

/// A layer's thumbnail stand-in: the color of a few sampled pixels.
fn layer_swatch(layer: &crate::core::document::Layer) -> egui::Color32 {
    let Some(image) = &layer.image else {
        return egui::Color32::from_gray(70);
    };
    if image.is_empty() {
        return egui::Color32::from_gray(70);
    }
    let mut sums = [0_u32; 4];
    let mut count = 0_u32;
    for y in 0..4 {
        for x in 0..4 {
            let sample_x = (x * image.width() / 4).min(image.width() - 1);
            let sample_y = (y * image.height() / 4).min(image.height() - 1);
            if let Some(pixel) = image.pixel(sample_x, sample_y) {
                for channel in 0..4 {
                    sums[channel] += u32::from(pixel[channel]);
                }
                count += 1;
            }
        }
    }
    if count == 0 {
        return egui::Color32::from_gray(70);
    }
    egui::Color32::from_rgba_premultiplied(
        (sums[0] / count) as u8,
        (sums[1] / count) as u8,
        (sums[2] / count) as u8,
        255,
    )
}

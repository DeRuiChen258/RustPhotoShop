//! The tool strip: the tools on the left, the active tool's settings beside them.

use eframe::egui;

use crate::app::state::{AppState, Tool};
use crate::core::ops::PaintTarget;

/// Draws the tool strip and the tool options.
pub fn draw(state: &mut AppState, ui: &mut egui::Ui) {
    egui::Panel::top("rustphotoshop-tools").show(ui, |ui| {
        ui.horizontal(|ui| {
            tools(state, ui);
            ui.separator();
            options(state, ui);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                colors(state, ui);
            });
        });
    });
}

fn tools(state: &mut AppState, ui: &mut egui::Ui) {
    for tool in Tool::ALL {
        let selected = state.tool == *tool;
        let label = tool.icon().to_string();
        let button = egui::Button::new(label).selected(selected);
        let response = ui.add(button).on_hover_text(format!(
            "{}（{}）",
            tool.label(),
            tool.shortcut().to_uppercase()
        ));
        if response.clicked() {
            state.tool = *tool;
            state.say(tool.label());
        }
    }
}

fn options(state: &mut AppState, ui: &mut egui::Ui) {
    match state.tool {
        Tool::Brush | Tool::Eraser => {
            ui.label("大小");
            ui.add(
                egui::Slider::new(&mut state.brush.size, 1.0..=500.0)
                    .suffix(" 像素")
                    .logarithmic(true),
            );
            ui.label("硬度");
            ui.add(egui::Slider::new(&mut state.brush.hardness, 0.0..=1.0).fixed_decimals(2));
            ui.label("不透明度");
            ui.add(egui::Slider::new(&mut state.brush.opacity, 0.0..=1.0).fixed_decimals(2));
            let has_mask = state
                .active_layer()
                .is_some_and(|layer| layer.mask.is_some());
            if has_mask {
                let mut on_mask = state.brush.target == PaintTarget::Mask;
                if ui.checkbox(&mut on_mask, "在蒙版上绘制").changed() {
                    state.brush.target = if on_mask {
                        PaintTarget::Mask
                    } else {
                        PaintTarget::Pixels
                    };
                }
            }
        }
        Tool::Heal => {
            ui.label("半径");
            ui.add(
                egui::Slider::new(&mut state.heal_radius, 2.0..=200.0)
                    .suffix(" 像素")
                    .logarithmic(true),
            );
            ui.label("内容感知");
        }
        Tool::Wand => {
            ui.label("容差");
            ui.add(egui::Slider::new(&mut state.wand.tolerance, 0..=255));
            ui.checkbox(&mut state.wand.contiguous, "连续区域");
        }
        Tool::MarqueeRect | Tool::MarqueeEllipse => {
            ui.label("拖拽创建选区；Shift 相加，Alt 相减，Shift+Alt 相交");
        }
        Tool::Crop => {
            ui.label("拖拽裁剪框；按住 Shift 保持正方形");
            let has_selection = state.selection.is_some();
            if ui
                .add_enabled(has_selection, egui::Button::new("裁剪到选区"))
                .clicked()
                && let Some(bounds) = state
                    .selection
                    .as_ref()
                    .and_then(|selection| selection.bounds())
            {
                state.crop_to(bounds);
            }
        }
        Tool::Move => {
            if let Some(layer) = state.active_layer() {
                let origin = layer.transform.origin;
                ui.label(format!("位置：{:.0}, {:.0}", origin[0], origin[1]));
            }
        }
        Tool::Eyedropper => {
            ui.label("点击画面拾取颜色");
        }
        Tool::Zoom => {
            ui.label("点击放大，Alt+点击缩小");
            if ui.button("适合屏幕").clicked() {
                state.zoom_to_fit();
            }
        }
        Tool::Hand => {
            ui.label("拖拽平移画布");
        }
    }
}

/// The foreground and background colors, as the two overlapping swatches every editor has.
fn colors(state: &mut AppState, ui: &mut egui::Ui) {
    let mut foreground = to_color32(state.foreground);
    let mut background = to_color32(state.background);
    if ui
        .color_edit_button_srgba(&mut foreground)
        .on_hover_text("前景色")
        .changed()
    {
        state.foreground = from_color32(foreground);
    }
    if ui
        .color_edit_button_srgba(&mut background)
        .on_hover_text("背景色")
        .changed()
    {
        state.background = from_color32(background);
    }
    if ui.button("⇄").on_hover_text("交换前景/背景色").clicked() {
        std::mem::swap(&mut state.foreground, &mut state.background);
    }
    if ui.button("⬛⬜").on_hover_text("恢复默认黑/白").clicked() {
        state.foreground = [0, 0, 0, 255];
        state.background = [255, 255, 255, 255];
    }
}

/// The document's straight RGBA color as the widget's color type.
fn to_color32(color: [u8; 4]) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(color[0], color[1], color[2], color[3])
}

/// Back again, keeping the straight (non-premultiplied) form the tools expect.
fn from_color32(color: egui::Color32) -> [u8; 4] {
    let [red, green, blue, alpha] = color.to_srgba_unmultiplied();
    [red, green, blue, alpha]
}

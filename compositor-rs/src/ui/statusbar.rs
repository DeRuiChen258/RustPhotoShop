//! The status bar: where the view is, what is under the pointer, and what just happened.

use eframe::egui;

use crate::app::state::AppState;

/// Draws the status bar along the bottom of the window.
pub fn draw(state: &mut AppState, ui: &mut egui::Ui) {
    egui::Panel::bottom("rustphotoshop-status").show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(format!("{:.0}%", state.view.zoom * 100.0));
            ui.separator();
            match state.canvas_size() {
                Some((width, height)) => {
                    let resolution = state
                        .document
                        .as_ref()
                        .map_or(72.0, |document| document.resolution);
                    ui.label(format!("{width} × {height} 像素 @ {resolution:.0} ppi"));
                }
                None => {
                    ui.label("未打开文档");
                }
            }
            ui.separator();
            match state.cursor {
                Some(point) => ui.label(format!("X {:.0}  Y {:.0}", point[0], point[1])),
                None => ui.label("—"),
            };
            if let Some(bounds) = state
                .selection
                .as_ref()
                .and_then(|selection| selection.bounds())
            {
                ui.separator();
                ui.label(format!(
                    "选区 {} × {}，位置 {}, {}",
                    bounds.width(),
                    bounds.height(),
                    bounds.x0,
                    bounds.y0
                ));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(&state.status);
            });
        });
    });
}

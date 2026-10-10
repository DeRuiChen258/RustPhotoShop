//! The interface: the menu bar, the tool strip, the canvas, the layer panel and the dialogs.
//!
//! Everything is drawn from [`AppState`], and every interaction goes back through its methods,
//! so the interface holds no state of its own beyond the canvas texture cache.

pub mod canvas;
pub mod dialogs;
pub mod fonts;
pub mod layers_panel;
pub mod menu;
pub mod statusbar;
pub mod toolbar;

use eframe::egui;

use crate::app::state::AppState;

pub use canvas::CanvasCache;

/// Colours shared by the panels, so the interface looks like one thing.
pub mod palette {
    use eframe::egui::Color32;

    /// The canvas backdrop: a mid gray Photoshop also uses, so colors read true against it.
    pub const BACKDROP: Color32 = Color32::from_rgb(56, 56, 58);
    /// The checkerboard's two squares, showing through transparent pixels.
    pub const CHECKER_LIGHT: Color32 = Color32::from_rgb(150, 150, 150);
    pub const CHECKER_DARK: Color32 = Color32::from_rgb(120, 120, 120);
    /// The selection tint and its marching edge.
    pub const SELECTION_FILL: Color32 = Color32::from_rgba_premultiplied(60, 70, 130, 90);
    pub const SELECTION_EDGE: Color32 = Color32::from_rgb(240, 240, 250);
    /// A panel's own background.
    pub const PANEL: Color32 = Color32::from_rgb(38, 38, 40);
}

/// Draws one frame of the whole application.
///
/// Takes a plain `Ui` rather than an `eframe` frame so the tests can drive a whole frame without
/// a window: see `tests/ui_headless.rs`.
pub fn draw(state: &mut AppState, cache: &mut CanvasCache, ui: &mut egui::Ui) {
    handle_shortcuts(state, ui);
    handle_dropped_files(state, ui);
    menu::draw(state, ui);
    toolbar::draw(state, ui);
    statusbar::draw(state, ui);
    layers_panel::draw(state, ui);
    egui::CentralPanel::default().show(ui, |ui| {
        canvas::draw(state, cache, ui);
    });
    dialogs::draw(state, ui.ctx());
    // An open adjustment dialog re-applies its settings so the canvas shows the result.
    dialogs::preview_open_dialog(state);
}

/// Keyboard shortcuts, in the arrangement the original application uses.
fn handle_shortcuts(state: &mut AppState, ui: &mut egui::Ui) {
    // A text field being typed in (a layer name, a dialog value) owns the keyboard.
    if ui.ctx().egui_wants_keyboard_input() {
        return;
    }
    const WATCHED: &[egui::Key] = &[
        egui::Key::N,
        egui::Key::O,
        egui::Key::S,
        egui::Key::E,
        egui::Key::Z,
        egui::Key::Y,
        egui::Key::A,
        egui::Key::D,
        egui::Key::I,
        egui::Key::J,
        egui::Key::F,
        egui::Key::B,
        egui::Key::E,
        egui::Key::M,
        egui::Key::W,
        egui::Key::C,
        egui::Key::V,
        egui::Key::H,
        egui::Key::Z,
        egui::Key::Num0,
        egui::Key::Num1,
        egui::Key::Plus,
        egui::Key::Equals,
        egui::Key::Minus,
        egui::Key::OpenBracket,
        egui::Key::CloseBracket,
        egui::Key::Delete,
        egui::Key::Backspace,
        egui::Key::Escape,
    ];
    let (command, shift, alt, pressed) = ui.ctx().input(|input| {
        let pressed: Vec<egui::Key> = WATCHED
            .iter()
            .filter(|key| input.key_pressed(**key))
            .copied()
            .collect();
        (
            input.modifiers.command,
            input.modifiers.shift,
            input.modifiers.alt,
            pressed,
        )
    });
    if pressed.is_empty() {
        return;
    }
    for key in pressed {
        use crate::app::state::{Background, Dialog, ExportFormat};
        match (command, shift, key) {
            (true, false, egui::Key::N) => {
                state.dialog = Some(Dialog::NewDocument {
                    width: 1920,
                    height: 1080,
                    background: Background::White,
                });
            }
            (true, false, egui::Key::O) => dialogs::open_file_dialog(state),
            (true, false, egui::Key::S) => dialogs::save_project(state, false),
            (true, true, egui::Key::S) => dialogs::save_project(state, true),
            (true, false, egui::Key::E) => state.merge_down(),
            (true, true, egui::Key::E) => {
                state.dialog = Some(Dialog::Export {
                    path: String::new(),
                    format: ExportFormat::Png,
                    quality: 90,
                });
            }
            (true, false, egui::Key::Z) | (true, false, egui::Key::Y) => state.undo(),
            (true, true, egui::Key::Z) => state.redo(),
            (true, false, egui::Key::A) => state.select_all(),
            (true, false, egui::Key::D) => state.deselect(),
            (true, true, egui::Key::I) => state.invert_selection(),
            (true, false, egui::Key::J) => state.duplicate_layer(),
            (true, true, egui::Key::N) => state.add_layer(),
            (true, false, egui::Key::Num0) => state.zoom_to_fit(),
            (true, false, egui::Key::Num1) => state.zoom_actual(),
            (true, _, egui::Key::Plus) | (true, _, egui::Key::Equals) => state.zoom_in(),
            (true, _, egui::Key::Minus) => state.zoom_out(),
            (false, false, egui::Key::OpenBracket) => {
                state.brush.size = (state.brush.size * 0.8).max(1.0);
            }
            (false, false, egui::Key::CloseBracket) => {
                state.brush.size = (state.brush.size * 1.25).min(2000.0);
            }
            (false, false, egui::Key::Delete) | (false, false, egui::Key::Backspace) => {
                state.delete_selection();
            }
            (false, false, egui::Key::Escape) => {
                if state.dialog.is_some() {
                    state.cancel_dialog();
                } else if state.selection.is_some() {
                    state.deselect();
                } else {
                    state.drag_shape = None;
                }
            }
            // Tool letters, only without a modifier so they cannot shadow menu shortcuts.
            (false, false, _) | (false, true, _) => {
                if let Some(tool) = crate::app::state::Tool::ALL
                    .iter()
                    .find(|tool| key_name_matches(**tool, key, shift, alt))
                {
                    state.tool = *tool;
                    state.say(tool.label());
                }
            }
            _ => {}
        }
    }
}

/// True when a key selects a tool: `M` cycles between the two marquees.
fn key_name_matches(tool: crate::app::state::Tool, key: egui::Key, shift: bool, alt: bool) -> bool {
    if alt {
        return false;
    }
    let letter = match key {
        egui::Key::B => 'b',
        egui::Key::E => 'e',
        egui::Key::M => 'm',
        egui::Key::W => 'w',
        egui::Key::C => 'c',
        egui::Key::J => 'j',
        egui::Key::I => 'i',
        egui::Key::V => 'v',
        egui::Key::H => 'h',
        egui::Key::Z => 'z',
        _ => return false,
    };
    if letter != tool.shortcut() {
        return false;
    }
    // Shift-M picks the ellipse marquee; plain M the rectangle.
    if letter == 'm' {
        return (shift && tool == crate::app::state::Tool::MarqueeEllipse)
            || (!shift && tool == crate::app::state::Tool::MarqueeRect);
    }
    true
}

/// Files dropped onto the window become layers, which is how the macOS build behaves.
fn handle_dropped_files(state: &mut AppState, ui: &mut egui::Ui) {
    let dropped: Vec<std::path::PathBuf> = ui.ctx().input(|input| {
        input
            .raw
            .dropped_files
            .iter()
            .map(|file| file.path().to_path_buf())
            .collect()
    });
    for (index, path) in dropped.into_iter().enumerate() {
        log::info!("dropped file: {}", path.display());
        let result = if index == 0 && !state.has_document() {
            state.open(&path)
        } else {
            state.import_as_layer(&path)
        };
        if let Err(error) = result {
            state.report(Err(error));
        }
    }
}

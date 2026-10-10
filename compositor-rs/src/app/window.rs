//! The window: how the application starts, what it is called, and how it closes.

use std::path::PathBuf;

use eframe::egui;

use crate::app::state::AppState;
use crate::platform::desktop::APP_ID;

/// The window's minimum size, below which the panels stop being usable.
const MIN_SIZE: [f32; 2] = [900.0, 620.0];
/// The size a first launch opens at.
const DEFAULT_SIZE: [f32; 2] = [1440.0, 900.0];

/// The egui application: it owns the state and hands every frame to the interface module.
pub struct RustPhotoShopApp {
    pub state: AppState,
    cache: crate::ui::CanvasCache,
    files_to_open: Vec<PathBuf>,
    opened_startup_files: bool,
}

impl RustPhotoShopApp {
    /// Wraps a state, opening the files the command line named on the first frame.
    #[must_use]
    pub fn new(state: AppState, files: Vec<PathBuf>) -> Self {
        Self {
            state,
            cache: crate::ui::CanvasCache::default(),
            files_to_open: files,
            opened_startup_files: false,
        }
    }

    /// Opens the files named on the command line, reporting a failure in the status bar rather
    /// than refusing to start: a broken recent file must not cost the window.
    fn open_startup_files(&mut self) {
        if self.opened_startup_files {
            return;
        }
        self.opened_startup_files = true;
        for (index, path) in self.files_to_open.clone().into_iter().enumerate() {
            let result = if index == 0 {
                self.state.open(&path)
            } else {
                self.state.import_as_layer(&path)
            };
            if let Err(error) = result {
                log::error!("could not open {}: {error}", path.display());
                self.state.say(format!("{}：{error}", path.display()));
            }
        }
    }

    /// Draws a frame in smoke-test mode and closes the window when the frames are up.
    fn run_smoke_test(&mut self, ctx: &egui::Context) {
        let Some(smoke) = self.state.smoke.as_mut() else {
            return;
        };
        smoke.drawn += 1;
        smoke.frames_left = smoke.frames_left.saturating_sub(1);
        if smoke.frames_left > 0 {
            ctx.request_repaint();
            return;
        }
        let drawn = smoke.drawn;
        let report = smoke.report.clone();
        let has_document = self.state.has_document();
        let layer_count = self
            .state
            .document
            .as_ref()
            .map_or(0, |document| document.layers.len());
        if let Some(path) = report {
            let line = format!(
                "smoke test ok: {drawn} frames drawn, document={}, layers={layer_count}\n",
                if has_document { "open" } else { "none" }
            );
            match std::fs::write(&path, line) {
                Ok(()) => log::info!("wrote the smoke test report to {}", path.display()),
                Err(error) => log::error!("could not write {}: {error}", path.display()),
            }
        }
        log::info!("smoke test finished after {drawn} frames");
        // Reported once: the next pass would otherwise write the file again.
        self.state.smoke = None;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

impl eframe::App for RustPhotoShopApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.open_startup_files();
        crate::ui::draw(&mut self.state, &mut self.cache, ui);
        self.run_smoke_test(ui.ctx());
    }

    /// Keeps the window title in step with the document's name and its unsaved state.
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let _ = frame;
        let title = match self.state.document.as_ref() {
            Some(document) => {
                let name = document
                    .path
                    .as_ref()
                    .and_then(|path| path.file_name())
                    .map_or_else(
                        || "未命名".to_owned(),
                        |name| name.to_string_lossy().to_string(),
                    );
                let marker = if document.dirty { "•" } else { "" };
                format!("{marker}{name} — {}", crate::APP_NAME)
            }
            None => crate::APP_NAME.to_owned(),
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
    }
}

/// Starts the window and runs until it closes.
///
/// # Errors
///
/// Fails when the graphics context cannot be created, which is what happens on a machine with
/// no display at all.
pub fn run_gui(state: AppState, files: Vec<PathBuf>) -> Result<i32, String> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(crate::APP_NAME)
            .with_app_id(APP_ID)
            .with_inner_size(DEFAULT_SIZE)
            .with_min_inner_size(MIN_SIZE)
            .with_icon(load_window_icon()),
        ..Default::default()
    };
    log::info!("opening the window ({} startup file(s))", files.len());
    eframe::run_native(
        crate::APP_NAME,
        options,
        Box::new(move |cc| {
            // Before the first frame: egui builds its font atlas from these definitions.
            crate::ui::fonts::install(&cc.egui_ctx);
            Ok(Box::new(RustPhotoShopApp::new(state, files)))
        }),
    )
    .map(|()| 0)
    .map_err(|error| format!("窗口无法打开：{error}"))
}

/// The taskbar and titlebar icon, taken from the installed icon set with a built-in fallback so
/// a window started from a bare executable still shows the right picture.
fn load_window_icon() -> egui::IconData {
    let from_disk =
        crate::platform::paths::find_asset("icons/hicolor/256x256/apps/rustphotoshop.png")
            .and_then(|path| crate::io::image_io::load(&path).ok());
    let image = from_disk.or_else(|| {
        crate::io::image_io::decode(
            crate::tests::EMBEDDED_ICON,
            std::path::Path::new("embedded icon"),
        )
        .ok()
    });
    match image {
        Some(image) => egui::IconData {
            // egui wants premultiplied RGBA, which is how this crate stores images already.
            rgba: image.as_slice().to_vec(),
            width: image.width(),
            height: image.height(),
        },
        None => egui::IconData {
            rgba: Vec::new(),
            width: 0,
            height: 0,
        },
    }
}

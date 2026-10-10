//! Application state: the open document, the tools, the view and the dialogs.
//!
//! Everything the interface draws comes from here, and every editing command goes through
//! [`AppState::edit`], which is the one place that records an undo step. Keeping that single
//! path is what makes Undo trustworthy.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use uuid::Uuid;

use crate::core::adjustment::{Adjustment, Filter};
use crate::core::composite::composite_document;
use crate::core::document::{Document, Layer, LayerKind};
use crate::core::geometry::RectI;
use crate::core::history::History;
use crate::core::ops::{self, BrushSettings, HealMode, WandSettings};
use crate::core::pixel::{PremulImage, Rgba};
use crate::core::selection::{Selection, SelectionMode};
use crate::io::{image_io, project, psd};

/// The tools the toolbar offers, in the order Photoshop's toolbox lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Move,
    MarqueeRect,
    MarqueeEllipse,
    Wand,
    Crop,
    Brush,
    Eraser,
    Heal,
    Eyedropper,
    Hand,
    Zoom,
}

impl Tool {
    pub const ALL: &'static [Self] = &[
        Self::Move,
        Self::MarqueeRect,
        Self::MarqueeEllipse,
        Self::Wand,
        Self::Crop,
        Self::Brush,
        Self::Eraser,
        Self::Heal,
        Self::Eyedropper,
        Self::Hand,
        Self::Zoom,
    ];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Move => "Move",
            Self::MarqueeRect => "Rectangle Select",
            Self::MarqueeEllipse => "Ellipse Select",
            Self::Wand => "Magic Wand",
            Self::Crop => "Crop",
            Self::Brush => "Brush",
            Self::Eraser => "Eraser",
            Self::Heal => "Spot Healing Brush",
            Self::Eyedropper => "Eyedropper",
            Self::Hand => "Hand",
            Self::Zoom => "Zoom",
        }
    }

    /// The letter that selects the tool, as in the original application.
    #[must_use]
    pub fn shortcut(self) -> char {
        match self {
            Self::Move => 'v',
            Self::MarqueeRect | Self::MarqueeEllipse => 'm',
            Self::Wand => 'w',
            Self::Crop => 'c',
            Self::Brush => 'b',
            Self::Eraser => 'e',
            Self::Heal => 'j',
            Self::Eyedropper => 'i',
            Self::Hand => 'h',
            Self::Zoom => 'z',
        }
    }

    #[must_use]
    pub fn icon(self) -> &'static str {
        match self {
            Self::Move => "✥",
            Self::MarqueeRect => "▭",
            Self::MarqueeEllipse => "◯",
            Self::Wand => "✦",
            Self::Crop => "⌗",
            Self::Brush => "🖌",
            Self::Eraser => "🩹",
            Self::Heal => "✚",
            Self::Eyedropper => "💧",
            Self::Hand => "✋",
            Self::Zoom => "🔍",
        }
    }

    /// True when the tool paints with the brush settings.
    #[must_use]
    pub fn is_brush(self) -> bool {
        matches!(self, Self::Brush | Self::Eraser)
    }

    /// True when the tool starts a marquee that becomes a selection on release.
    #[must_use]
    pub fn is_marquee(self) -> bool {
        matches!(self, Self::MarqueeRect | Self::MarqueeEllipse)
    }
}

/// What a new document starts with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Background {
    White,
    Transparent,
}

impl Background {
    #[must_use]
    pub fn color(self) -> Option<Rgba> {
        match self {
            Self::White => Some([255, 255, 255, 255]),
            Self::Transparent => None,
        }
    }
}

/// The formats the export dialog offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportFormat {
    Png,
    Jpeg,
    WebP,
    Tiff,
    Bmp,
}

impl ExportFormat {
    pub const ALL: &'static [Self] = &[Self::Png, Self::Jpeg, Self::WebP, Self::Tiff, Self::Bmp];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
            Self::WebP => "WebP",
            Self::Tiff => "TIFF",
            Self::Bmp => "BMP",
        }
    }

    #[must_use]
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::WebP => "webp",
            Self::Tiff => "tif",
            Self::Bmp => "bmp",
        }
    }

    /// JPEG is the only lossy one here, so it is the only one that asks for a quality.
    #[must_use]
    pub fn uses_quality(self) -> bool {
        self == Self::Jpeg
    }

    #[must_use]
    pub fn from_extension(extension: &str) -> Option<Self> {
        match extension.to_lowercase().as_str() {
            "png" => Some(Self::Png),
            "jpg" | "jpeg" => Some(Self::Jpeg),
            "webp" => Some(Self::WebP),
            "tif" | "tiff" => Some(Self::Tiff),
            "bmp" => Some(Self::Bmp),
            _ => None,
        }
    }
}

/// Where the canvas is looking.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewState {
    /// Screen pixels per document pixel.
    pub zoom: f32,
    /// The document point shown at the canvas widget's top-left corner.
    pub top_left: [f32; 2],
    /// Fit the canvas on the next layout pass.
    pub fit_pending: bool,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            top_left: [0.0, 0.0],
            fit_pending: true,
        }
    }
}

/// A dialog waiting for the user.
pub enum Dialog {
    NewDocument {
        width: u32,
        height: u32,
        background: Background,
    },
    ImageSize {
        width: u32,
        height: u32,
        resample: bool,
    },
    CanvasSize {
        width: u32,
        height: u32,
        anchor: [f64; 2],
    },
    Export {
        path: String,
        format: ExportFormat,
        quality: u8,
    },
    /// An adjustment being edited, with the document as it was when the dialog opened so the
    /// canvas can preview the result live.
    Adjust(Box<AdjustDialog>),
    Filter(Box<FilterDialog>),
    About,
    Diagnostics,
    Message {
        title: String,
        detail: String,
        is_error: bool,
    },
}

/// Which pixels an adjustment dialog is changing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdjustTarget {
    /// Straight onto the active layer's pixels (Image ▸ Adjustments).
    Pixels,
    /// The settings of an adjustment layer (double-click it in the layers panel).
    Layer(Uuid),
}

/// A live-previewed adjustment dialog.
pub struct AdjustDialog {
    pub adjustment: Adjustment,
    pub target: AdjustTarget,
    /// The document before the dialog opened, restored before each preview.
    pub baseline: Box<Document>,
    pub needs_preview: bool,
}

/// A live-previewed filter dialog.
pub struct FilterDialog {
    pub filter: Filter,
    pub baseline: Box<Document>,
    pub needs_preview: bool,
}

/// The startup smoke test: draw a few frames, then close and report.
#[derive(Clone, Debug)]
pub struct SmokeState {
    pub frames_left: u32,
    pub drawn: u32,
    pub report: Option<PathBuf>,
}

/// The whole running application.
pub struct AppState {
    pub document: Option<Document>,
    pub history: History,
    pub tool: Tool,
    pub brush: BrushSettings,
    pub wand: WandSettings,
    pub heal_radius: f64,
    /// The paint color, straight sRGB.
    pub foreground: Rgba,
    /// The secondary color, used by Fill and by the swatch's swap button.
    pub background: Rgba,
    pub selection: Option<Selection>,
    pub view: ViewState,
    pub dialog: Option<Dialog>,
    /// The last thing that happened, shown in the status bar.
    pub status: String,
    /// The pointer's position in document pixels, or `None` when it is off the canvas.
    pub cursor: Option<[f64; 2]>,
    pub recent: Vec<PathBuf>,
    /// A marquee or crop rectangle being dragged, in document pixels.
    pub drag_shape: Option<RectI>,
    /// The last point of the stroke in progress, for a smooth line between events.
    stroke_last: Option<[f64; 2]>,
    pub smoke: Option<SmokeState>,
    /// Bumped whenever the picture changes, so the canvas knows to re-upload its texture.
    pub revision: u64,
    /// A message for the interface to surface once, used by the command line paths.
    pub startup_message: Option<String>,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    /// An empty application: no document, default tools.
    #[must_use]
    pub fn new() -> Self {
        Self {
            document: None,
            history: History::default(),
            tool: Tool::Brush,
            brush: BrushSettings::default(),
            wand: WandSettings::default(),
            heal_radius: 16.0,
            foreground: [0, 0, 0, 255],
            background: [255, 255, 255, 255],
            selection: None,
            view: ViewState::default(),
            dialog: None,
            status: "就绪".to_owned(),
            cursor: None,
            recent: Vec::new(),
            drag_shape: None,
            stroke_last: None,
            smoke: None,
            revision: 0,
            startup_message: None,
        }
    }

    /// Runs an edit: record an undo step, change the document, and mark the picture stale.
    pub fn edit<R>(&mut self, label: &str, change: impl FnOnce(&mut Document) -> R) -> Option<R> {
        let document = self.document.as_mut()?;
        self.history.record(document, label);
        let result = change(document);
        self.document.as_mut().expect("still open").dirty = true;
        self.touch();
        Some(result)
    }

    /// Marks the composited picture as out of date.
    pub fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// Records an undo step for an edit the caller is about to make itself, for drags that
    /// change the document across many frames and should undo as one step.
    pub fn record(&mut self, label: &str) {
        if let Some(document) = self.document.as_ref() {
            let snapshot = document.clone();
            self.history.record(&snapshot, label);
        }
    }

    /// True when a document is open.
    #[must_use]
    pub fn has_document(&self) -> bool {
        self.document.is_some()
    }

    /// The layer the user is working on.
    #[must_use]
    pub fn active_layer(&self) -> Option<&Layer> {
        self.document.as_ref()?.active()
    }

    /// The active layer's id.
    #[must_use]
    pub fn active_layer_id(&self) -> Option<Uuid> {
        self.document.as_ref()?.active_layer
    }

    /// The document's size, or `None` when nothing is open.
    #[must_use]
    pub fn canvas_size(&self) -> Option<(u32, u32)> {
        self.document
            .as_ref()
            .map(|document| (document.width, document.height))
    }

    /// The composited picture, clipped to the canvas, for the canvas widget and exports.
    #[must_use]
    pub fn composite(&self) -> Option<PremulImage> {
        self.document.as_ref().map(composite_document)
    }

    /// Puts a message in the status bar.
    pub fn say(&mut self, message: impl Into<String>) {
        self.status = message.into();
    }

    /// Reports the result of an editing call: the message goes to the status bar, and failures
    /// also open the message dialog so nothing fails silently.
    pub fn report(&mut self, result: Result<String, String>) {
        match result {
            Ok(message) => self.say(message),
            Err(message) => {
                log::warn!("{message}");
                self.say(format!("失败：{message}"));
                self.dialog = Some(Dialog::Message {
                    title: "操作未完成".to_owned(),
                    detail: message,
                    is_error: true,
                });
            }
        }
    }

    // -----------------------------------------------------------------------------------------
    // Opening and saving
    // -----------------------------------------------------------------------------------------

    /// Opens a `.comp` project, a Photoshop file or an image, replacing whatever is open.
    ///
    /// # Errors
    ///
    /// Fails when the file cannot be read, or is not a format this build opens.
    pub fn open(&mut self, path: &Path) -> Result<(), String> {
        let document = if project::is_project(path) {
            project::load(path).map_err(|error| error.to_string())?
        } else if psd::looks_like_psd(&std::fs::read(path).map_err(|error| error.to_string())?) {
            let image = psd::load(path).map_err(|error| error.to_string())?;
            let mut document = Document::new(image.width(), image.height());
            let layer = Layer::new_pixel("Background", Arc::new(image));
            document.active_layer = Some(layer.id);
            document.layers.push(layer);
            document
        } else {
            let image = image_io::load(path).map_err(|error| error.to_string())?;
            let mut document = Document::new(image.width(), image.height());
            let layer = Layer::new_pixel("Background", Arc::new(image));
            document.active_layer = Some(layer.id);
            document.layers.push(layer);
            document
        };
        let name = path.file_name().map_or_else(
            || "untitled".to_owned(),
            |name| name.to_string_lossy().to_string(),
        );
        self.adopt(document, path.to_path_buf());
        self.say(format!("已打开 {name}"));
        log::info!("opened {name} from {}", path.display());
        Ok(())
    }

    /// Adds an image file as a new layer on top of the open document.
    ///
    /// # Errors
    ///
    /// Fails when there is no document, or the file cannot be read.
    pub fn import_as_layer(&mut self, path: &Path) -> Result<(), String> {
        if self.document.is_none() {
            return self.open(path);
        }
        let image = image_io::load(path).map_err(|error| error.to_string())?;
        let name = path.file_stem().map_or_else(
            || "Layer".to_owned(),
            |name| name.to_string_lossy().to_string(),
        );
        let (width, height) = (image.width(), image.height());
        self.edit("Import layer", |document| {
            document.add_layer(name.clone(), Arc::new(image), document.active_layer);
        });
        self.say(format!("已导入 {name}（{width}×{height}）"));
        Ok(())
    }

    /// Makes a new, empty document.
    pub fn new_document(&mut self, width: u32, height: u32, background: Background) {
        if crate::core::document::check_size(width, height).is_err() {
            self.report(Err(format!(
                "{width}×{height} is outside the supported canvas range"
            )));
            return;
        }
        let document = match background.color() {
            Some(color) => Document::with_background(width, height, color),
            None => {
                let mut document = Document::new(width, height);
                let layer = Layer::new_pixel("Layer 1", Arc::new(PremulImage::new(width, height)));
                document.active_layer = Some(layer.id);
                document.layers.push(layer);
                document
            }
        };
        self.adopt(document, PathBuf::new());
        self.say(format!("已新建文档 {width}×{height}"));
    }

    /// Installs a freshly opened document as the current one.
    fn adopt(&mut self, document: Document, path: PathBuf) {
        let has_path = !path.as_os_str().is_empty();
        self.history.clear();
        self.selection = None;
        self.dialog = None;
        self.drag_shape = None;
        self.stroke_last = None;
        self.view = ViewState::default();
        self.document = Some(document);
        if has_path {
            if let Some(document) = self.document.as_mut() {
                document.path = Some(path.clone());
            }
            self.remember_recent(&path);
        }
        self.touch();
    }

    /// Saves the document where it came from.
    ///
    /// # Errors
    ///
    /// Fails when no document is open, or when the project cannot be written.
    pub fn save(&mut self) -> Result<(), String> {
        let path = self
            .document
            .as_ref()
            .and_then(|document| document.path.clone())
            .ok_or_else(|| "该文档尚未保存为工程文件，请使用「另存为」".to_owned())?;
        self.save_to(&path)
    }

    /// Saves the document as a `.comp` project at `path`.
    ///
    /// # Errors
    ///
    /// Fails when no document is open, or when the project cannot be written.
    pub fn save_to(&mut self, path: &Path) -> Result<(), String> {
        let document = self
            .document
            .as_mut()
            .ok_or_else(|| "nothing is open".to_owned())?;
        project::save(document, path).map_err(|error| error.to_string())?;
        self.remember_recent(path);
        self.say(format!("已保存 {}", path.display()));
        log::info!("saved {}", path.display());
        Ok(())
    }

    /// Writes the flattened picture to an image file.
    ///
    /// # Errors
    ///
    /// Fails when no document is open, when the extension is not a format this build writes, or
    /// when the file cannot be written.
    pub fn export_to(&mut self, path: &Path, quality: u8) -> Result<(), String> {
        let document = self
            .document
            .as_ref()
            .ok_or_else(|| "nothing is open".to_owned())?;
        let format = image_io::extension(path)
            .and_then(|extension| ExportFormat::from_extension(&extension))
            .ok_or_else(|| {
                format!(
                    "{}: export as one of {}",
                    path.display(),
                    ExportFormat::ALL
                        .iter()
                        .map(|format| format.extension())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?;
        let picture = composite_document(document);
        let result = match format {
            ExportFormat::Jpeg => {
                image_io::save_jpeg(&picture, path, quality, [255, 255, 255, 255])
            }
            ExportFormat::Png => image_io::save_png(&picture, path),
            _ => image_io::save(&picture, path),
        };
        result.map_err(|error| error.to_string())?;
        self.say(format!("已导出 {}", path.display()));
        log::info!("exported {} as {}", path.display(), format.label());
        Ok(())
    }

    // -----------------------------------------------------------------------------------------
    // History
    // -----------------------------------------------------------------------------------------

    /// Steps back one edit.
    pub fn undo(&mut self) {
        let Some(document) = self.document.as_mut() else {
            return;
        };
        if let Some(label) = self.history.undo(document) {
            self.selection = None;
            self.touch();
            self.say(format!("撤销：{label}"));
        } else {
            self.say("没有可撤销的步骤");
        }
    }

    /// Steps forward one edit.
    pub fn redo(&mut self) {
        let Some(document) = self.document.as_mut() else {
            return;
        };
        if let Some(label) = self.history.redo(document) {
            self.selection = None;
            self.touch();
            self.say(format!("重做：{label}"));
        } else {
            self.say("没有可重做的步骤");
        }
    }

    // -----------------------------------------------------------------------------------------
    // Recent files
    // -----------------------------------------------------------------------------------------

    /// Reads the recent-files list, dropping entries that no longer exist.
    pub fn load_recent(&mut self) {
        let path = crate::platform::paths::recent_files_file();
        let Ok(contents) = std::fs::read_to_string(path) else {
            return;
        };
        self.recent = contents
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(PathBuf::from)
            .filter(|path| path.exists())
            .take(12)
            .collect();
    }

    fn remember_recent(&mut self, path: &Path) {
        let path = path.to_path_buf();
        self.recent.retain(|existing| existing != &path);
        self.recent.insert(0, path);
        self.recent.truncate(12);
        let file = crate::platform::paths::recent_files_file();
        if let Some(parent) = file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let contents: String = self
            .recent
            .iter()
            .map(|path| format!("{}\n", path.display()))
            .collect();
        if let Err(error) = std::fs::write(&file, contents) {
            log::warn!("could not write the recent-files list: {error}");
        }
    }

    // -----------------------------------------------------------------------------------------
    // Tools
    // -----------------------------------------------------------------------------------------

    /// Starts a brush stroke or a healing click at a document point.
    pub fn begin_stroke(&mut self, point: [f64; 2]) {
        match self.tool {
            Tool::Heal => {
                let radius = self.heal_radius;
                let layer = self.active_layer_id();
                let Some(layer) = layer else {
                    return;
                };
                let seed = point[0] as u32 ^ ((point[1] as u32) << 16);
                let document = self.document.as_mut();
                let label = "Spot Healing Brush";
                if let Some(document) = document {
                    self.history.record(document, label);
                }
                if let Some(document) = self.document.as_mut() {
                    match ops::heal(document, layer, point, radius, HealMode::ContentAware, seed) {
                        Ok(()) => {
                            self.touch();
                            self.say("已完成污点修复");
                        }
                        Err(error) => self.say(format!("失败：{error}")),
                    }
                }
            }
            tool if tool.is_brush() => {
                self.stroke_last = Some(point);
                let layer = self.active_layer_id();
                let settings = self.brush_settings();
                let selection = self.selection.clone();
                let Some(layer) = layer else {
                    return;
                };
                if let Some(document) = self.document.as_mut() {
                    self.history.record(document, "Brush");
                }
                if let Some(document) = self.document.as_mut() {
                    ops::stamp_brush(document, layer, point, &settings, selection.as_ref());
                }
                self.touch();
            }
            Tool::Eyedropper => self.pick_color(point),
            _ => {}
        }
    }

    /// Continues a brush stroke to a new point.
    pub fn continue_stroke(&mut self, point: [f64; 2]) {
        if !self.tool.is_brush() {
            return;
        }
        let Some(from) = self.stroke_last else {
            return;
        };
        let Some(layer) = self.active_layer_id() else {
            return;
        };
        let settings = self.brush_settings();
        let selection = self.selection.clone();
        if let Some(document) = self.document.as_mut() {
            ops::stroke_segment(document, layer, from, point, &settings, selection.as_ref());
        }
        self.stroke_last = Some(point);
        self.touch();
    }

    /// Ends the stroke in progress.
    pub fn end_stroke(&mut self) {
        self.stroke_last = None;
    }

    /// The brush settings with the current foreground color applied.
    #[must_use]
    pub fn brush_settings(&self) -> BrushSettings {
        BrushSettings {
            color: self.foreground,
            erase: self.tool == Tool::Eraser,
            ..self.brush
        }
    }

    /// Picks the color under a document point, from the composited picture.
    pub fn pick_color(&mut self, point: [f64; 2]) {
        let Some(picture) = self.composite() else {
            return;
        };
        let x = point[0].floor();
        let y = point[1].floor();
        if x < 0.0 || y < 0.0 || x >= f64::from(picture.width()) || y >= f64::from(picture.height())
        {
            return;
        }
        let pixel = picture.pixel(x as u32, y as u32).unwrap_or([0; 4]);
        let straight = unpremultiply(pixel);
        self.foreground = straight;
        self.say(format!(
            "已拾取颜色 #{:02X}{:02X}{:02X}",
            straight[0], straight[1], straight[2]
        ));
    }

    // -----------------------------------------------------------------------------------------
    // Selections
    // -----------------------------------------------------------------------------------------

    /// Selects everything.
    pub fn select_all(&mut self) {
        let Some((width, height)) = self.canvas_size() else {
            return;
        };
        self.selection = Some(Selection::all(width, height));
        self.say("已全选");
    }

    /// Drops the selection.
    pub fn deselect(&mut self) {
        self.selection = None;
        self.say("已取消选择");
    }

    /// Turns the selection inside out.
    pub fn invert_selection(&mut self) {
        match self.selection.as_mut() {
            Some(selection) => {
                selection.invert();
                self.touch();
                self.say("已反选");
            }
            None => self.select_all(),
        }
    }

    /// Softens the selection's edge.
    pub fn feather_selection(&mut self, radius: u32) {
        let Some(selection) = self.selection.as_mut() else {
            self.say("请先创建选区");
            return;
        };
        selection.feather(radius);
        self.touch();
        self.say(format!("羽化 {radius} 像素"));
    }

    /// Turns a dragged rectangle into a selection.
    pub fn marquee(&mut self, rect: RectI, ellipse: bool, mode: SelectionMode) {
        let Some((width, height)) = self.canvas_size() else {
            return;
        };
        if rect.width() < 1 || rect.height() < 1 {
            return;
        }
        let shape = if ellipse {
            Selection::ellipse(width, height, rect)
        } else {
            Selection::rectangle(width, height, rect)
        };
        match self.selection.as_mut() {
            Some(existing) if mode != SelectionMode::Replace => existing.combine(&shape, mode),
            _ => self.selection = Some(shape),
        }
        self.touch();
        self.say(format!("已选择 {}×{}", rect.width(), rect.height()));
    }

    /// Runs the magic wand at a document point.
    pub fn wand_select(&mut self, point: [f64; 2], mode: SelectionMode) {
        let Some(layer) = self.active_layer_id() else {
            return;
        };
        let Some(document) = self.document.as_ref() else {
            return;
        };
        let settings = self.wand;
        match ops::magic_wand_selection(document, layer, point, &settings) {
            Ok(shape) => {
                match self.selection.as_mut() {
                    Some(existing) if mode != SelectionMode::Replace => {
                        existing.combine(&shape, mode);
                    }
                    _ => self.selection = Some(shape),
                }
                self.touch();
                let count = self
                    .selection
                    .as_ref()
                    .and_then(Selection::bounds)
                    .map_or(0, |bounds| bounds.width() * bounds.height());
                self.say(format!(
                    "魔棒：容差 {}，包围盒内约 {count} 像素",
                    settings.tolerance
                ));
            }
            Err(error) => self.report(Err(error.to_string())),
        }
    }

    /// Select ▸ Load Layer Alpha.
    pub fn select_from_layer_alpha(&mut self) {
        let Some(layer) = self.active_layer_id() else {
            return;
        };
        let Some(document) = self.document.as_ref() else {
            return;
        };
        match ops::load_layer_alpha(document, layer) {
            Ok(selection) => {
                self.selection = Some(selection);
                self.touch();
                self.say("已从图层像素载入选区");
            }
            Err(error) => self.report(Err(error.to_string())),
        }
    }

    /// Deletes the selected pixels from the active layer.
    pub fn delete_selection(&mut self) {
        let Some(selection) = self.selection.clone() else {
            self.say("请先创建选区");
            return;
        };
        let Some(layer) = self.active_layer_id() else {
            return;
        };
        let mut outcome = Ok("已清除选区像素".to_owned());
        if let Some(document) = self.document.as_mut() {
            self.history.record(document, "Delete");
        }
        if let Some(document) = self.document.as_mut()
            && let Err(error) = ops::delete_selected_pixels(document, layer, &selection)
        {
            outcome = Err(error.to_string());
        }
        self.touch();
        self.report(outcome);
    }

    // -----------------------------------------------------------------------------------------
    // Layers
    // -----------------------------------------------------------------------------------------

    /// Adds an empty layer above the active one.
    pub fn add_layer(&mut self) {
        let Some((width, height)) = self.canvas_size() else {
            return;
        };
        let name = format!(
            "Layer {}",
            self.document.as_ref().map_or(1, |d| d.layers.len() + 1)
        );
        self.edit("New layer", |document| {
            document.add_layer(
                name,
                Arc::new(PremulImage::new(width, height)),
                document.active_layer,
            );
        });
        self.say("已新建图层");
    }

    /// Copies the active layer, and anything inside it.
    pub fn duplicate_layer(&mut self) {
        let Some(layer) = self.active_layer_id() else {
            return;
        };
        self.edit("Duplicate layer", |document| {
            document.duplicate_layer(layer);
        });
        self.say("已复制图层");
    }

    /// Deletes the active layer.
    pub fn delete_layer(&mut self) {
        let Some(layer) = self.active_layer_id() else {
            return;
        };
        self.edit("Delete layer", |document| {
            document.remove_layer(layer);
        });
        self.say("已删除图层");
    }

    /// Merges the active layer into the one below it.
    pub fn merge_down(&mut self) {
        let Some(layer) = self.active_layer_id() else {
            return;
        };
        let mut outcome = Ok("已向下合并".to_owned());
        if let Some(document) = self.document.as_mut() {
            self.history.record(document, "Merge Down");
            if let Err(error) = document.merge_down(layer) {
                outcome = Err(error.to_string());
            }
        }
        self.touch();
        self.report(outcome);
    }

    /// Collapses everything into one layer.
    pub fn flatten(&mut self) {
        self.edit("Flatten Image", Document::flatten);
        self.say("已拼合图像");
    }

    /// Puts the active layer into a new folder.
    pub fn group_active_layer(&mut self) {
        let Some(layer) = self.active_layer_id() else {
            return;
        };
        self.edit("Group Layers", |document| {
            document.group_layers(&[layer], "Folder");
        });
        self.say("已编组");
    }

    /// Lifts the active folder's contents out.
    pub fn ungroup_active_layer(&mut self) {
        let Some(id) = self.active_layer_id() else {
            return;
        };
        let is_group = self
            .document
            .as_ref()
            .is_some_and(|document| document.is_group(id));
        if !is_group {
            self.say("请先选择一个文件夹图层");
            return;
        }
        self.edit("Ungroup", |document| {
            document.ungroup(id);
        });
        self.say("已取消编组");
    }

    /// Makes a layer active.
    pub fn select_layer(&mut self, id: Uuid) {
        if let Some(document) = self.document.as_mut() {
            document.active_layer = Some(id);
        }
        self.touch();
    }

    /// Renames a layer.
    pub fn rename_layer(&mut self, id: Uuid, name: String) {
        self.edit("Rename layer", |document| {
            if let Some(layer) = document.layer_mut(id) {
                layer.name = name;
            }
        });
    }

    /// Shows or hides a layer.
    pub fn set_layer_visible(&mut self, id: Uuid, visible: bool) {
        self.edit("Layer visibility", |document| {
            if let Some(layer) = document.layer_mut(id) {
                layer.visible = visible;
            }
        });
    }

    /// Sets a layer's opacity.
    pub fn set_layer_opacity(&mut self, id: Uuid, opacity: f64) {
        self.edit("Layer opacity", |document| {
            if let Some(layer) = document.layer_mut(id) {
                layer.opacity = opacity.clamp(0.0, 1.0);
            }
        });
    }

    /// Sets a layer's blend mode.
    pub fn set_layer_blend(&mut self, id: Uuid, blend: crate::core::blend::BlendMode) {
        self.edit("Blend mode", |document| {
            if let Some(layer) = document.layer_mut(id) {
                layer.blend = blend;
            }
        });
    }

    /// Moves a layer one step up or down its stack.
    pub fn nudge_layer(&mut self, id: Uuid, delta: i32) {
        self.edit("Restack", |document| {
            document.nudge_layer(id, delta);
        });
    }

    // -----------------------------------------------------------------------------------------
    // Masks
    // -----------------------------------------------------------------------------------------

    /// Adds a mask that reveals everything.
    pub fn add_mask(&mut self) {
        let Some(layer) = self.active_layer_id() else {
            return;
        };
        let mut outcome = Ok("已添加图层蒙版".to_owned());
        if let Some(document) = self.document.as_mut() {
            self.history.record(document, "Add Layer Mask");
            if let Err(error) = ops::add_mask(document, layer, true) {
                outcome = Err(error.to_string());
            }
        }
        self.touch();
        self.report(outcome);
    }

    /// Removes the active layer's mask.
    pub fn remove_mask(&mut self) {
        let Some(layer) = self.active_layer_id() else {
            return;
        };
        let mut outcome = Ok("已删除蒙版".to_owned());
        if let Some(document) = self.document.as_mut() {
            self.history.record(document, "Delete Layer Mask");
            if let Err(error) = ops::remove_mask(document, layer) {
                outcome = Err(error.to_string());
            }
        }
        self.touch();
        self.report(outcome);
    }

    /// Inverts the active layer's mask.
    pub fn invert_mask(&mut self) {
        let Some(layer) = self.active_layer_id() else {
            return;
        };
        let mut outcome = Ok("已反相蒙版".to_owned());
        if let Some(document) = self.document.as_mut() {
            self.history.record(document, "Invert Layer Mask");
            if let Err(error) = ops::invert_mask(document, layer) {
                outcome = Err(error.to_string());
            }
        }
        self.touch();
        self.report(outcome);
    }

    /// Bakes the mask into the pixels.
    pub fn apply_mask(&mut self) {
        let Some(layer) = self.active_layer_id() else {
            return;
        };
        let mut outcome = Ok("已应用蒙版".to_owned());
        if let Some(document) = self.document.as_mut() {
            self.history.record(document, "Apply Layer Mask");
            if let Err(error) = ops::apply_mask(document, layer) {
                outcome = Err(error.to_string());
            }
        }
        self.touch();
        self.report(outcome);
    }

    /// Flips whether the mask affects the picture.
    pub fn toggle_mask_enabled(&mut self) {
        let Some(layer) = self.active_layer_id() else {
            return;
        };
        self.edit("Mask visibility", |document| {
            if let Some(layer) = document.layer_mut(layer) {
                layer.mask_enabled = !layer.mask_enabled;
            }
        });
    }

    // -----------------------------------------------------------------------------------------
    // The canvas itself
    // -----------------------------------------------------------------------------------------

    /// Crops to a rectangle in document pixels.
    pub fn crop_to(&mut self, rect: RectI) {
        let mut outcome = Ok("已裁剪".to_owned());
        if let Some(document) = self.document.as_mut() {
            self.history.record(document, "Crop");
            if let Err(error) = ops::crop(document, rect) {
                outcome = Err(error.to_string());
            }
        }
        self.selection = None;
        self.touch();
        self.report(outcome);
    }

    /// Crops away everything the picture does not cover.
    pub fn trim(&mut self) {
        let mut outcome = Ok("已修整".to_owned());
        if let Some(document) = self.document.as_mut() {
            self.history.record(document, "Trim");
            if let Err(error) = ops::trim(document) {
                outcome = Err(error.to_string());
            }
        }
        self.selection = None;
        self.touch();
        self.report(outcome);
    }

    /// Mirrors the whole canvas.
    pub fn flip_canvas(&mut self, horizontal: bool) {
        self.edit("Flip Canvas", |document| {
            ops::flip_canvas(document, horizontal);
        });
        self.say(if horizontal {
            "已水平翻转画布"
        } else {
            "已垂直翻转画布"
        });
    }

    /// Mirrors the active layer about its own center.
    pub fn flip_active_layer(&mut self, horizontal: bool) {
        let Some(layer) = self.active_layer_id() else {
            return;
        };
        let mut outcome = Ok("已翻转图层".to_owned());
        if let Some(document) = self.document.as_mut() {
            self.history.record(document, "Flip Layer");
            if let Err(error) = ops::flip_layer(document, layer, horizontal) {
                outcome = Err(error.to_string());
            }
        }
        self.touch();
        self.report(outcome);
    }

    /// Turns the canvas a quarter turn.
    pub fn rotate_canvas(&mut self, clockwise: bool) {
        let mut outcome = Ok("已旋转画布".to_owned());
        if let Some(document) = self.document.as_mut() {
            self.history.record(document, "Rotate Canvas");
            if let Err(error) = ops::rotate_canvas_90(document, clockwise) {
                outcome = Err(error.to_string());
            }
        }
        self.selection = None;
        self.touch();
        self.report(outcome);
    }

    /// Scales the whole document.
    ///
    /// # Errors
    ///
    /// Fails when the requested size is out of range.
    pub fn resize_image(&mut self, width: u32, height: u32, resample: bool) -> Result<(), String> {
        let label = format!("Image Size {width}×{height}");
        let Some(document) = self.document.as_mut() else {
            return Err("nothing is open".to_owned());
        };
        self.history.record(document, label);
        let result = document
            .resize_image(width, height, resample)
            .map_err(|error| error.to_string());
        self.selection = None;
        self.touch();
        result
    }

    /// Changes the canvas size, keeping the picture where the anchor says.
    ///
    /// # Errors
    ///
    /// Fails when the requested size is out of range.
    pub fn resize_canvas(
        &mut self,
        width: u32,
        height: u32,
        anchor: [f64; 2],
    ) -> Result<(), String> {
        let label = format!("Canvas Size {width}×{height}");
        let Some(document) = self.document.as_mut() else {
            return Err("nothing is open".to_owned());
        };
        self.history.record(document, label);
        let result = document
            .resize_canvas(width, height, anchor)
            .map_err(|error| error.to_string());
        self.selection = None;
        self.touch();
        result
    }

    // -----------------------------------------------------------------------------------------
    // The view
    // -----------------------------------------------------------------------------------------

    /// Zooms in around the canvas center.
    pub fn zoom_in(&mut self) {
        self.view.zoom = (self.view.zoom * 1.25).min(32.0);
    }

    /// Zooms out around the canvas center.
    pub fn zoom_out(&mut self) {
        self.view.zoom = (self.view.zoom / 1.25).max(0.02);
    }

    /// Back to one screen pixel per document pixel.
    pub fn zoom_actual(&mut self) {
        self.view.zoom = 1.0;
        self.view.fit_pending = false;
    }

    /// Fits the canvas in the window on the next layout pass.
    pub fn zoom_to_fit(&mut self) {
        self.view.fit_pending = true;
    }

    // -----------------------------------------------------------------------------------------
    // Painting
    // -----------------------------------------------------------------------------------------

    /// Fills the selection with a color (Edit ▸ Fill).
    pub fn fill_selection(&mut self, color: Rgba) {
        let Some(layer) = self.active_layer_id() else {
            self.say("请先选择图层");
            return;
        };
        let selection = self.selection.clone();
        let mut outcome = Ok("已填充".to_owned());
        if let Some(document) = self.document.as_mut() {
            self.history.record(document, "Fill");
            if let Err(error) = ops::fill(document, layer, color, selection.as_ref()) {
                outcome = Err(error.to_string());
            }
        }
        self.touch();
        self.report(outcome);
    }

    /// Edit ▸ Fill ▸ Content-Aware, the kernel from the original application.
    pub fn content_aware_fill(&mut self) {
        let Some(selection) = self.selection.clone() else {
            self.say("内容感知填充需要先创建选区");
            return;
        };
        let Some(layer) = self.active_layer_id() else {
            return;
        };
        let mut outcome = Ok("已用周边像素完成填充".to_owned());
        if let Some(document) = self.document.as_mut() {
            self.history.record(document, "Content-Aware Fill");
            if let Err(error) = ops::content_aware_fill(document, layer, &selection) {
                outcome = Err(error.to_string());
            }
        }
        self.touch();
        self.report(outcome);
    }

    // -----------------------------------------------------------------------------------------
    // Adjustments and filters
    // -----------------------------------------------------------------------------------------

    /// Applies an adjustment to the active layer's pixels.
    ///
    /// The work is not done here: the caller opens the dialog, which previews and commits.
    pub fn open_adjustment_dialog(&mut self, adjustment: Adjustment) {
        let Some(document) = self.document.as_ref() else {
            self.say("请先打开文档");
            return;
        };
        let target = self
            .active_layer()
            .filter(|layer| layer.kind == LayerKind::Adjustment)
            .map_or(AdjustTarget::Pixels, |layer| AdjustTarget::Layer(layer.id));
        let baseline = Box::new(document.clone());
        self.dialog = Some(Dialog::Adjust(Box::new(AdjustDialog {
            adjustment,
            target,
            baseline,
            needs_preview: true,
        })));
    }

    /// Re-runs the open adjustment over the document as it was when the dialog opened.
    pub fn preview_adjustment(&mut self) {
        let Some(Dialog::Adjust(dialog)) = self.dialog.as_mut() else {
            return;
        };
        if !dialog.needs_preview {
            return;
        }
        dialog.needs_preview = false;
        let adjustment = dialog.adjustment.clone();
        let target = dialog.target;
        let baseline = dialog.baseline.clone();
        let selection = self.selection.clone();
        let Some(document) = self.document.as_mut() else {
            return;
        };
        let active = document.active_layer;
        *document = *baseline;
        document.active_layer = active;
        match target {
            AdjustTarget::Pixels => {
                if let Some(layer) = document.active_layer {
                    let _ = ops::apply_adjustment_to_layer(
                        document,
                        layer,
                        &adjustment,
                        selection.as_ref(),
                    );
                }
            }
            AdjustTarget::Layer(id) => {
                let _ = ops::update_adjustment_layer(document, id, adjustment);
            }
        }
        self.touch();
    }

    /// Keeps what the dialog is previewing, as one undoable step.
    pub fn commit_adjustment_dialog(&mut self) {
        let Some(Dialog::Adjust(dialog)) = self.dialog.take() else {
            return;
        };
        let label = dialog.adjustment.display_label().to_owned();
        let baseline = dialog.baseline;
        let current = self.document.take();
        if let Some(current) = current {
            // The history step is the document as it was before the dialog opened.
            self.history.record(&baseline, label.clone());
            self.document = Some(current);
        }
        self.say(format!("已应用 {label}"));
    }

    /// Puts the document back the way it was before the dialog opened.
    pub fn cancel_dialog(&mut self) {
        match self.dialog.take() {
            Some(Dialog::Adjust(dialog)) => {
                let active = self
                    .document
                    .as_ref()
                    .and_then(|document| document.active_layer);
                self.document = Some(*dialog.baseline);
                if let Some(document) = self.document.as_mut() {
                    document.active_layer = active;
                }
                self.touch();
                self.say("已取消");
            }
            Some(Dialog::Filter(dialog)) => {
                let active = self
                    .document
                    .as_ref()
                    .and_then(|document| document.active_layer);
                self.document = Some(*dialog.baseline);
                if let Some(document) = self.document.as_mut() {
                    document.active_layer = active;
                }
                self.touch();
                self.say("已取消");
            }
            other => {
                self.dialog = other;
            }
        }
    }

    /// Adds an adjustment layer above the active one.
    pub fn add_adjustment_layer(&mut self, adjustment: Adjustment) {
        let label = adjustment.display_label().to_owned();
        self.edit("New adjustment layer", |document| {
            ops::add_adjustment_layer(document, adjustment)
        });
        self.say(format!("已新建 {label} 调整图层"));
    }

    /// Applies a destructive filter to the active layer.
    pub fn apply_filter(&mut self, filter: Filter) {
        let Some(layer) = self.active_layer_id() else {
            return;
        };
        let selection = self.selection.clone();
        let label = filter.label().to_owned();
        let mut outcome = Ok(format!("Applied {label}"));
        if let Some(document) = self.document.as_mut() {
            self.history.record(document, &label);
            if let Err(error) =
                ops::apply_filter_to_layer(document, layer, &filter, selection.as_ref())
            {
                outcome = Err(error.to_string());
            }
        }
        self.touch();
        self.report(outcome);
    }
}

/// Undoes premultiplication for a single pixel, for the eyedropper and the color swatches.
#[must_use]
pub fn unpremultiply(pixel: Rgba) -> Rgba {
    let alpha = u32::from(pixel[3]);
    if alpha == 0 {
        return [0, 0, 0, 0];
    }
    if alpha == 255 {
        return pixel;
    }
    [
        ((u32::from(pixel[0]) * 255 + alpha / 2) / alpha).min(255) as u8,
        ((u32::from(pixel[1]) * 255 + alpha / 2) / alpha).min(255) as u8,
        ((u32::from(pixel[2]) * 255 + alpha / 2) / alpha).min(255) as u8,
        pixel[3],
    ]
}

/// Turns a straight color into the premultiplied form the canvas expects.
#[must_use]
pub fn premultiply(color: Rgba) -> Rgba {
    let alpha = u32::from(color[3]);
    [
        ((u32::from(color[0]) * alpha + 127) / 255) as u8,
        ((u32::from(color[1]) * alpha + 127) / 255) as u8,
        ((u32::from(color[2]) * alpha + 127) / 255) as u8,
        color[3],
    ]
}

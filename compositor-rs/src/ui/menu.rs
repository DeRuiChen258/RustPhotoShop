//! The menu bar, with the same arrangement of commands as the macOS build.

use eframe::egui;

use crate::app::state::{AppState, Background, Dialog, ExportFormat};
use crate::core::adjustment::{
    Adjustment, ColorBalanceSettings, ExposureSettings, GradientMapSettings, GrainSettings,
    LevelsSettings,
};

/// Draws the menu bar.
pub fn draw(state: &mut AppState, ui: &mut egui::Ui) {
    egui::Panel::top("rustphotoshop-menu").show(ui, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            file_menu(state, ui);
            edit_menu(state, ui);
            image_menu(state, ui);
            layer_menu(state, ui);
            select_menu(state, ui);
            adjust_menu(state, ui);
            filter_menu(state, ui);
            view_menu(state, ui);
            help_menu(state, ui);
        });
    });
}

fn file_menu(state: &mut AppState, ui: &mut egui::Ui) {
    ui.menu_button("文件", |ui| {
        if ui.button("新建…\tCtrl+N").clicked() {
            state.dialog = Some(Dialog::NewDocument {
                width: 1920,
                height: 1080,
                background: Background::White,
            });
            ui.close();
        }
        if ui.button("打开…\tCtrl+O").clicked() {
            super::dialogs::open_file_dialog(state);
            ui.close();
        }
        let recent = state.recent.clone();
        if !recent.is_empty() {
            ui.menu_button("最近打开", |ui| {
                for path in recent {
                    let name = path.file_name().map_or_else(
                        || path.display().to_string(),
                        |name| name.to_string_lossy().to_string(),
                    );
                    if ui.button(name).clicked() {
                        let result = state.open(&path);
                        if let Err(error) = result {
                            state.report(Err(error));
                        }
                        ui.close();
                    }
                }
            });
        }
        if ui.button("作为图层导入…").clicked() {
            if let Some(path) = super::dialogs::pick_image() {
                let result = state.import_as_layer(&path);
                if let Err(error) = result {
                    state.report(Err(error));
                }
            }
            ui.close();
        }
        ui.separator();
        let has_document = state.has_document();
        if ui
            .add_enabled(has_document, egui::Button::new("保存\tCtrl+S"))
            .clicked()
        {
            super::dialogs::save_project(state, false);
            ui.close();
        }
        if ui
            .add_enabled(has_document, egui::Button::new("另存为…\tCtrl+Shift+S"))
            .clicked()
        {
            super::dialogs::save_project(state, true);
            ui.close();
        }
        if ui
            .add_enabled(has_document, egui::Button::new("导出…\tCtrl+Shift+E"))
            .clicked()
        {
            state.dialog = Some(Dialog::Export {
                path: String::new(),
                format: ExportFormat::Png,
                quality: 90,
            });
            ui.close();
        }
        ui.separator();
        if ui.button("退出").clicked() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    });
}

fn edit_menu(state: &mut AppState, ui: &mut egui::Ui) {
    ui.menu_button("编辑", |ui| {
        let undo_label = match state.history.undo_label() {
            Some(label) => format!("撤销 {label}\tCtrl+Z"),
            None => "撤销\tCtrl+Z".to_owned(),
        };
        if ui
            .add_enabled(state.history.can_undo(), egui::Button::new(undo_label))
            .clicked()
        {
            state.undo();
            ui.close();
        }
        let redo_label = match state.history.redo_label() {
            Some(label) => format!("重做 {label}\tCtrl+Shift+Z"),
            None => "重做\tCtrl+Shift+Z".to_owned(),
        };
        if ui
            .add_enabled(state.history.can_redo(), egui::Button::new(redo_label))
            .clicked()
        {
            state.redo();
            ui.close();
        }
        ui.separator();
        let foreground = state.foreground;
        let background = state.background;
        if ui.button("用前景色填充").clicked() {
            state.fill_selection(foreground);
            ui.close();
        }
        if ui.button("用背景色填充").clicked() {
            state.fill_selection(background);
            ui.close();
        }
        let has_selection = state.selection.is_some();
        if ui
            .add_enabled(has_selection, egui::Button::new("内容感知填充"))
            .clicked()
        {
            state.content_aware_fill();
            ui.close();
        }
        if ui
            .add_enabled(has_selection, egui::Button::new("删除选区像素\tDel"))
            .clicked()
        {
            state.delete_selection();
            ui.close();
        }
    });
}

fn image_menu(state: &mut AppState, ui: &mut egui::Ui) {
    ui.menu_button("图像", |ui| {
        let has_document = state.has_document();
        let (width, height) = state.canvas_size().unwrap_or((0, 0));
        if ui
            .add_enabled(has_document, egui::Button::new("图像大小…"))
            .clicked()
        {
            state.dialog = Some(Dialog::ImageSize {
                width,
                height,
                resample: true,
            });
            ui.close();
        }
        if ui
            .add_enabled(has_document, egui::Button::new("画布大小…"))
            .clicked()
        {
            state.dialog = Some(Dialog::CanvasSize {
                width,
                height,
                anchor: [0.5, 0.5],
            });
            ui.close();
        }
        if ui
            .add_enabled(has_document, egui::Button::new("修整（裁剪到内容）"))
            .clicked()
        {
            state.trim();
            ui.close();
        }
        ui.separator();
        if ui
            .add_enabled(has_document, egui::Button::new("顺时针旋转 90°"))
            .clicked()
        {
            state.rotate_canvas(true);
            ui.close();
        }
        if ui
            .add_enabled(has_document, egui::Button::new("逆时针旋转 90°"))
            .clicked()
        {
            state.rotate_canvas(false);
            ui.close();
        }
        if ui
            .add_enabled(has_document, egui::Button::new("水平翻转画布"))
            .clicked()
        {
            state.flip_canvas(true);
            ui.close();
        }
        if ui
            .add_enabled(has_document, egui::Button::new("垂直翻转画布"))
            .clicked()
        {
            state.flip_canvas(false);
            ui.close();
        }
    });
}

fn layer_menu(state: &mut AppState, ui: &mut egui::Ui) {
    ui.menu_button("图层", |ui| {
        let has_layer = state.active_layer().is_some();
        let has_mask = state
            .active_layer()
            .is_some_and(|layer| layer.mask.is_some());
        if ui
            .add_enabled(has_layer, egui::Button::new("新建图层\tCtrl+Shift+N"))
            .clicked()
        {
            state.add_layer();
            ui.close();
        }
        if ui
            .add_enabled(has_layer, egui::Button::new("复制图层\tCtrl+J"))
            .clicked()
        {
            state.duplicate_layer();
            ui.close();
        }
        if ui
            .add_enabled(has_layer, egui::Button::new("删除图层"))
            .clicked()
        {
            state.delete_layer();
            ui.close();
        }
        ui.separator();
        if ui
            .add_enabled(has_layer, egui::Button::new("编组"))
            .clicked()
        {
            state.group_active_layer();
            ui.close();
        }
        let is_group = state
            .document
            .as_ref()
            .zip(state.active_layer_id())
            .is_some_and(|(document, id)| document.is_group(id));
        if ui
            .add_enabled(is_group, egui::Button::new("取消编组"))
            .clicked()
        {
            state.ungroup_active_layer();
            ui.close();
        }
        if ui
            .add_enabled(has_layer, egui::Button::new("向下合并\tCtrl+E"))
            .clicked()
        {
            state.merge_down();
            ui.close();
        }
        if ui
            .add_enabled(has_layer, egui::Button::new("拼合图像"))
            .clicked()
        {
            state.flatten();
            ui.close();
        }
        ui.separator();
        if ui
            .add_enabled(has_layer, egui::Button::new("添加图层蒙版"))
            .clicked()
        {
            state.add_mask();
            ui.close();
        }
        if ui
            .add_enabled(has_mask, egui::Button::new("反相蒙版"))
            .clicked()
        {
            state.invert_mask();
            ui.close();
        }
        if ui
            .add_enabled(has_mask, egui::Button::new("应用图层蒙版"))
            .clicked()
        {
            state.apply_mask();
            ui.close();
        }
        if ui
            .add_enabled(has_mask, egui::Button::new("删除图层蒙版"))
            .clicked()
        {
            state.remove_mask();
            ui.close();
        }
        ui.separator();
        if ui
            .add_enabled(has_layer, egui::Button::new("水平翻转图层"))
            .clicked()
        {
            state.flip_active_layer(true);
            ui.close();
        }
        if ui
            .add_enabled(has_layer, egui::Button::new("垂直翻转图层"))
            .clicked()
        {
            state.flip_active_layer(false);
            ui.close();
        }
    });
}

fn select_menu(state: &mut AppState, ui: &mut egui::Ui) {
    ui.menu_button("选择", |ui| {
        let has_document = state.has_document();
        let has_selection = state.selection.is_some();
        if ui
            .add_enabled(has_document, egui::Button::new("全选\tCtrl+A"))
            .clicked()
        {
            state.select_all();
            ui.close();
        }
        if ui
            .add_enabled(has_selection, egui::Button::new("取消选择\tCtrl+D"))
            .clicked()
        {
            state.deselect();
            ui.close();
        }
        if ui
            .add_enabled(has_document, egui::Button::new("反选\tCtrl+Shift+I"))
            .clicked()
        {
            state.invert_selection();
            ui.close();
        }
        ui.separator();
        if ui
            .add_enabled(has_selection, egui::Button::new("羽化 4 像素"))
            .clicked()
        {
            state.feather_selection(4);
            ui.close();
        }
        if ui
            .add_enabled(has_selection, egui::Button::new("羽化 16 像素"))
            .clicked()
        {
            state.feather_selection(16);
            ui.close();
        }
        let has_layer = state.active_layer().is_some();
        if ui
            .add_enabled(has_layer, egui::Button::new("载入图层 Alpha 通道"))
            .clicked()
        {
            state.select_from_layer_alpha();
            ui.close();
        }
    });
}

fn adjust_menu(state: &mut AppState, ui: &mut egui::Ui) {
    ui.menu_button("调整", |ui| {
        let has_layer = state
            .active_layer()
            .is_some_and(|layer| layer.kind == crate::core::document::LayerKind::Pixel);
        adjustment_items(state, ui, has_layer, false);
        ui.separator();
        ui.menu_button("新建调整图层", |ui| {
            adjustment_items(state, ui, state.has_document(), true);
        });
    });
}

/// The adjustment list, used both for Image ▸ Adjustments and for new adjustment layers.
fn adjustment_items(state: &mut AppState, ui: &mut egui::Ui, enabled: bool, as_layer: bool) {
    let defaults: [(&str, Adjustment); 12] = [
        ("色阶…", Adjustment::Levels(LevelsSettings::default())),
        (
            "曲线…",
            Adjustment::Curves(crate::core::adjustment::CurvesSettings::default()),
        ),
        (
            "色相/饱和度…",
            Adjustment::HueSaturation {
                hue: 0.0,
                saturation: 0.0,
                lightness: 0.0,
                colorize: false,
            },
        ),
        ("曝光…", Adjustment::Exposure(ExposureSettings::default())),
        ("反相", Adjustment::Invert),
        (
            "黑白…",
            Adjustment::BlackAndWhite {
                weights: [0.4, 0.6, 0.4, 0.6, 0.2, 0.8],
                tint: false,
                tint_hue: 0.0,
                tint_saturation: 0.0,
            },
        ),
        (
            "色彩平衡…",
            Adjustment::ColorBalance(ColorBalanceSettings::default()),
        ),
        (
            "渐变映射…",
            Adjustment::GradientMap(GradientMapSettings::default()),
        ),
        ("Gaussian Blur…", Adjustment::GaussianBlur { radius: 4.0 }),
        (
            "动感模糊…",
            Adjustment::MotionBlur {
                angle: 0.0,
                distance: 20.0,
            },
        ),
        (
            "添加杂色…",
            Adjustment::AddNoise {
                amount: 10.0,
                gaussian: false,
                monochromatic: false,
                seed: 1,
            },
        ),
        ("颗粒…", Adjustment::Grain(GrainSettings::default())),
    ];
    for (label, adjustment) in defaults {
        if !ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
            continue;
        }
        if as_layer {
            state.add_adjustment_layer(adjustment);
        } else if matches!(adjustment, Adjustment::Invert) {
            // Nothing to set, so it applies straight away like the other instant commands.
            let layer = state.active_layer_id();
            if let Some(layer) = layer {
                let selection = state.selection.clone();
                let mut outcome = Ok("已反相".to_owned());
                if let Some(document) = state.document.as_mut() {
                    state.history.record(document, "Invert");
                    if let Err(error) = crate::core::ops::apply_adjustment_to_layer(
                        document,
                        layer,
                        &Adjustment::Invert,
                        selection.as_ref(),
                    ) {
                        outcome = Err(error.to_string());
                    }
                }
                state.touch();
                state.report(outcome);
            }
        } else {
            state.open_adjustment_dialog(adjustment);
        }
        ui.close();
    }
}

fn filter_menu(state: &mut AppState, ui: &mut egui::Ui) {
    ui.menu_button("滤镜", |ui| {
        let enabled = state
            .active_layer()
            .is_some_and(|layer| layer.kind == crate::core::document::LayerKind::Pixel);
        let filters: [(&str, crate::core::adjustment::Filter); 8] = [
            (
                "高斯模糊…",
                crate::core::adjustment::Filter::GaussianBlur { radius: 4.0 },
            ),
            (
                "动感模糊…",
                crate::core::adjustment::Filter::MotionBlur {
                    angle: 0.0,
                    distance: 20.0,
                },
            ),
            (
                "添加杂色…",
                crate::core::adjustment::Filter::AddNoise {
                    amount: 10.0,
                    gaussian: false,
                    monochromatic: false,
                    seed: 1,
                },
            ),
            (
                "颗粒…",
                crate::core::adjustment::Filter::Grain {
                    amount: 20.0,
                    size: 1.5,
                    roughness: 40.0,
                    seed: 1,
                },
            ),
            (
                "暗角…",
                crate::core::adjustment::Filter::Vignette {
                    amount: 60.0,
                    midpoint: 50.0,
                    roundness: 0.0,
                    feather: 50.0,
                    highlights: 0.0,
                    color: [0, 0, 0, 255],
                },
            ),
            (
                "色调对比…",
                crate::core::adjustment::Filter::TonalContrast {
                    amount: 50.0,
                    shadows: 0.0,
                    midtones: 0.0,
                    highlights: 0.0,
                    radius: 20.0,
                },
            ),
            (
                "镜头校正…",
                crate::core::adjustment::Filter::LensCorrection { k: 0.1 },
            ),
            (
                "抖动…",
                crate::core::adjustment::Filter::Dither(crate::ffi::DitherSettings::default()),
            ),
        ];
        for (label, filter) in filters {
            if !ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                continue;
            }
            super::dialogs::open_filter_dialog(state, filter);
            ui.close();
        }
    });
}

fn view_menu(state: &mut AppState, ui: &mut egui::Ui) {
    ui.menu_button("视图", |ui| {
        let enabled = state.has_document();
        if ui
            .add_enabled(enabled, egui::Button::new("放大\tCtrl++"))
            .clicked()
        {
            state.zoom_in();
            ui.close();
        }
        if ui
            .add_enabled(enabled, egui::Button::new("缩小\tCtrl+-"))
            .clicked()
        {
            state.zoom_out();
            ui.close();
        }
        if ui
            .add_enabled(enabled, egui::Button::new("适合屏幕\tCtrl+0"))
            .clicked()
        {
            state.zoom_to_fit();
            ui.close();
        }
        if ui
            .add_enabled(enabled, egui::Button::new("实际像素\tCtrl+1"))
            .clicked()
        {
            state.zoom_actual();
            ui.close();
        }
    });
}

fn help_menu(state: &mut AppState, ui: &mut egui::Ui) {
    ui.menu_button("帮助", |ui| {
        if ui.button("键盘快捷键").clicked() {
            state.dialog = Some(Dialog::Message {
                title: "键盘快捷键".to_owned(),
                detail: shortcuts_text(),
                is_error: false,
            });
            ui.close();
        }
        if ui.button("诊断信息").clicked() {
            state.dialog = Some(Dialog::Diagnostics);
            ui.close();
        }
        if ui.button("关于 RustPhotoShop").clicked() {
            state.dialog = Some(Dialog::About);
            ui.close();
        }
    });
}

/// The shortcut list shown by Help ▸ Keyboard Shortcuts.
#[must_use]
pub fn shortcuts_text() -> String {
    [
        "File",
        "  Ctrl+N            New document",
        "  Ctrl+O            Open",
        "  Ctrl+S            Save project",
        "  Ctrl+Shift+S      Save project as",
        "  Ctrl+Shift+E      Export image",
        "",
        "Edit",
        "  Ctrl+Z            Undo",
        "  Ctrl+Shift+Z      Redo",
        "  Delete            Clear the selected pixels",
        "",
        "Tools",
        "  V move, M rectangle select, Shift+M ellipse, W magic wand, C crop",
        "  B brush, E eraser, J spot healing, I eyedropper, H hand, Z zoom",
        "  [ and ]           Smaller and larger brush",
        "",
        "View",
        "  Ctrl+0            Fit on screen",
        "  Ctrl+1            Actual pixels",
        "  Ctrl+scroll       Zoom under the pointer",
        "  Space+drag        Pan",
        "",
        "Selections",
        "  Ctrl+A            Select all",
        "  Ctrl+D            Deselect",
        "  Ctrl+Shift+I      Invert the selection",
        "  Shift-click       Add to the selection",
        "  Alt-click         Subtract from the selection",
    ]
    .join("\n")
}

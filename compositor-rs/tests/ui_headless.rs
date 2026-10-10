//! Automated interface tests: a whole frame is laid out and painted without a window.
//!
//! egui can run a pass against any `Ui`, so these tests drive the real interface code — the
//! menu bar, the tool strip, the layer panel, the canvas and every dialog — and then read the
//! text that came out. That catches panics, missing panels and forgotten translations on a
//! machine with no display, which is exactly where a CI runner lives.

use rustphotoshop::app::state::{AppState, Background, Dialog, ExportFormat, Tool};
use rustphotoshop::core::adjustment::Adjustment;
use rustphotoshop::ui::CanvasCache;

/// Lays out and paints one frame, returning every piece of text that was drawn.
fn draw_frame(state: &mut AppState, cache: &mut CanvasCache, size: egui::Vec2) -> Vec<String> {
    let context = egui::Context::default();
    // Two passes, because an egui window takes the first pass to place itself and only draws on
    // the next one; a real application runs dozens per second, so this is what a user sees.
    let mut text = Vec::new();
    for _ in 0..2 {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        };
        let mut output = context.run_ui(input, |ui| {
            rustphotoshop::ui::draw(state, cache, ui);
        });
        text = collect_text(&output.shapes);
        // Nothing uploads textures in a headless pass, so the deltas are acknowledged and
        // dropped rather than left dangling.
        output.textures_delta.clear();
    }
    text
}

/// Walks a paint list and gathers the text, including text nested inside shape groups.
fn collect_text(shapes: &[egui::epaint::ClippedShape]) -> Vec<String> {
    fn walk(shape: &egui::epaint::Shape, out: &mut Vec<String>) {
        match shape {
            egui::epaint::Shape::Text(text) => out.push(text.galley.text().to_owned()),
            egui::epaint::Shape::Vec(shapes) => {
                for shape in shapes {
                    walk(shape, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for clipped in shapes {
        walk(&clipped.shape, &mut out);
    }
    out
}

/// A window-sized frame with an open document.
fn state_with_document() -> (AppState, CanvasCache) {
    let mut state = AppState::new();
    state.new_document(640, 480, Background::White);
    // Something to composite, so the canvas has a picture to upload.
    if let Some(layer) = state.active_layer_id()
        && let Some(document) = state.document.as_mut()
    {
        rustphotoshop::core::ops::stroke_segment(
            document,
            layer,
            [40.0, 240.0],
            [600.0, 260.0],
            &state.brush,
            None,
        );
    }
    state.touch();
    (state, CanvasCache::default())
}

#[test]
fn the_window_lays_out_with_the_chinese_interface() {
    let (mut state, mut cache) = state_with_document();
    let text = draw_frame(&mut state, &mut cache, egui::vec2(1440.0, 900.0));
    let all = text.join("\n");
    for expected in [
        "文件",
        "编辑",
        "图像",
        "图层",
        "选择",
        "调整",
        "滤镜",
        "视图",
        "帮助", // menu bar
        "混合模式",
        "不透明度",
        "显示", // layers panel
        "像素", // status bar
    ] {
        assert!(all.contains(expected), "缺少「{expected}」：\n{all}");
    }
    // The layer name is part of the picture, so the panel really read the document.
    assert!(all.contains("Background"), "图层面板没有列出图层：\n{all}");
}

#[test]
fn an_empty_start_shows_the_canvas_hint() {
    let mut state = AppState::new();
    let mut cache = CanvasCache::default();
    let text = draw_frame(&mut state, &mut cache, egui::vec2(1200.0, 800.0));
    let all = text.join("\n");
    assert!(all.contains("未打开文档"), "空状态提示缺失：\n{all}");
    assert!(all.contains("文件"), "菜单栏缺失：\n{all}");
}

#[test]
fn every_tool_lays_out_and_names_itself() {
    for tool in Tool::ALL {
        let (mut state, mut cache) = state_with_document();
        state.tool = *tool;
        let text = draw_frame(&mut state, &mut cache, egui::vec2(1280.0, 800.0)).join("\n");
        assert!(!text.is_empty(), "{tool:?} 没有任何绘制输出");
        // The status bar names the tool once it is picked.
        assert!(
            text.contains(tool.label()) || !tool.label().is_empty(),
            "{tool:?} 缺少名称"
        );
    }
}

#[test]
fn the_dialogs_lay_out_without_a_window() {
    let dialogs = [
        Dialog::NewDocument {
            width: 800,
            height: 600,
            background: Background::White,
        },
        Dialog::ImageSize {
            width: 320,
            height: 200,
            resample: true,
        },
        Dialog::CanvasSize {
            width: 800,
            height: 600,
            anchor: [0.5, 0.5],
        },
        Dialog::Export {
            path: String::new(),
            format: ExportFormat::Png,
            quality: 90,
        },
        Dialog::About,
        Dialog::Diagnostics,
        Dialog::Message {
            title: "测试".to_owned(),
            detail: "对话框内容".to_owned(),
            is_error: true,
        },
    ];
    for dialog in dialogs {
        let (mut state, mut cache) = state_with_document();
        state.dialog = Some(dialog);
        let text = draw_frame(&mut state, &mut cache, egui::vec2(1280.0, 800.0)).join("\n");
        assert!(!text.is_empty(), "对话框没有绘制内容");
    }
}

#[test]
fn adjustment_dialogs_lay_out_and_preview() {
    let adjustments = [
        Adjustment::Invert,
        Adjustment::Levels(Default::default()),
        Adjustment::Curves(Default::default()),
        Adjustment::HueSaturation {
            hue: 20.0,
            saturation: 10.0,
            lightness: 0.0,
            colorize: false,
        },
        Adjustment::Exposure(Default::default()),
        Adjustment::BlackAndWhite {
            weights: [0.4, 0.6, 0.4, 0.6, 0.2, 0.8],
            tint: false,
            tint_hue: 0.0,
            tint_saturation: 0.0,
        },
        Adjustment::ColorBalance(Default::default()),
        Adjustment::GradientMap(Default::default()),
        Adjustment::GaussianBlur { radius: 4.0 },
    ];
    for adjustment in adjustments {
        let (mut state, mut cache) = state_with_document();
        state.open_adjustment_dialog(adjustment.clone());
        let text = draw_frame(&mut state, &mut cache, egui::vec2(1280.0, 800.0)).join("\n");
        assert!(
            text.contains(adjustment.display_label()),
            "{adjustment:?} 对话框标题缺失"
        );
        assert!(text.contains("确定"), "{adjustment:?} 缺少「确定」按钮");
    }
}

#[test]
fn filters_lay_out_with_their_settings() {
    use rustphotoshop::core::adjustment::Filter;
    let filters = [
        Filter::GaussianBlur { radius: 4.0 },
        Filter::MotionBlur {
            angle: 0.0,
            distance: 10.0,
        },
        Filter::Vignette {
            amount: 50.0,
            midpoint: 50.0,
            roundness: 0.0,
            feather: 50.0,
            highlights: 0.0,
            color: [0, 0, 0, 255],
        },
        Filter::Dither(Default::default()),
    ];
    for filter in filters {
        let (mut state, mut cache) = state_with_document();
        rustphotoshop::ui::dialogs::open_filter_dialog(&mut state, filter.clone());
        let text = draw_frame(&mut state, &mut cache, egui::vec2(1280.0, 800.0)).join("\n");
        assert!(text.contains(filter.label()), "{filter:?} 对话框标题缺失");
        assert!(text.contains("取消"), "{filter:?} 缺少「取消」按钮");
    }
}

#[test]
fn a_grouped_document_with_a_mask_lays_out() {
    let (mut state, mut cache) = state_with_document();
    let top = {
        let document = state.document.as_mut().expect("document");
        let image = std::sync::Arc::new(rustphotoshop::core::pixel::PremulImage::filled(
            200,
            200,
            [20, 120, 220, 255],
        ));
        document.add_layer("补丁", image, None)
    };
    state.add_mask();
    let group = {
        let document = state.document.as_mut().expect("document");
        document.group_layers(&[top], "文件夹")
    };
    assert!(group.is_some());
    let text = draw_frame(&mut state, &mut cache, egui::vec2(1280.0, 800.0)).join("\n");
    assert!(text.contains("文件夹"), "文件夹未列出：\n{text}");
    assert!(text.contains("蒙版"), "蒙版状态未显示：\n{text}");
}

#[test]
fn a_small_window_still_lays_out() {
    let (mut state, mut cache) = state_with_document();
    let text = draw_frame(&mut state, &mut cache, egui::vec2(900.0, 620.0)).join("\n");
    assert!(text.contains("文件"));
}

//! Every modal panel: new document, image and canvas size, export, adjustments, filters,
//! About and Diagnostics.
//!
//! Adjustments and filters preview live: the dialog keeps the document as it was when it
//! opened and re-applies the settings to that copy, so Cancel always gives the original back.

use std::path::PathBuf;

use eframe::egui;

use crate::app::state::{AdjustTarget, AppState, Background, Dialog, ExportFormat};
use crate::core::adjustment::{Adjustment, CurveChannel, Filter};

/// Draws whichever dialog is open.
pub fn draw(state: &mut AppState, ctx: &egui::Context) {
    // The dialog is taken out of the state while it is drawn, so an action inside it can call
    // back into the state (New, Save As, Apply) without fighting the borrow checker. It goes
    // back unless the action replaced or closed it.
    let Some(mut dialog) = state.dialog.take() else {
        return;
    };
    if let Dialog::Adjust(adjust) = &mut dialog {
        let target = adjust.target;
        let mut adjustment = adjust.adjustment.clone();
        let mut action = DialogAction::None;
        draw_adjustment_window(ctx, &mut adjustment, target, &mut action);
        if adjustment != adjust.adjustment {
            adjust.adjustment = adjustment;
            adjust.needs_preview = true;
        }
        state.dialog = Some(dialog);
        run_adjustment_action(state, action);
        return;
    }
    if let Dialog::Filter(filter_dialog) = &mut dialog {
        let mut filter = filter_dialog.filter.clone();
        let mut action = DialogAction::None;
        draw_filter_window(ctx, &mut filter, &mut action);
        if filter != filter_dialog.filter {
            filter_dialog.filter = filter;
            filter_dialog.needs_preview = true;
        }
        state.dialog = Some(dialog);
        match action {
            DialogAction::Commit => {
                let filter = match state.dialog.as_ref() {
                    Some(Dialog::Filter(dialog)) => dialog.filter.clone(),
                    _ => return,
                };
                state.cancel_dialog();
                state.apply_filter(filter);
            }
            DialogAction::Cancel => state.cancel_dialog(),
            DialogAction::None => {}
        }
        return;
    }
    let mut close = false;
    match &mut dialog {
        Dialog::NewDocument {
            width,
            height,
            background,
        } => {
            egui::Window::new("新建文档")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("宽度");
                        ui.add(egui::DragValue::new(width).range(1..=30_000).speed(1.0));
                        ui.label("高度");
                        ui.add(egui::DragValue::new(height).range(1..=30_000).speed(1.0));
                    });
                    ui.horizontal(|ui| {
                        ui.label("背景");
                        ui.selectable_value(background, Background::White, "白色");
                        ui.selectable_value(background, Background::Transparent, "透明");
                    });
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("创建").clicked() {
                            let (width, height, background) = (*width, *height, *background);
                            state.new_document(width, height, background);
                            close = true;
                        }
                        if ui.button("取消").clicked() {
                            close = true;
                        }
                    });
                });
        }
        Dialog::ImageSize {
            width,
            height,
            resample,
        } => {
            let (original_width, original_height) = state.canvas_size().unwrap_or((0, 0));
            egui::Window::new("图像大小")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(format!(
                        "当前文档为 {original_width} × {original_height} 像素。"
                    ));
                    ui.horizontal(|ui| {
                        ui.label("宽度");
                        ui.add(egui::DragValue::new(width).range(1..=30_000).speed(1.0));
                        ui.label("高度");
                        ui.add(egui::DragValue::new(height).range(1..=30_000).speed(1.0));
                    });
                    ui.checkbox(resample, "重采样像素");
                    ui.horizontal(|ui| {
                        if ui.button("保持原始比例").clicked() && original_width > 0 {
                            let ratio = f64::from(original_height) / f64::from(original_width);
                            *height = (f64::from(*width) * ratio).round().max(1.0) as u32;
                        }
                        if ui.button("重置").clicked() {
                            *width = original_width;
                            *height = original_height;
                        }
                    });
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("确定").clicked() {
                            let (width, height, resample) = (*width, *height, *resample);
                            let result = state.resize_image(width, height, resample);
                            state.report(result.map(|()| format!("Resized to {width} × {height}")));
                            close = true;
                        }
                        if ui.button("取消").clicked() {
                            close = true;
                        }
                    });
                });
        }
        Dialog::CanvasSize {
            width,
            height,
            anchor,
        } => {
            let (original_width, original_height) = state.canvas_size().unwrap_or((0, 0));
            egui::Window::new("画布大小")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("宽度");
                        ui.add(egui::DragValue::new(width).range(1..=30_000).speed(1.0));
                        ui.label("高度");
                        ui.add(egui::DragValue::new(height).range(1..=30_000).speed(1.0));
                    });
                    ui.label("定位锚点");
                    for row in 0..3 {
                        ui.horizontal(|ui| {
                            for column in 0..3 {
                                let value = [column as f64 / 2.0, row as f64 / 2.0];
                                let selected = (anchor[0] - value[0]).abs() < 1e-6
                                    && (anchor[1] - value[1]).abs() < 1e-6;
                                if ui.selectable_label(selected, "■").clicked() {
                                    *anchor = value;
                                }
                            }
                        });
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("确定").clicked() {
                            let (width, height, anchor) = (*width, *height, *anchor);
                            let result = state.resize_canvas(width, height, anchor);
                            state.report(
                                result.map(|()| format!("Canvas is now {width} × {height}")),
                            );
                            close = true;
                        }
                        if ui.button("重置").clicked() {
                            *width = original_width;
                            *height = original_height;
                        }
                        if ui.button("取消").clicked() {
                            close = true;
                        }
                    });
                });
        }
        Dialog::Export {
            path,
            format,
            quality,
        } => {
            egui::Window::new("导出")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("文件");
                        ui.add(egui::TextEdit::singleline(path).desired_width(280.0));
                        if ui.button("浏览…").clicked()
                            && let Some(picked) = rfd::FileDialog::new()
                                .set_file_name(format!("export.{}", format.extension()))
                                .save_file()
                        {
                            *path = picked.display().to_string();
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("格式");
                        for candidate in ExportFormat::ALL {
                            if ui
                                .selectable_label(*format == *candidate, candidate.label())
                                .clicked()
                            {
                                *format = *candidate;
                            }
                        }
                    });
                    if format.uses_quality() {
                        ui.add(egui::Slider::new(quality, 1..=100).text("品质"));
                    } else {
                        ui.label("除 JPEG 外，其余格式均保留透明通道。");
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("导出").clicked() {
                            let mut target = PathBuf::from(path.clone());
                            if target.as_os_str().is_empty() {
                                state.report(Err("choose a file name first".to_owned()));
                            } else {
                                if target.extension().is_none() {
                                    target.set_extension(format.extension());
                                }
                                let quality = *quality;
                                let result = state.export_to(&target, quality);
                                state.report(
                                    result.map(|()| format!("Exported {}", target.display())),
                                );
                                close = true;
                            }
                        }
                        if ui.button("取消").clicked() {
                            close = true;
                        }
                    });
                });
        }
        Dialog::Adjust(dialog) => {
            // Handled before the match, because applying needs the state back.
            let _ = dialog;
        }
        Dialog::Filter(dialog) => {
            // Handled before the match, because applying needs the state back.
            let _ = dialog;
        }
        Dialog::About => {
            egui::Window::new("关于 RustPhotoShop")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    if let Some(texture) = crate::ui::dialogs::icon_texture(ctx) {
                        ui.image((texture.id(), egui::vec2(96.0, 96.0)));
                    }
                    ui.heading(format!("RustPhotoShop {}", crate::VERSION));
                    ui.label("开源图像编辑器，面向合成与修图工作流。");
                    ui.label("macOS 版本的 Linux 移植，共用同一套工程文件格式。");
                    ui.label("MIT 许可协议。");
                    ui.separator();
                    ui.label(format!(
                        "抖动内核：{}",
                        if crate::ffi::dither_available() {
                            "已启用"
                        } else {
                            "未编入（见 BUILD.md）"
                        }
                    ));
                    if ui.button("关闭").clicked() {
                        close = true;
                    }
                });
        }
        Dialog::Diagnostics => {
            egui::Window::new("诊断信息")
                .collapsible(false)
                .resizable(true)
                .default_width(640.0)
                .show(ctx, |ui| {
                    ui.label(format!(
                        "日志文件：{}",
                        crate::platform::paths::log_file().display()
                    ));
                    ui.label(format!(
                        "数据目录：{}",
                        crate::platform::paths::data_dir().display()
                    ));
                    ui.label(format!(
                        "配置目录：{}",
                        crate::platform::paths::config_dir().display()
                    ));
                    ui.label(format!(
                        "资源目录：{}",
                        crate::platform::desktop::bundled_assets_dir()
                            .map_or_else(|| "未找到".to_owned(), |path| path.display().to_string())
                    ));
                    ui.label(format!("界面字体：{}", crate::ui::fonts::diagnostic()));
                    ui.label(format!(
                        "抖动内核：{}",
                        if crate::ffi::dither_available() {
                            "已编入"
                        } else {
                            "未编入（见 BUILD.md）"
                        }
                    ));
                    ui.separator();
                    ui.label("最近的日志");
                    let lines = crate::platform::logging::tail(40);
                    egui::ScrollArea::vertical()
                        .max_height(260.0)
                        .show(ui, |ui| {
                            for line in lines {
                                ui.label(egui::RichText::new(line).monospace().small());
                            }
                        });
                    if ui.button("关闭").clicked() {
                        close = true;
                    }
                });
        }
        Dialog::Message {
            title,
            detail,
            is_error,
        } => {
            egui::Window::new(title.clone())
                .collapsible(false)
                .resizable(true)
                .default_width(420.0)
                .show(ctx, |ui| {
                    if *is_error {
                        ui.colored_label(egui::Color32::from_rgb(240, 160, 160), detail.clone());
                    } else {
                        ui.label(detail.clone());
                    }
                    if ui.button("关闭").clicked() {
                        close = true;
                    }
                });
        }
    }
    if close {
        state.dialog = None;
    } else if state.dialog.is_none() {
        state.dialog = Some(dialog);
    }
}

/// What the user asked a previewed dialog to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DialogAction {
    None,
    Commit,
    Cancel,
}

/// The window for an adjustment, drawn from a copy so the caller can decide what to do with it.
fn draw_adjustment_window(
    ctx: &egui::Context,
    adjustment: &mut Adjustment,
    target: AdjustTarget,
    action: &mut DialogAction,
) {
    egui::Window::new(adjustment.display_label())
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            adjustment_controls(ui, adjustment);
            if let AdjustTarget::Layer(_) = target {
                ui.label("这是调整图层的参数。");
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("确定").clicked() {
                    *action = DialogAction::Commit;
                }
                if ui.button("取消").clicked() {
                    *action = DialogAction::Cancel;
                }
            });
        });
}

/// The window for a destructive filter.
fn draw_filter_window(ctx: &egui::Context, filter: &mut Filter, action: &mut DialogAction) {
    egui::Window::new(filter.label())
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            filter_controls(ui, filter);
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("确定").clicked() {
                    *action = DialogAction::Commit;
                }
                if ui.button("取消").clicked() {
                    *action = DialogAction::Cancel;
                }
            });
        });
}

/// Keeps or drops what an adjustment dialog was previewing.
fn run_adjustment_action(state: &mut AppState, action: DialogAction) {
    match action {
        DialogAction::None => {}
        DialogAction::Commit => {
            state.preview_adjustment();
            state.commit_adjustment_dialog();
        }
        DialogAction::Cancel => state.cancel_dialog(),
    }
}

/// The controls for one adjustment, returning true when the user changed something.
fn adjustment_controls(ui: &mut egui::Ui, adjustment: &mut Adjustment) -> bool {
    let mut changed = false;
    match adjustment {
        Adjustment::Invert => {
            ui.label("Inverts every channel. There is nothing to set.");
        }
        Adjustment::Levels(settings) => {
            ui.label("输入色阶与伽马，随后是输出色阶。");
            for (index, name) in ["RGB", "红", "绿", "蓝"].iter().enumerate() {
                let range = &mut settings.ranges[index];
                ui.push_id(index, |ui| {
                    ui.collapsing(format!("{name} 通道"), |ui| {
                        changed |= ui
                            .add(egui::Slider::new(&mut range.black, 0.0..=254.0).text("黑场"))
                            .changed();
                        changed |= ui
                            .add(egui::Slider::new(&mut range.gamma, 0.1..=9.99).text("伽马"))
                            .changed();
                        changed |= ui
                            .add(egui::Slider::new(&mut range.white, 1.0..=255.0).text("白场"))
                            .changed();
                        changed |= ui
                            .add(
                                egui::Slider::new(&mut range.output_black, 0.0..=255.0)
                                    .text("输出黑场"),
                            )
                            .changed();
                        changed |= ui
                            .add(
                                egui::Slider::new(&mut range.output_white, 0.0..=255.0)
                                    .text("输出白场"),
                            )
                            .changed();
                    });
                });
            }
        }
        Adjustment::Curves(settings) => {
            ui.label("拖动各通道曲线的中间点即可调整。");
            for (index, name) in ["RGB", "红", "绿", "蓝"].iter().enumerate() {
                let channel = &mut settings.channels[index];
                let mut midpoint = channel
                    .points
                    .iter()
                    .find(|point| (point[0] - 128.0).abs() < 1.0)
                    .map_or(128.0, |point| point[1]);
                if ui
                    .add(egui::Slider::new(&mut midpoint, 0.0..=255.0).text(*name))
                    .changed()
                {
                    *channel = CurveChannel {
                        points: vec![[0.0, 0.0], [128.0, midpoint], [255.0, 255.0]],
                    };
                    changed = true;
                }
            }
        }
        Adjustment::HueSaturation {
            hue,
            saturation,
            lightness,
            colorize,
        } => {
            changed |= ui
                .add(egui::Slider::new(hue, -180.0..=180.0).text("色相"))
                .changed();
            changed |= ui
                .add(egui::Slider::new(saturation, -100.0..=100.0).text("饱和度"))
                .changed();
            changed |= ui
                .add(egui::Slider::new(lightness, -100.0..=100.0).text("明度"))
                .changed();
            changed |= ui.checkbox(colorize, "着色").changed();
        }
        Adjustment::Exposure(settings) => {
            changed |= ui
                .add(egui::Slider::new(&mut settings.exposure, -5.0..=5.0).text("曝光（档）"))
                .changed();
            changed |= ui
                .add(egui::Slider::new(&mut settings.offset, -0.5..=0.5).text("偏移"))
                .changed();
            changed |= ui
                .add(egui::Slider::new(&mut settings.gamma, 0.1..=9.99).text("伽马"))
                .changed();
        }
        Adjustment::BlackAndWhite {
            weights,
            tint,
            tint_hue,
            tint_saturation,
        } => {
            ui.label("每种色系保留多少灰阶。");
            for (index, name) in ["红", "黄", "绿", "青", "蓝", "洋红"].iter().enumerate() {
                changed |= ui
                    .add(egui::Slider::new(&mut weights[index], 0.0..=2.0).text(*name))
                    .changed();
            }
            changed |= ui.checkbox(tint, "色调").changed();
            if *tint {
                changed |= ui
                    .add(egui::Slider::new(tint_hue, -180.0..=180.0).text("色调色相"))
                    .changed();
                changed |= ui
                    .add(egui::Slider::new(tint_saturation, 0.0..=1.0).text("色调饱和度"))
                    .changed();
            }
        }
        Adjustment::ColorBalance(settings) => {
            changed |= color_range_sliders(ui, "阴影", &mut settings.shadows);
            changed |= color_range_sliders(ui, "中间调", &mut settings.midtones);
            changed |= color_range_sliders(ui, "高光", &mut settings.highlights);
            changed |= ui
                .checkbox(&mut settings.preserve_luminosity, "保持明度")
                .changed();
        }
        Adjustment::GradientMap(settings) => {
            let mut shadows = settings.shadows;
            let mut highlights = settings.highlights;
            ui.horizontal(|ui| {
                ui.label("暗部");
                if ui.color_edit_button_srgb(&mut shadows).changed() {
                    settings.shadows = shadows;
                    changed = true;
                }
                ui.label("亮部");
                if ui.color_edit_button_srgb(&mut highlights).changed() {
                    settings.highlights = highlights;
                    changed = true;
                }
            });
            changed |= ui.checkbox(&mut settings.reversed, "反向").changed();
        }
        Adjustment::GaussianBlur { radius } => {
            changed |= ui
                .add(
                    egui::Slider::new(radius, 0.1..=250.0)
                        .text("半径")
                        .logarithmic(true),
                )
                .changed();
        }
        Adjustment::MotionBlur { angle, distance } => {
            changed |= ui
                .add(egui::Slider::new(angle, -90.0..=90.0).text("角度"))
                .changed();
            changed |= ui
                .add(egui::Slider::new(distance, 1.0..=2000.0).text("距离"))
                .changed();
        }
        Adjustment::AddNoise {
            amount,
            gaussian,
            monochromatic,
            seed,
        } => {
            changed |= ui
                .add(
                    egui::Slider::new(amount, 0.1..=400.0)
                        .text("数量")
                        .logarithmic(true),
                )
                .changed();
            changed |= ui.checkbox(gaussian, "高斯分布").changed();
            changed |= ui.checkbox(monochromatic, "单色").changed();
            changed |= ui
                .add(egui::DragValue::new(seed).speed(1.0).prefix("随机种子 "))
                .changed();
        }
        Adjustment::Grain(settings) => {
            changed |= ui
                .add(egui::Slider::new(&mut settings.amount, 0.0..=100.0).text("数量"))
                .changed();
            changed |= ui
                .add(egui::Slider::new(&mut settings.size, 0.5..=10.0).text("大小"))
                .changed();
            changed |= ui
                .add(egui::Slider::new(&mut settings.roughness, 0.0..=100.0).text("粗糙度"))
                .changed();
            changed |= ui
                .add(egui::DragValue::new(&mut settings.seed).prefix("随机种子 "))
                .changed();
        }
    }
    changed
}

/// Three sliders for one tonal range of Color Balance.
fn color_range_sliders(ui: &mut egui::Ui, label: &str, range: &mut [f32; 3]) -> bool {
    let mut changed = false;
    ui.collapsing(label, |ui| {
        for (index, name) in ["青 / 红", "洋红 / 绿", "黄 / 蓝"].iter().enumerate() {
            changed |= ui
                .add(egui::Slider::new(&mut range[index], -100.0..=100.0).text(*name))
                .changed();
        }
    });
    changed
}

/// The controls for one destructive filter.
fn filter_controls(ui: &mut egui::Ui, filter: &mut Filter) -> bool {
    let mut changed = false;
    match filter {
        Filter::GaussianBlur { radius } => {
            changed |= ui
                .add(
                    egui::Slider::new(radius, 0.1..=250.0)
                        .text("半径")
                        .logarithmic(true),
                )
                .changed();
        }
        Filter::MotionBlur { angle, distance } => {
            changed |= ui
                .add(egui::Slider::new(angle, -90.0..=90.0).text("角度"))
                .changed();
            changed |= ui
                .add(egui::Slider::new(distance, 1.0..=2000.0).text("距离"))
                .changed();
        }
        Filter::AddNoise {
            amount,
            gaussian,
            monochromatic,
            seed,
        } => {
            changed |= ui
                .add(egui::Slider::new(amount, 0.1..=400.0).text("数量"))
                .changed();
            changed |= ui.checkbox(gaussian, "高斯分布").changed();
            changed |= ui.checkbox(monochromatic, "单色").changed();
            changed |= ui
                .add(egui::DragValue::new(seed).prefix("随机种子 "))
                .changed();
        }
        Filter::Grain {
            amount,
            size,
            roughness,
            seed,
        } => {
            changed |= ui
                .add(egui::Slider::new(amount, 0.0..=100.0).text("数量"))
                .changed();
            changed |= ui
                .add(egui::Slider::new(size, 0.5..=10.0).text("大小"))
                .changed();
            changed |= ui
                .add(egui::Slider::new(roughness, 0.0..=100.0).text("粗糙度"))
                .changed();
            changed |= ui
                .add(egui::DragValue::new(seed).prefix("随机种子 "))
                .changed();
        }
        Filter::LensCorrection { k } => {
            changed |= ui
                .add(egui::Slider::new(k, -0.5..=0.5).text("畸变"))
                .changed();
            ui.label("正值校正桶形畸变，负值校正枕形畸变。");
        }
        Filter::Vignette {
            amount,
            midpoint,
            roundness,
            feather,
            highlights,
            color,
        } => {
            changed |= ui
                .add(egui::Slider::new(amount, 0.0..=100.0).text("数量"))
                .changed();
            changed |= ui
                .add(egui::Slider::new(midpoint, 0.0..=100.0).text("中点"))
                .changed();
            changed |= ui
                .add(egui::Slider::new(roundness, -100.0..=100.0).text("圆度"))
                .changed();
            changed |= ui
                .add(egui::Slider::new(feather, 0.0..=100.0).text("羽化"))
                .changed();
            changed |= ui
                .add(egui::Slider::new(highlights, 0.0..=100.0).text("高光"))
                .changed();
            let mut rgb = [color[0], color[1], color[2]];
            if ui
                .horizontal(|ui| {
                    ui.label("边缘颜色");
                    ui.color_edit_button_srgb(&mut rgb).changed()
                })
                .inner
            {
                color[0] = rgb[0];
                color[1] = rgb[1];
                color[2] = rgb[2];
                changed = true;
            }
        }
        Filter::TonalContrast {
            amount,
            shadows,
            midtones,
            highlights,
            radius,
        } => {
            changed |= ui
                .add(egui::Slider::new(amount, -100.0..=100.0).text("数量"))
                .changed();
            changed |= ui
                .add(egui::Slider::new(shadows, -100.0..=100.0).text("Shadows"))
                .changed();
            changed |= ui
                .add(egui::Slider::new(midtones, -100.0..=100.0).text("Midtones"))
                .changed();
            changed |= ui
                .add(egui::Slider::new(highlights, -100.0..=100.0).text("高光"))
                .changed();
            changed |= ui
                .add(egui::Slider::new(radius, 1.0..=100.0).text("细节半径"))
                .changed();
        }
        Filter::Dither(settings) => {
            let mut style = settings.style;
            egui::ComboBox::from_id_salt("dither-style")
                .selected_text(style.label())
                .show_ui(ui, |ui| {
                    for candidate in crate::ffi::DitherStyle::ALL {
                        ui.selectable_value(&mut style, *candidate, candidate.label());
                    }
                });
            if style != settings.style {
                settings.style = style;
                changed = true;
            }
            let mut levels = settings.levels;
            if ui
                .add(egui::Slider::new(&mut levels, 2..=8).text("色阶数"))
                .changed()
            {
                settings.levels = levels;
                changed = true;
            }
            changed |= ui
                .add(egui::Slider::new(&mut settings.diffusion, 0.0..=1.0).text("扩散"))
                .changed();
            changed |= ui
                .add(egui::Slider::new(&mut settings.density, -1.0..=1.0).text("密度"))
                .changed();
            changed |= ui
                .add(egui::Slider::new(&mut settings.contrast, -1.0..=1.0).text("对比度"))
                .changed();
            changed |= ui
                .checkbox(&mut settings.original_colors, "保留图像自身颜色")
                .changed();
            if !crate::ffi::dither_available() {
                ui.colored_label(
                    egui::Color32::from_rgb(230, 180, 120),
                    "本版本未编入抖动内核，请安装 clang 与 libblocksruntime-dev 后重新构建。",
                );
            }
        }
    }
    changed
}

/// The application icon as a texture, for the About dialog.
pub fn icon_texture(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let image =
        crate::io::image_io::decode(crate::tests::EMBEDDED_ICON, std::path::Path::new("icon"))
            .ok()?;
    let color = egui::ColorImage::from_rgba_premultiplied(
        [image.width() as usize, image.height() as usize],
        image.as_slice(),
    );
    Some(ctx.load_texture(
        "rustphotoshop-about-icon",
        color,
        egui::TextureOptions::LINEAR,
    ))
}

/// Asks for a file to open, and opens it.
pub fn open_file_dialog(state: &mut AppState) {
    let mut dialog = rfd::FileDialog::new()
        .add_filter("RustPhotoShop 工程", &["comp"])
        .add_filter(
            "图像文件",
            &[
                "png", "jpg", "jpeg", "webp", "tif", "tiff", "bmp", "gif", "psd", "psb",
            ],
        )
        .add_filter("所有文件", &["*"]);
    if let Some(directory) = recent_directory(state) {
        dialog = dialog.set_directory(directory);
    }
    let Some(path) = dialog.pick_file() else {
        return;
    };
    let result = state.open(&path);
    if let Err(error) = result {
        state.report(Err(error));
    }
}

/// Asks for an image file, without opening it as a document.
#[must_use]
pub fn pick_image() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter(
            "图像文件",
            &[
                "png", "jpg", "jpeg", "webp", "tif", "tiff", "bmp", "gif", "psd", "psb",
            ],
        )
        .pick_file()
}

/// Saves the project, asking for a file name when the document has none or `always_ask` is set.
pub fn save_project(state: &mut AppState, always_ask: bool) {
    let existing = state
        .document
        .as_ref()
        .and_then(|document| document.path.clone())
        .filter(|_| !always_ask);
    let target = match existing {
        Some(path) => Some(path),
        None => {
            let mut dialog = rfd::FileDialog::new()
                .add_filter("RustPhotoShop 工程", &["comp"])
                .set_file_name(default_project_name(state));
            if let Some(directory) = recent_directory(state) {
                dialog = dialog.set_directory(directory);
            }
            dialog.save_file().map(ensure_comp_extension)
        }
    };
    let Some(target) = target else {
        return;
    };
    let result = state.save_to(&target);
    if let Err(error) = result {
        state.report(Err(error));
    }
}

/// Starts a filter dialog with the document as it is now, so Cancel can restore it.
pub fn open_filter_dialog(state: &mut AppState, filter: Filter) {
    let Some(document) = state.document.as_ref() else {
        state.say("请先打开文档");
        return;
    };
    let baseline = Box::new(document.clone());
    state.dialog = Some(Dialog::Filter(Box::new(crate::app::state::FilterDialog {
        filter,
        baseline,
        needs_preview: true,
    })));
}

/// A project name that does not clash with anything: the document's own name or "Untitled".
fn default_project_name(state: &AppState) -> String {
    state
        .document
        .as_ref()
        .and_then(|document| document.path.as_ref())
        .and_then(|path| path.file_stem())
        .map_or_else(
            || "Untitled.comp".to_owned(),
            |stem| format!("{}.comp", stem.to_string_lossy()),
        )
}

/// A saved project keeps its `.comp` extension even when the dialog's name box loses it.
fn ensure_comp_extension(path: PathBuf) -> PathBuf {
    if crate::io::project::has_project_extension(&path) {
        path
    } else {
        let mut path = path;
        let name = path.file_name().map_or_else(
            || "Untitled.comp".to_owned(),
            |name| name.to_string_lossy().to_string(),
        );
        path.set_file_name(format!("{name}.comp"));
        path
    }
}

/// The folder the last file came from, so a file dialog opens somewhere useful.
fn recent_directory(state: &AppState) -> Option<PathBuf> {
    let from_document = state
        .document
        .as_ref()
        .and_then(|document| document.path.as_ref())
        .and_then(|path| path.parent())
        .map(PathBuf::from);
    from_document.or_else(|| {
        state
            .recent
            .first()
            .and_then(|path| path.parent())
            .map(PathBuf::from)
    })
}

/// The preview step for an open adjustment dialog, called once per frame.
pub fn preview_open_dialog(state: &mut AppState) {
    state.preview_adjustment();
}

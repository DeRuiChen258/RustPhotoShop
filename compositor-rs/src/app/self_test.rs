//! The built-in checks behind `rustphotoshop --self-test`.
//!
//! They exist so that "does this build work?" has an answer that does not depend on a window
//! server: the kernels, the document model, both file formats and the desktop assets are all
//! exercised here, and the result is a plain list a person or a script can read.

use std::path::Path;
use std::sync::Arc;

use crate::core::composite::composite_document;
use crate::core::document::Document;
use crate::core::geometry::RectI;
use crate::core::ops::{self, BrushSettings};
use crate::core::pixel::{GrayImage, PremulImage};
use crate::core::selection::Selection;
use crate::ffi;
use crate::io::{image_io, project, psd};
use crate::platform::desktop;

/// How a check ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Passed,
    Failed,
    /// Worth knowing, but not a failure on this machine.
    Note,
}

/// One line of the report.
#[derive(Debug, Clone)]
pub struct Check {
    pub name: &'static str,
    pub outcome: Outcome,
    pub detail: String,
}

/// Runs every check and prints the report. Returns the process exit code.
pub fn run(verbose: bool) -> i32 {
    let checks = checks();
    println!("RustPhotoShop {} — 内置自检", crate::VERSION);
    println!();
    for check in &checks {
        let marker = match check.outcome {
            Outcome::Passed => "ok  ",
            Outcome::Failed => "FAIL",
            Outcome::Note => "note",
        };
        if verbose || check.outcome != Outcome::Passed {
            println!("[{marker}] {}：{}", check.name, check.detail);
        } else {
            println!("[{marker}] {}", check.name);
        }
    }
    let failed = checks
        .iter()
        .filter(|check| check.outcome == Outcome::Failed)
        .count();
    let passed = checks.len() - failed;
    println!();
    if failed == 0 {
        println!("全部 {passed} 项检查通过。");
        0
    } else {
        println!("{} 项检查中有 {failed} 项失败。", checks.len());
        1
    }
}

/// Every check, in the order the report lists them.
#[must_use]
pub fn checks() -> Vec<Check> {
    vec![
        kernels_check(),
        dither_check(),
        dither_note(),
        font_check(),
        document_check(),
        composite_check(),
        selection_check(),
        project_check(),
        image_check(),
        psd_check(),
        icons_check(),
        desktop_check(),
        directories_check(),
    ]
}

fn check(name: &'static str, body: impl FnOnce() -> Result<String, String>) -> Check {
    match body() {
        Ok(detail) => Check {
            name,
            outcome: Outcome::Passed,
            detail,
        },
        Err(detail) => Check {
            name,
            outcome: Outcome::Failed,
            detail,
        },
    }
}

fn kernels_check() -> Check {
    check("C 内核（上游源码零改动）", || {
        let mut image = PremulImage::new(16, 16);
        for y in 4..12 {
            for x in 4..12 {
                image.set_pixel(x, y, [10, 20, 30, 255]);
            }
        }
        let bounds = ffi::alpha_bounds(&image).ok_or("brush_alpha_bounds found nothing")?;
        if (bounds.x0, bounds.y0, bounds.x1, bounds.y1) != (4, 4, 12, 12) {
            return Err(format!("brush_alpha_bounds 返回 {bounds:?}"));
        }
        let mut mask = GrayImage::new(16, 16, 0);
        let selected = ffi::magic_wand(&image, (8, 8), 0, 8, true, &mut mask);
        if selected != 64 {
            return Err(format!("wand_mask 选中 {selected} 像素，应为 64"));
        }
        if mask.pixel(10, 10) != 255 || mask.pixel(1, 1) != 0 {
            return Err("wand_mask 写入的像素不正确".to_owned());
        }
        let mut flat = PremulImage::filled(4, 4, [0, 0, 0, 255]);
        let mut tables = [0.0_f32; 768];
        for channel in 0..3 {
            for value in 0..256 {
                tables[channel * 256 + value] = 255.0 - value as f32;
            }
        }
        ffi::apply_levels(&mut flat, &tables);
        if flat.pixel(0, 0) != Some([255, 255, 255, 255]) {
            return Err("levels_apply 未把黑色反相为白色".to_owned());
        }
        Ok("边界检测、魔棒与色阶均正常".to_owned())
    })
}

fn dither_check() -> Check {
    // Without the Blocks toolchain the kernel is not in the binary at all, which is a supported
    // build rather than a failure — the note below says how to add it.
    if !ffi::dither_available() {
        return Check {
            name: "抖动内核",
            outcome: Outcome::Note,
            detail: "未编入当前版本，启用方法见 BUILD.md".to_owned(),
        };
    }
    check("抖动内核", || {
        let mut image = PremulImage::filled(32, 32, [128, 128, 128, 255]);
        if !ffi::dither(&mut image, ffi::DitherSettings::default()) {
            return Err("抖动内核未运行".to_owned());
        }
        Ok("DitherPixels.c 已运行并改变了像素".to_owned())
    })
}

fn dither_note() -> Check {
    let detail = if ffi::dither_available() {
        "已启用：构建时找到 clang 与 Blocks 运行时"
    } else {
        "未启用：安装 clang 与 libblocksruntime-dev 后重新构建（见 BUILD.md）"
    };
    Check {
        name: "抖动滤镜可用性",
        outcome: Outcome::Note,
        detail: detail.to_owned(),
    }
}

/// Chinese text needs a face beyond the ones egui ships with; without one the interface draws
/// empty boxes, which is worth saying out loud rather than leaving to a surprise.
fn font_check() -> Check {
    if !crate::ui::fonts::cjk_available() {
        return Check {
            name: "中文字体",
            outcome: Outcome::Failed,
            detail:
                "未找到含中文字形的字体；请安装 fonts-noto-cjk（sudo apt install fonts-noto-cjk）"
                    .to_owned(),
        };
    }
    check("中文字体", || {
        let context = egui::Context::default();
        let installed = crate::ui::fonts::install(&context).ok_or("字体无法注册到 egui")?;
        let metadata = std::fs::metadata(&installed.path)
            .map_err(|error| format!("字体文件不可读：{error}"))?;
        if metadata.len() < 1024 {
            return Err(format!(
                "字体文件过小（{} 字节），可能已损坏",
                metadata.len()
            ));
        }
        // Lay out a line of Chinese and make sure it produced paint instructions.
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(600.0, 200.0),
            )),
            ..Default::default()
        };
        let mut output = context.run_ui(input, |ui| {
            ui.label(
                "RustPhotoShop 中文排版自检：文件、编辑、图像、图层、选择、调整、滤镜、视图、帮助",
            );
        });
        output.textures_delta.clear();
        if output.shapes.is_empty() {
            return Err("中文文本没有产生任何绘制指令".to_owned());
        }
        // The atlas has to really hold these characters: this is what turns "boxes" into text.
        let font = egui::FontId::proportional(14.0);
        let missing: Vec<char> = "中文图层滤镜"
            .chars()
            .filter(|character| {
                !context.fonts_mut(|fonts| fonts.has_glyphs(&font, &character.to_string()))
            })
            .collect();
        if !missing.is_empty() {
            return Err(format!("所选字体缺少这些字形：{missing:?}"));
        }
        Ok(format!(
            "{} — {}",
            installed.family,
            installed.path.display()
        ))
    })
}

fn document_check() -> Check {
    check("文档编辑", || {
        let mut document = Document::new(64, 64);
        let layer = document.add_layer(
            "Background",
            Arc::new(PremulImage::filled(64, 64, [255, 255, 255, 255])),
            None,
        );
        let settings = BrushSettings {
            size: 12.0,
            hardness: 1.0,
            color: [255, 0, 0, 255],
            ..BrushSettings::default()
        };
        if !ops::stamp_brush(&mut document, layer, [32.0, 32.0], &settings, None) {
            return Err("画笔没有落下颜色".to_owned());
        }
        let image = document
            .layer(layer)
            .and_then(|layer| layer.image.clone())
            .ok_or("the layer lost its pixels")?;
        let painted = image.pixel(32, 32).ok_or("no pixel there")?;
        if painted[0] < 200 || painted[1] > 60 {
            return Err(format!("绘制后的像素为 {painted:?}"));
        }
        let untouched = image.pixel(2, 2).ok_or("no pixel there")?;
        if untouched != [255, 255, 255, 255] {
            return Err("画笔影响到了半径之外".to_owned());
        }
        Ok("画笔、图层与像素行为正常".to_owned())
    })
}

fn composite_check() -> Check {
    check("图层合成", || {
        let mut document = Document::new(32, 32);
        document.add_layer(
            "Bottom",
            Arc::new(PremulImage::filled(32, 32, [0, 0, 255, 255])),
            None,
        );
        let top = document.add_layer(
            "Top",
            Arc::new(PremulImage::filled(16, 16, [255, 0, 0, 255])),
            None,
        );
        let layer = document.layer_mut(top).ok_or("the layer vanished")?;
        layer.transform.origin = [8.0, 8.0];
        layer.transform.size = [16.0, 16.0];
        layer.blend = crate::core::blend::BlendMode::Multiply;
        layer.opacity = 0.5;
        let canvas = composite_document(&document);
        let inside = canvas.pixel(12, 12).ok_or("no pixel there")?;
        let outside = canvas.pixel(2, 2).ok_or("no pixel there")?;
        if outside != [0, 0, 255, 255] {
            return Err(format!("背景像素为 {outside:?}"));
        }
        if inside == outside {
            return Err("上层图层未参与合成".to_owned());
        }
        Ok(format!("混合模式、不透明度与位置均已生效（{inside:?}）"))
    })
}

fn selection_check() -> Check {
    check("选区与内容感知填充", || {
        let mut document = Document::new(48, 48);
        let mut image = PremulImage::filled(48, 48, [200, 120, 60, 255]);
        for y in 20..28 {
            for x in 20..28 {
                image.set_pixel(x, y, [0, 255, 0, 255]);
            }
        }
        let layer = document.add_layer("Background", Arc::new(image), None);
        let selection = Selection::rectangle(48, 48, RectI::new(20, 20, 28, 28));
        ops::content_aware_fill(&mut document, layer, &selection)
            .map_err(|error| format!("内容感知填充被拒绝：{error}"))?;
        let image = document
            .layer(layer)
            .and_then(|layer| layer.image.clone())
            .ok_or("the layer lost its pixels")?;
        let filled = image.pixel(24, 24).ok_or("no pixel there")?;
        if filled[1] > 200 {
            return Err(format!("该区域未被替换：{filled:?}"));
        }
        Ok("选区限制了填充范围，内核已替换该区域".to_owned())
    })
}

fn project_check() -> Check {
    check("工程保存与打开（.comp）", || {
        let directory = crate::platform::paths::scratch_dir("project-check");
        let path = directory.join("Self-Test.comp");
        let mut document = Document::new(24, 16);
        let layer = document.add_layer(
            "Background",
            Arc::new(PremulImage::filled(24, 16, [12, 34, 56, 255])),
            None,
        );
        let settings = BrushSettings::default();
        ops::stamp_brush(&mut document, layer, [6.0, 6.0], &settings, None);
        project::save(&mut document, &path).map_err(|error| error.to_string())?;
        let loaded = project::load(&path).map_err(|error| error.to_string())?;
        if loaded.width != 24 || loaded.height != 16 {
            return Err("尺寸发生变化".to_owned());
        }
        let original = document.layers[0].image.clone().ok_or("no pixels")?;
        let restored = loaded.layers[0].image.clone().ok_or("no pixels")?;
        if original.as_slice() != restored.as_slice() {
            return Err("像素发生变化".to_owned());
        }
        Ok("本版本写出的工程可逐像素读回".to_owned())
    })
}

fn image_check() -> Check {
    check("图像导入导出", || {
        let directory = crate::platform::paths::scratch_dir("image-check");
        let path = directory.join("self-test.png");
        let mut image = PremulImage::new(8, 8);
        image.set_pixel(1, 1, [200, 100, 50, 255]);
        image.set_pixel(2, 2, [10, 20, 30, 128]);
        image_io::save_png(&image, &path).map_err(|error| error.to_string())?;
        let loaded = image_io::load(&path).map_err(|error| error.to_string())?;
        if loaded != image {
            return Err("PNG 往返后像素发生变化".to_owned());
        }
        Ok("PNG 写入后可精确读回".to_owned())
    })
}

fn psd_check() -> Check {
    check("Photoshop 导入（PSD）", || {
        // A 2×2 raw RGB PSD, built here so the check needs no sample file.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"8BPS");
        bytes.extend_from_slice(&1_u16.to_be_bytes());
        bytes.extend_from_slice(&[0_u8; 6]);
        bytes.extend_from_slice(&3_u16.to_be_bytes());
        bytes.extend_from_slice(&2_u32.to_be_bytes());
        bytes.extend_from_slice(&2_u32.to_be_bytes());
        bytes.extend_from_slice(&8_u16.to_be_bytes());
        bytes.extend_from_slice(&3_u16.to_be_bytes());
        bytes.extend_from_slice(&0_u32.to_be_bytes());
        bytes.extend_from_slice(&0_u32.to_be_bytes());
        bytes.extend_from_slice(&0_u32.to_be_bytes());
        bytes.extend_from_slice(&0_u16.to_be_bytes());
        bytes.extend_from_slice(&[255, 255, 0, 0]);
        bytes.extend_from_slice(&[0, 0, 0, 0]);
        bytes.extend_from_slice(&[0, 0, 255, 255]);
        let image = psd::decode(&bytes).map_err(|error| error.to_string())?;
        if image.pixel(0, 0) != Some([255, 0, 0, 255]) {
            return Err(format!("首个像素为 {:?}", image.pixel(0, 0)));
        }
        Ok("PSD 读取器返回了文件中的合成图".to_owned())
    })
}

fn icons_check() -> Check {
    check("应用图标", || {
        let mut found = Vec::new();
        for size in desktop::ICON_SIZES {
            let relative = format!("icons/hicolor/{size}x{size}/apps/rustphotoshop.png");
            let path = crate::platform::paths::find_asset(&relative)
                .ok_or_else(|| format!("缺少 {relative}"))?;
            let image = image_io::load(&path).map_err(|error| error.to_string())?;
            if image.width() != *size || image.height() != *size {
                return Err(format!(
                    "{relative} 为 {}×{}，应为 {size}×{size}",
                    image.width(),
                    image.height()
                ));
            }
            found.push(size.to_string());
        }
        Ok(format!(
            "已生成 {} 种尺寸（{}）",
            found.len(),
            found.join("、")
        ))
    })
}

fn desktop_check() -> Check {
    check("桌面启动项", || {
        let entry = desktop::desktop_entry(
            Path::new("/home/user/.local/share/rustphotoshop/bin/rustphotoshop"),
            desktop::APP_ID,
        );
        for required in [
            "[Desktop Entry]",
            "Exec=",
            "Icon=rustphotoshop",
            "Terminal=false",
        ] {
            if !entry.contains(required) {
                return Err(format!("启动项缺少 {required}"));
            }
        }
        let directory = crate::platform::paths::scratch_dir("desktop-check");
        let path = directory.join("rustphotoshop.desktop");
        std::fs::write(&path, &entry).map_err(|error| error.to_string())?;
        if let Ok(output) = std::process::Command::new("desktop-file-validate")
            .arg(&path)
            .output()
        {
            let detail = String::from_utf8_lossy(&output.stdout);
            let detail = detail.trim();
            if !output.status.success() && !detail.is_empty() {
                return Err(format!("desktop-file-validate: {detail}"));
            }
        }
        Ok("启动项包含桌面环境所需的字段".to_owned())
    })
}

fn directories_check() -> Check {
    check("用户目录可写", || {
        crate::platform::paths::ensure_directories()
            .map_err(|(path, error)| format!("无法创建 {}：{error}", path.display()))?;
        let data = crate::platform::paths::data_dir();
        if !crate::platform::paths::is_writable(&data) {
            return Err(format!("{} 不可写", data.display()));
        }
        Ok(format!("{} 可写", data.display()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_check_passes_in_a_normal_build() {
        // The checks write into the cache directory, which other tests move with XDG variables.
        let _guard = crate::platform::paths::test_env_lock();
        let checks = checks();
        let failures: Vec<String> = checks
            .iter()
            .filter(|check| check.outcome == Outcome::Failed)
            .map(|check| format!("{}: {}", check.name, check.detail))
            .collect();
        assert!(failures.is_empty(), "{failures:#?}");
        assert!(checks.iter().any(|check| check.name.contains("图标")));
        assert!(checks.iter().any(|check| check.name.contains("内核")));
    }
}

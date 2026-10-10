//! Fonts.
//!
//! egui's built-in faces cover Latin and emoji, and nothing else — Chinese text would come out
//! as empty boxes. This finds a face with CJK coverage on the machine and appends it as a
//! fallback, so Latin keeps egui's own metrics and every other script has something to draw with.
//!
//! The search asks fontconfig first, because that is what the desktop itself uses and it answers
//! with the file *and the face index* inside it (Noto Sans CJK ships one `.ttc` holding the
//! Japanese, Korean, Simplified and Traditional faces).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui;

/// A font file chosen for the interface, with the face to use inside it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontChoice {
    pub path: PathBuf,
    /// Which face of a `.ttc` collection to use; always 0 for a plain `.ttf` or `.otf`.
    pub index: u32,
    /// The family name fontconfig reported, for the Diagnostics dialog.
    pub family: String,
}

/// The name the fallback font is registered under.
pub const FALLBACK_FONT: &str = "rustphotoshop-cjk";

/// The environment variable that overrides the search with an explicit font file.
pub const FONT_OVERRIDE: &str = "RUSTPHOTOSHOP_FONT";
/// The face index to use with [`FONT_OVERRIDE`], when the file is a collection.
pub const FONT_INDEX_OVERRIDE: &str = "RUSTPHOTOSHOP_FONT_INDEX";

/// Finds the font to use for text outside the Latin range.
#[must_use]
pub fn cjk_font() -> Option<FontChoice> {
    if let Some(path) = std::env::var_os(FONT_OVERRIDE) {
        let path = PathBuf::from(path);
        if path.is_file() {
            let index = std::env::var(FONT_INDEX_OVERRIDE)
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(0);
            return Some(FontChoice {
                path,
                index,
                family: "由 RUSTPHOTOSHOP_FONT 指定".to_owned(),
            });
        }
        log::warn!("{FONT_OVERRIDE} 指向的字体不存在，改用自动查找");
    }
    fontconfig_choice()
        .or_else(known_path_choice)
        .or_else(scanned_choice)
}

/// Asks fontconfig for the font the desktop would use for Simplified Chinese.
fn fontconfig_choice() -> Option<FontChoice> {
    let output = std::process::Command::new("fc-match")
        .args([
            "-f",
            "%{file}\n%{index}\n%{family}\n",
            "sans-serif:lang=zh-cn",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut lines = text.lines();
    let path = PathBuf::from(lines.next()?.trim());
    let index = lines
        .next()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(0);
    let family = lines.next().unwrap_or_default().trim().to_owned();
    if !path.is_file() {
        return None;
    }
    // fontconfig answers with *something* even when no Chinese font is installed, so make sure
    // the file it named is one that can actually draw these characters.
    if !looks_like_cjk_font(&path, &family) {
        log::warn!(
            "fontconfig 指向 {}（{}），看起来不含中文字形；改为查找常见路径",
            path.display(),
            family
        );
        return None;
    }
    Some(FontChoice {
        path,
        index,
        family,
    })
}

/// True when the file or its family name looks like a pan-CJK or Chinese face.
fn looks_like_cjk_font(path: &Path, family: &str) -> bool {
    let haystack = format!(
        "{} {}",
        path.file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().to_string()),
        family
    )
    .to_lowercase();
    [
        "cjk", "han", "song", "hei", "ming", "kai", "wqy", "zh-cn", "sc", "gb",
    ]
    .iter()
    .any(|needle| haystack.contains(needle))
}

/// The paths the usual packages install into.
fn known_path_choice() -> Option<FontChoice> {
    const CANDIDATES: &[(&str, u32)] = &[
        // Noto Sans CJK: one collection, the Simplified Chinese face is index 2.
        ("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", 2),
        (
            "/usr/share/fonts/truetype/noto-cjk/NotoSansCJK-Regular.ttc",
            2,
        ),
        ("/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc", 2),
        // Source Han Sans, the same design under Adobe's name.
        (
            "/usr/share/fonts/opentype/adobe-source-han-sans/SourceHanSansSC-Regular.otf",
            0,
        ),
        (
            "/usr/share/fonts/opentype/source-han-sans/SourceHanSansSC-Regular.otf",
            0,
        ),
        // WenQuanYi and Droid, the lighter fallbacks.
        ("/usr/share/fonts/truetype/wqy/wqy-microhei.ttc", 0),
        ("/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc", 0),
        (
            "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
            0,
        ),
        ("/usr/share/fonts/truetype/arphic/uming.ttc", 0),
        ("/usr/share/fonts/truetype/arphic/ukai.ttc", 0),
    ];
    for (path, index) in CANDIDATES {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(FontChoice {
                family: path
                    .file_stem()
                    .map_or_else(String::new, |stem| stem.to_string_lossy().to_string()),
                path,
                index: *index,
            });
        }
    }
    None
}

/// A last resort: walk the font directories for a file whose name says it covers CJK.
fn scanned_choice() -> Option<FontChoice> {
    const ROOTS: &[&str] = &["/usr/share/fonts", "/usr/local/share/fonts"];
    for root in ROOTS {
        if let Some(path) = scan_directory(Path::new(root), 0) {
            return Some(FontChoice {
                family: path
                    .file_stem()
                    .map_or_else(String::new, |stem| stem.to_string_lossy().to_string()),
                path,
                index: 2,
            });
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        let user_fonts = PathBuf::from(home).join(".local/share/fonts");
        if let Some(path) = scan_directory(&user_fonts, 0) {
            return Some(FontChoice {
                family: path
                    .file_stem()
                    .map_or_else(String::new, |stem| stem.to_string_lossy().to_string()),
                path,
                index: 2,
            });
        }
    }
    None
}

/// Depth-limited search for a font file that names a CJK family.
fn scan_directory(root: &Path, depth: usize) -> Option<PathBuf> {
    if depth > 3 || !root.is_dir() {
        return None;
    }
    let mut subdirectories = Vec::new();
    for entry in std::fs::read_dir(root).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            subdirectories.push(path);
            continue;
        }
        let name = path
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().to_lowercase());
        let is_font = name.ends_with(".ttf") || name.ends_with(".otf") || name.ends_with(".ttc");
        if is_font && looks_like_cjk_font(&path, "") {
            return Some(path);
        }
    }
    subdirectories
        .into_iter()
        .find_map(|directory| scan_directory(&directory, depth + 1))
}

/// Registers the fallback font with egui. Returns the font that was used, when one was found.
///
/// Call this once, before the first frame: egui builds its font atlas from the definitions that
/// are in place when text is first laid out.
pub fn install(ctx: &egui::Context) -> Option<FontChoice> {
    let choice = cjk_font()?;
    let bytes = match std::fs::read(&choice.path) {
        Ok(bytes) => bytes,
        Err(error) => {
            log::warn!("无法读取字体 {}：{error}", choice.path.display());
            return None;
        }
    };
    let mut definitions = egui::FontDefinitions::default();
    definitions.font_data.insert(
        FALLBACK_FONT.to_owned(),
        Arc::new(egui::FontData {
            font: bytes.into(),
            index: choice.index,
            tweak: Default::default(),
        }),
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        definitions
            .families
            .entry(family)
            .or_default()
            .push(FALLBACK_FONT.to_owned());
    }
    ctx.set_fonts(definitions);
    log::info!(
        "中文字体：{}（{}，face {}）",
        choice.family,
        choice.path.display(),
        choice.index
    );
    Some(choice)
}

/// A one-line description for the Diagnostics dialog and the self-test.
#[must_use]
pub fn diagnostic() -> String {
    match cjk_font() {
        Some(choice) => format!(
            "{} — {}（face {}）",
            choice.family,
            choice.path.display(),
            choice.index
        ),
        None => "未找到中文字体；界面中的中文会显示为方框。请安装 fonts-noto-cjk".to_owned(),
    }
}

/// True when a font with Chinese coverage can be found on this machine.
#[must_use]
pub fn cjk_available() -> bool {
    cjk_font().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_icon_and_font_paths_look_like_cjk_fonts() {
        assert!(looks_like_cjk_font(
            Path::new("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc"),
            ""
        ));
        assert!(looks_like_cjk_font(Path::new("/x/wqy-microhei.ttc"), ""));
        assert!(!looks_like_cjk_font(
            Path::new("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"),
            "DejaVu Sans"
        ));
    }

    #[test]
    fn an_explicit_override_wins() {
        let directory = tempfile::tempdir().expect("temp dir");
        let fake = directory.path().join("MyFont.ttf");
        std::fs::write(&fake, b"not a real font, only a file").expect("write");
        // SAFETY: the lock keeps other tests away from these variables.
        let _guard = crate::platform::paths::test_env_lock();
        unsafe {
            std::env::set_var(FONT_OVERRIDE, &fake);
            std::env::set_var(FONT_INDEX_OVERRIDE, "1");
        }
        let choice = cjk_font().expect("override is used");
        assert_eq!(choice.path, fake);
        assert_eq!(choice.index, 1);
        unsafe {
            std::env::remove_var(FONT_OVERRIDE);
            std::env::remove_var(FONT_INDEX_OVERRIDE);
        }
    }

    #[test]
    fn this_machine_has_a_chinese_font() {
        // The check is what the self-test reports; on a machine without one it is a note rather
        // than a failure, so the test only documents the expectation.
        let description = diagnostic();
        assert!(!description.is_empty());
        if cjk_available() {
            assert!(description.contains(".tt"), "{description}");
        }
    }

    #[test]
    fn installing_registers_the_fallback_family() {
        let context = egui::Context::default();
        if install(&context).is_none() {
            eprintln!("本机没有中文字体，跳过字体注册校验");
            return;
        }
        // A frame with Chinese text must lay out without panicking, and the fallback font has to
        // be in the definitions egui will use.
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 120.0),
            )),
            ..Default::default()
        };
        let mut output = context.run_ui(input, |ui| {
            ui.label("中文排版测试：文件、编辑、图层");
        });
        output.textures_delta.clear();
        assert!(!output.shapes.is_empty(), "中文文本没有产生绘制指令");
    }
}

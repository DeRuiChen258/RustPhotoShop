//! The application: the command line, the state, the window and the built-in checks.

pub mod cli;
pub mod self_test;
pub mod state;
pub mod window;

use std::path::{Path, PathBuf};

pub use state::{AppState, Background, Dialog, ExportFormat, Tool};

/// Runs one parsed command and returns the process exit code.
///
/// # Errors
///
/// Fails only for problems the caller should see on stderr; everything else is reported through
/// the return code and the log.
pub fn run(args: &[String]) -> Result<i32, String> {
    let command = cli::parse(args).map_err(|error| format!("{error}\n\n{}", cli::help_text()))?;
    match command {
        cli::Command::Help => {
            print!("{}", cli::help_text());
            Ok(0)
        }
        cli::Command::Version => {
            println!(
                "RustPhotoShop {VERSION}（Linux 版）",
                VERSION = crate::VERSION
            );
            Ok(0)
        }
        cli::Command::SelfTest { verbose } => {
            crate::platform::logging::init(log::LevelFilter::Info, true);
            Ok(self_test::run(verbose))
        }
        cli::Command::Export {
            input,
            output,
            quality,
        } => run_export(&input, &output, quality),
        cli::Command::Install {
            desktop_shortcut,
            bin_dir,
        } => run_install(desktop_shortcut, bin_dir.as_deref()),
        cli::Command::Uninstall => run_uninstall(),
        cli::Command::SmokeTest { frames, report } => {
            crate::platform::logging::init(log::LevelFilter::Info, true);
            prepare_environment();
            let mut state = AppState::new();
            // A document with something on it, so the pass exercises the rustphotoshop and the
            // canvas texture, not just an empty window.
            state.new_document(800, 600, Background::White);
            state.brush.size = 48.0;
            state.brush.color = [30, 120, 200, 255];
            state.foreground = [30, 120, 200, 255];
            if let Some(layer) = state.active_layer_id()
                && let Some(document) = state.document.as_mut()
            {
                crate::core::ops::stroke_segment(
                    document,
                    layer,
                    [80.0, 300.0],
                    [720.0, 320.0],
                    &state.brush,
                    None,
                );
            }
            state.touch();
            state.smoke = Some(state::SmokeState {
                frames_left: frames.max(1),
                drawn: 0,
                report,
            });
            window::run_gui(state, Vec::new())
        }
        cli::Command::Run { files, .. } => {
            crate::platform::logging::init(log::LevelFilter::Info, true);
            prepare_environment();
            let mut state = AppState::new();
            state.load_recent();
            if files.is_empty() {
                // Start with something to look at, as a fresh install has no recent files.
                state.new_document(1280, 800, Background::White);
            }
            window::run_gui(state, files)
        }
    }
}

/// Creates the directories the application writes to, without failing the launch if it cannot.
pub fn prepare_environment() {
    if let Err((path, error)) = crate::platform::paths::ensure_directories() {
        log::warn!("could not create {}: {error}", path.display());
    }
}

/// Opens a file, exports it and exits: the batch path, and what the tests use headlessly.
fn run_export(input: &Path, output: &Path, quality: u8) -> Result<i32, String> {
    crate::platform::logging::init(log::LevelFilter::Info, true);
    let mut state = AppState::new();
    state.open(input).map_err(|error| error.to_string())?;
    state
        .export_to(output, quality)
        .map_err(|error| error.to_string())?;
    println!("已导出 {}", output.display());
    Ok(0)
}

/// Installs the desktop entry, icons and (when asked) a Desktop shortcut.
fn run_install(desktop_shortcut: bool, bin_dir: Option<&Path>) -> Result<i32, String> {
    crate::platform::logging::init(log::LevelFilter::Info, true);
    let mut options = crate::platform::desktop::InstallOptions::for_current_user()
        .map_err(|error| error.to_string())?;
    if let Some(bin_dir) = bin_dir {
        options.bin_dir = bin_dir.to_path_buf();
    }
    if !desktop_shortcut {
        options.desktop_dir = None;
    }
    let report = crate::platform::desktop::install(&options).map_err(|error| error.to_string())?;
    println!("RustPhotoShop 安装完成。");
    if let Some(binary) = report.binary {
        println!("  可执行文件：{}", binary.display());
    }
    println!("  图标      ：{} 种尺寸", report.icons.len());
    if let Some(entry) = report.desktop_file {
        println!("  启动项    ：{}", entry.display());
    }
    match report.desktop_shortcut {
        Some(shortcut) => println!("  桌面图标  ：{}", shortcut.display()),
        None => println!("  桌面图标  ：已跳过（去掉 --no-desktop-shortcut 可创建）"),
    }
    println!(
        "\n可以从应用列表启动、双击桌面图标，或直接运行 {}",
        options.bin_dir.join("rustphotoshop").display()
    );
    Ok(0)
}

/// Removes everything the installer wrote.
fn run_uninstall() -> Result<i32, String> {
    crate::platform::logging::init(log::LevelFilter::Info, true);
    let options = crate::platform::desktop::InstallOptions::for_current_user()
        .map_err(|error| error.to_string())?;
    let removed =
        crate::platform::desktop::uninstall(&options).map_err(|error| error.to_string())?;
    if removed.is_empty() {
        println!("当前用户没有安装记录。");
    } else {
        println!("已删除：");
        for path in removed {
            println!("  {}", path.display());
        }
    }
    Ok(0)
}

/// The paths the launcher and the tests use to find a built binary.
#[must_use]
pub fn default_install_binary() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("rustphotoshop"))
}

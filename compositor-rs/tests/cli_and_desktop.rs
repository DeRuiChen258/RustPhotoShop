//! End-to-end checks that run the real executable, the way a person or a launcher would.
//!
//! These cover the paths that only exist outside the library: the command line parsing of the
//! shipped binary, the batch export a script would use, the install command writing a desktop
//! entry and icons into a scratch root, and uninstalling it again.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The binary cargo built for these tests.
fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_rustphotoshop"))
}

fn run(args: &[&str]) -> (bool, String, String) {
    let output = Command::new(binary())
        .args(args)
        .output()
        .expect("the binary runs");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

#[test]
fn the_binary_reports_its_version_and_help() {
    let (ok, stdout, _) = run(&["--version"]);
    assert!(ok, "--version 应当成功");
    assert!(stdout.contains("RustPhotoShop"), "{stdout}");
    assert!(stdout.contains(env!("CARGO_PKG_VERSION")), "{stdout}");

    let (ok, stdout, _) = run(&["--help"]);
    assert!(ok, "--help 应当成功");
    for expected in ["--install", "--self-test", "--export", "--smoke-test"] {
        assert!(
            stdout.contains(expected),
            "帮助里缺少 {expected}：\n{stdout}"
        );
    }
}

#[test]
fn the_built_in_checks_pass_from_the_command_line() {
    let (ok, stdout, stderr) = run(&["--self-test", "--self-test-verbose"]);
    assert!(ok, "自检失败：\n{stdout}\n{stderr}");
    assert!(stdout.contains("内置自检"), "{stdout}");
    assert!(stdout.contains("[ok  ]"), "{stdout}");
    assert!(!stdout.contains("[FAIL]"), "{stdout}");
    assert!(stdout.contains("全部"), "自检结尾缺少总结：{stdout}");
}

#[test]
fn an_unknown_option_is_refused_with_help() {
    let (ok, _, stderr) = run(&["--definitely-not-an-option"]);
    assert!(!ok, "未知选项应当失败");
    assert!(stderr.contains("未知选项"), "{stderr}");
    assert!(stderr.contains("用法"), "失败时应打印用法：{stderr}");
}

#[test]
fn a_batch_export_writes_the_requested_file() {
    let directory = tempfile::tempdir().expect("temp dir");
    // A source picture, written by the library so the test does not need a fixture file.
    let source = directory.path().join("source.png");
    let image = rustphotoshop::tests::test_image(32, 24);
    rustphotoshop::io::image_io::save_png(&image, &source).expect("save source");

    let png = directory.path().join("out.png");
    let (ok, stdout, stderr) = run(&[
        source.to_str().expect("path"),
        "--export",
        png.to_str().expect("path"),
    ]);
    assert!(ok, "导出失败：\n{stdout}\n{stderr}");
    assert!(png.is_file(), "没有写出 {}", png.display());
    let exported = rustphotoshop::io::image_io::load(&png).expect("load export");
    assert_eq!((exported.width(), exported.height()), (32, 24));

    let jpeg = directory.path().join("out.jpg");
    let (ok, _, stderr) = run(&[
        source.to_str().expect("path"),
        "--export",
        jpeg.to_str().expect("path"),
        "--quality",
        "70",
    ]);
    assert!(ok, "JPEG 导出失败：{stderr}");
    assert!(jpeg.is_file());
}

#[test]
fn exporting_an_unknown_format_says_what_is_supported() {
    let directory = tempfile::tempdir().expect("temp dir");
    let source = directory.path().join("source.png");
    rustphotoshop::io::image_io::save_png(&rustphotoshop::tests::test_image(8, 8), &source)
        .expect("save source");
    let target = directory.path().join("out.xyz");
    let (ok, _, stderr) = run(&[
        source.to_str().expect("path"),
        "--export",
        target.to_str().expect("path"),
    ]);
    assert!(!ok, "未知格式应当失败");
    assert!(stderr.contains("png"), "错误信息应当列出可用格式：{stderr}");
}

/// Installs into a scratch root, validating the same artifacts the real install writes.
#[test]
fn the_installer_writes_a_launchable_desktop_entry() {
    let directory = tempfile::tempdir().expect("temp dir");
    let root = directory.path();
    let options = install_options_for(root);
    std::fs::create_dir_all(options.binary.parent().expect("parent")).expect("build dir");
    std::fs::copy(binary(), &options.binary).expect("copy the built binary");
    std::fs::create_dir_all(options.desktop_dir.clone().expect("desktop")).expect("desktop");

    let report = rustphotoshop::platform::desktop::install(&options).expect("install");
    let entry = report.desktop_file.clone().expect("desktop file");
    let contents = std::fs::read_to_string(&entry).expect("read entry");
    assert!(contents.contains("Name=RustPhotoShop\n"), "应用名称未更新");
    assert!(contents.contains("StartupWMClass=rustphotoshop\n"));
    assert!(
        contents.contains("application/x-compositor-project;"),
        "工程 MIME 类型应保持兼容"
    );
    assert!(contents.contains("Terminal=false"), "启动项不能依赖终端");
    assert!(contents.contains("Icon=rustphotoshop"));
    assert!(contents.contains("Name[zh_CN]="), "缺少中文名称");
    assert!(contents.contains("Comment[zh_CN]="), "缺少中文描述");
    // The icons the launcher will look for, in the theme's own layout.
    assert_eq!(
        report.icons.len(),
        rustphotoshop::platform::desktop::ICON_SIZES.len()
    );
    for icon in &report.icons {
        assert!(icon.is_file(), "{} 缺失", icon.display());
    }
    let shortcut = report.desktop_shortcut.clone().expect("desktop shortcut");
    assert_eq!(shortcut.file_name().unwrap(), "RustPhotoShop.desktop");
    assert!(shortcut.is_file(), "桌面图标缺失");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&shortcut)
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111, "桌面图标必须可执行，实际 {mode:o}");
    }
    // The entry has to satisfy the desktop entry specification.
    if let Ok(output) = Command::new("desktop-file-validate").arg(&entry).output() {
        assert!(
            output.status.success(),
            "desktop-file-validate 报错：{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    // Uninstalling removes what installing wrote.
    let removed = rustphotoshop::platform::desktop::uninstall(&options).expect("uninstall");
    assert!(!removed.is_empty());
    assert!(!entry.exists());
    assert!(!shortcut.exists());
}

#[test]
fn installing_twice_is_idempotent() {
    let directory = tempfile::tempdir().expect("temp dir");
    let options = install_options_for(directory.path());
    std::fs::create_dir_all(options.binary.parent().expect("parent")).expect("build dir");
    std::fs::copy(binary(), &options.binary).expect("copy the built binary");
    std::fs::create_dir_all(options.desktop_dir.clone().expect("desktop")).expect("desktop");
    let first = rustphotoshop::platform::desktop::install(&options).expect("first");
    let second = rustphotoshop::platform::desktop::install(&options).expect("second");
    assert_eq!(first.icons.len(), second.icons.len());
    assert_eq!(first.desktop_file, second.desktop_file);
    assert_eq!(
        std::fs::read_to_string(first.desktop_file.expect("entry")).expect("read"),
        std::fs::read_to_string(second.desktop_file.expect("entry")).expect("read")
    );
}

/// The installer's paths, pointed at a scratch directory instead of the home directory.
fn install_options_for(root: &Path) -> rustphotoshop::platform::desktop::InstallOptions {
    rustphotoshop::platform::desktop::InstallOptions::with_root(root)
}

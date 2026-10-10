//! Desktop integration: the `.desktop` entry, the icon theme files and the MIME type.
//!
//! Everything the installer does is available from the binary itself (`rustphotoshop --install`),
//! so a user who downloaded a tarball gets the same result as one who ran `packaging/install.sh`.
//! Every path is a parameter, which also lets the tests install into a scratch directory and
//! check what came out.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::paths;

/// The application's id: also the window's `app_id`, the icon name and the desktop entry's name.
pub const APP_ID: &str = "rustphotoshop";

/// The icon sizes installed into the icon theme.
pub const ICON_SIZES: &[u32] = &[16, 32, 48, 64, 128, 256, 512];

/// What the launcher advertises the application can open.
pub const MIME_TYPES: &str = "image/png;image/jpeg;image/tiff;image/webp;image/bmp;image/x-psd;image/x-photoshop;application/x-compositor-project;";

/// The freedesktop categories, in the order the specification lists them.
pub const CATEGORIES: &str = "Graphics;2DGraphics;RasterGraphics;Photography;";

/// The comment the launcher shows, which doubles as the package description.
pub const COMMENT: &str = "Open-source image editor for compositing and photo work";
/// The same description in the desktop entry's Chinese keys.
pub const COMMENT_ZH: &str = "开源图像编辑器：合成与修图";

/// What went wrong installing or removing the desktop integration.
#[derive(Debug, thiserror::Error)]
pub enum DesktopError {
    #[error("无法写入 {path}：{source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("找不到程序自带的资源，无法安装图标")]
    MissingAssets,
    #[error("{0} 不存在，没有可安装的程序")]
    MissingBinary(PathBuf),
}

/// Where everything goes: built either from the user's environment or from a scratch root.
#[derive(Debug, Clone)]
pub struct InstallOptions {
    pub binary: PathBuf,
    pub bin_dir: PathBuf,
    pub icons_dir: PathBuf,
    pub applications_dir: PathBuf,
    pub desktop_dir: Option<PathBuf>,
    pub mime_dir: PathBuf,
    pub assets_dir: PathBuf,
    pub data_dir: PathBuf,
}

impl InstallOptions {
    /// The paths a normal user installation uses.
    ///
    /// # Errors
    ///
    /// Fails when the running executable's location cannot be determined, or when the assets
    /// that ship with the build are missing.
    pub fn for_current_user() -> Result<Self, DesktopError> {
        let binary = std::env::current_exe().map_err(|source| DesktopError::Write {
            path: PathBuf::from("the running executable"),
            source,
        })?;
        let shared = paths::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".local/share");
        let shared = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or(shared);
        Ok(Self {
            binary,
            bin_dir: paths::data_dir().join("bin"),
            icons_dir: paths::icons_dir(),
            applications_dir: paths::applications_dir(),
            desktop_dir: Some(paths::desktop_dir()),
            mime_dir: shared.join("mime/packages"),
            assets_dir: bundled_assets_dir().ok_or(DesktopError::MissingAssets)?,
            data_dir: paths::data_dir().join("assets"),
        })
    }

    /// The same layout, rooted in a scratch directory: what a test installation looks like.
    #[must_use]
    pub fn with_root(root: &Path) -> Self {
        Self {
            binary: root.join("build/rustphotoshop"),
            bin_dir: root.join("data/rustphotoshop/bin"),
            icons_dir: root.join("data/icons"),
            applications_dir: root.join("data/applications"),
            desktop_dir: Some(root.join("Desktop")),
            mime_dir: root.join("data/mime/packages"),
            assets_dir: bundled_assets_dir().unwrap_or_else(|| root.join("assets")),
            data_dir: root.join("data/rustphotoshop/assets"),
        }
    }
}

/// The assets that ship with this build: the icons, the MIME definition and the entry template.
#[must_use]
pub fn bundled_assets_dir() -> Option<PathBuf> {
    let icon = format!("icons/hicolor/256x256/apps/{APP_ID}.png");
    paths::asset_search_paths()
        .into_iter()
        .find(|root| root.join(&icon).is_file())
}

/// What an installation did, for the report the user sees.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct InstallReport {
    pub binary: Option<PathBuf>,
    pub icons: Vec<PathBuf>,
    pub desktop_file: Option<PathBuf>,
    pub desktop_shortcut: Option<PathBuf>,
    pub mime_file: Option<PathBuf>,
    pub assets: Vec<PathBuf>,
}

/// The `.desktop` entry's contents.
///
/// `exec` is quoted and absolute: the desktop specification only searches `PATH` for a bare
/// command name, and an application installed under the data directory is not on `PATH`.
#[must_use]
pub fn desktop_entry(exec: &Path, icon: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Version=1.0\n\
         Name={name}\n\
         Name[zh_CN]={name} 图像编辑器\n\
         GenericName=Image Editor\n\
         GenericName[zh_CN]=图像编辑器\n\
         Comment={COMMENT}\n\
         Comment[zh_CN]={COMMENT_ZH}\n\
         Exec=\"{exec}\" %F\n\
         TryExec={exec}\n\
         Icon={icon}\n\
         Terminal=false\n\
         StartupNotify=true\n\
         StartupWMClass={APP_ID}\n\
         Categories={CATEGORIES}\n\
         MimeType={MIME_TYPES}\n\
         Keywords=image;editor;photo;layers;compositing;mask;brush;psd;\n\
         Actions=NewWindow;\n\
         \n\
         [Desktop Action NewWindow]\n\
         Name=New Window\n\
         Name[zh_CN]=新窗口\n\
         Exec=\"{exec}\" --new-window\n",
        exec = exec.display(),
        name = crate::APP_NAME,
    )
}

/// Installs the executable, the icons, the desktop entry and the MIME definition.
///
/// # Errors
///
/// Fails when the executable is missing, when the bundled icons are missing, or when a file
/// cannot be written.
pub fn install(options: &InstallOptions) -> Result<InstallReport, DesktopError> {
    if !options.binary.is_file() {
        return Err(DesktopError::MissingBinary(options.binary.clone()));
    }
    let mut report = InstallReport::default();

    std::fs::create_dir_all(&options.bin_dir).map_err(|source| DesktopError::Write {
        path: options.bin_dir.clone(),
        source,
    })?;
    let installed_binary = options.bin_dir.join(APP_ID);
    std::fs::copy(&options.binary, &installed_binary).map_err(|source| DesktopError::Write {
        path: installed_binary.clone(),
        source,
    })?;
    set_executable(&installed_binary)?;
    report.binary = Some(installed_binary.clone());

    for size in ICON_SIZES {
        let relative = format!("icons/hicolor/{size}x{size}/apps/{APP_ID}.png");
        let source = options.assets_dir.join(&relative);
        if !source.is_file() {
            continue;
        }
        let target = options
            .icons_dir
            .join(format!("hicolor/{size}x{size}/apps/{APP_ID}.png"));
        copy_with_parents(&source, &target)?;
        report.icons.push(target);
    }
    if report.icons.is_empty() {
        return Err(DesktopError::MissingAssets);
    }

    for relative in ["icons", "mime", "rustphotoshop.desktop"] {
        let source = options.assets_dir.join(relative);
        if !source.exists() {
            continue;
        }
        let target = options.data_dir.join(relative);
        copy_tree(&source, &target)?;
        report.assets.push(target);
    }

    let mime_source = options
        .assets_dir
        .join("mime/application-x-compositor-project.xml");
    if mime_source.is_file() {
        let target = options
            .mime_dir
            .join("application-x-compositor-project.xml");
        copy_with_parents(&mime_source, &target)?;
        report.mime_file = Some(target);
    }

    let entry = desktop_entry(&installed_binary, APP_ID);
    let desktop_file = options.applications_dir.join(format!("{APP_ID}.desktop"));
    write_file(&desktop_file, entry.as_bytes())?;
    report.desktop_file = Some(desktop_file);

    if let Some(desktop_dir) = &options.desktop_dir
        && desktop_dir.is_dir()
    {
        let shortcut = desktop_dir.join("RustPhotoShop.desktop");
        write_file(&shortcut, entry.as_bytes())?;
        // GNOME and KDE both refuse to launch a desktop file that is not executable, even
        // with the trusted flag set, so the shortcut gets the bit the launcher entry in
        // `applications/` does not need.
        set_executable(&shortcut)?;
        // GNOME treats a shortcut as untrusted until the flag is set; without it a
        // double-click opens a text editor instead of the application.
        let _ = std::process::Command::new("gio")
            .arg("set")
            .arg(&shortcut)
            .arg("metadata::trusted")
            .arg("true")
            .status();
        report.desktop_shortcut = Some(shortcut);
    }

    refresh_caches(options);
    Ok(report)
}

/// Removes everything [`install`] wrote, reporting the paths it deleted.
///
/// # Errors
///
/// Fails when a file it owns cannot be removed.
pub fn uninstall(options: &InstallOptions) -> Result<Vec<PathBuf>, DesktopError> {
    let mut removed = Vec::new();
    let mut candidates = vec![
        options.bin_dir.join(APP_ID),
        options.applications_dir.join(format!("{APP_ID}.desktop")),
        options
            .mime_dir
            .join("application-x-compositor-project.xml"),
    ];
    for size in ICON_SIZES {
        candidates.push(
            options
                .icons_dir
                .join(format!("hicolor/{size}x{size}/apps/{APP_ID}.png")),
        );
    }
    if let Some(desktop_dir) = &options.desktop_dir {
        candidates.push(desktop_dir.join("RustPhotoShop.desktop"));
    }
    for candidate in candidates {
        if candidate.exists() {
            std::fs::remove_file(&candidate).map_err(|source| DesktopError::Write {
                path: candidate.clone(),
                source,
            })?;
            removed.push(candidate);
        }
    }
    if options.data_dir.exists() {
        std::fs::remove_dir_all(&options.data_dir).map_err(|source| DesktopError::Write {
            path: options.data_dir.clone(),
            source,
        })?;
        removed.push(options.data_dir.clone());
    }
    refresh_caches(options);
    Ok(removed)
}

/// Tells the desktop about the new files. Every step is best effort: a desktop without the tool
/// still works, it just takes a little longer to notice.
fn refresh_caches(options: &InstallOptions) {
    run_with_timeout(
        "update-desktop-database",
        &[options.applications_dir.as_os_str()],
        CACHE_TOOL_TIMEOUT,
    );
    let icon_theme = options.icons_dir.join("hicolor");
    run_with_timeout(
        "gtk-update-icon-cache",
        &[
            std::ffi::OsStr::new("-q"),
            std::ffi::OsStr::new("-t"),
            icon_theme.as_os_str(),
        ],
        CACHE_TOOL_TIMEOUT,
    );
    if let Some(mime_parent) = options.mime_dir.parent() {
        run_with_timeout(
            "update-mime-database",
            &[mime_parent.as_os_str()],
            CACHE_TOOL_TIMEOUT,
        );
    }
}

/// How long a desktop cache helper may take before it is dropped. They are conveniences: a slow
/// or broken one (some `update-desktop-database` builds spin forever) must never hold up an
/// install or an uninstall.
const CACHE_TOOL_TIMEOUT: Duration = Duration::from_secs(5);

/// Runs a helper, killing it when it overruns. Never fails the caller: the exit status is only
/// logged.
fn run_with_timeout(program: &str, arguments: &[&std::ffi::OsStr], timeout: Duration) {
    let Ok(mut child) = Command::new(program)
        .args(arguments)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return;
    };
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    log::info!("{program} exited with {status}");
                }
                return;
            }
            Ok(None) => {}
            Err(error) => {
                log::warn!("{program} could not be waited for: {error}");
                let _ = child.kill();
                return;
            }
        }
        if Instant::now() >= deadline {
            log::warn!("{program} took longer than {timeout:?}; killing it");
            let _ = child.kill();
            let _ = child.wait();
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn write_file(path: &Path, contents: &[u8]) -> Result<(), DesktopError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| DesktopError::Write {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    std::fs::write(path, contents).map_err(|source| DesktopError::Write {
        path: path.to_path_buf(),
        source,
    })
}

fn copy_with_parents(source: &Path, target: &Path) -> Result<(), DesktopError> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|source| DesktopError::Write {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    std::fs::copy(source, target).map_err(|source| DesktopError::Write {
        path: target.to_path_buf(),
        source,
    })?;
    Ok(())
}

fn copy_tree(source: &Path, target: &Path) -> Result<(), DesktopError> {
    if source.is_file() {
        return copy_with_parents(source, target);
    }
    std::fs::create_dir_all(target).map_err(|error| DesktopError::Write {
        path: target.to_path_buf(),
        source: error,
    })?;
    let entries = std::fs::read_dir(source).map_err(|error| DesktopError::Write {
        path: source.to_path_buf(),
        source: error,
    })?;
    for entry in entries.flatten() {
        copy_tree(&entry.path(), &target.join(entry.file_name()))?;
    }
    Ok(())
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<(), DesktopError> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = std::fs::metadata(path).map_err(|source| DesktopError::Write {
        path: path.to_path_buf(),
        source,
    })?;
    let mut permissions = metadata.permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).map_err(|source| DesktopError::Write {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<(), DesktopError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_desktop_entry_has_every_field_a_launcher_needs() {
        let entry = desktop_entry(
            Path::new("/home/someone/.local/share/rustphotoshop/bin/rustphotoshop"),
            APP_ID,
        );
        for expected in [
            "[Desktop Entry]",
            "Type=Application",
            "Name=RustPhotoShop",
            "Comment=",
            "Icon=rustphotoshop",
            "Terminal=false",
            "Categories=Graphics;",
            "MimeType=image/png;",
            "StartupWMClass=rustphotoshop",
            "Exec=\"/home/someone/.local/share/rustphotoshop/bin/rustphotoshop\" %F",
        ] {
            assert!(entry.contains(expected), "missing {expected} in:\n{entry}");
        }
        assert!(entry.ends_with('\n'));
    }

    /// Installs into a scratch root and returns the report and the root.
    fn install_into_scratch() -> (tempfile::TempDir, InstallOptions, InstallReport) {
        let directory = tempfile::tempdir().expect("temp dir");
        let options = InstallOptions::with_root(directory.path());
        let build_dir = options.binary.parent().expect("parent").to_path_buf();
        std::fs::create_dir_all(&build_dir).expect("build dir");
        std::fs::write(&options.binary, b"#!/bin/sh\necho rustphotoshop\n").expect("fake binary");
        let desktop = options.desktop_dir.clone().expect("desktop dir");
        std::fs::create_dir_all(&desktop).expect("desktop");
        let report = install(&options).expect("install");
        (directory, options, report)
    }

    #[test]
    fn installing_writes_icons_entries_and_a_desktop_shortcut() {
        let (directory, _options, report) = install_into_scratch();
        assert!(report.binary.expect("binary").is_file());
        assert_eq!(report.icons.len(), ICON_SIZES.len());
        for icon in &report.icons {
            assert!(icon.is_file(), "{} is missing", icon.display());
        }
        let desktop_file = report.desktop_file.expect("desktop file");
        let contents = std::fs::read_to_string(&desktop_file).expect("read");
        assert!(contents.contains("Exec=\""));
        assert!(contents.contains("Icon=rustphotoshop"));
        let shortcut = report.desktop_shortcut.expect("shortcut");
        assert!(shortcut.is_file());
        assert_eq!(
            std::fs::read_to_string(&shortcut).expect("read"),
            contents,
            "the shortcut and the launcher entry must be identical"
        );
        assert!(report.mime_file.expect("mime file").is_file());
        assert!(
            directory
                .path()
                .join("data/rustphotoshop/assets/icons")
                .is_dir()
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_installed_binary_is_marked_executable() {
        use std::os::unix::fs::PermissionsExt;
        let (_directory, _options, report) = install_into_scratch();
        let binary = report.binary.expect("binary");
        let metadata = std::fs::metadata(&binary).expect("metadata");
        let mode = metadata.permissions().mode();
        assert_eq!(mode & 0o111, 0o111, "mode was {mode:o}");
    }

    #[test]
    fn installing_twice_is_harmless() {
        let (directory, options, first) = install_into_scratch();
        let second = install(&options).expect("second install");
        assert_eq!(first.desktop_file, second.desktop_file);
        assert_eq!(first.icons.len(), second.icons.len());
        drop(directory);
    }

    #[test]
    fn uninstalling_removes_what_install_wrote() {
        let (directory, options, report) = install_into_scratch();
        let removed = uninstall(&options).expect("uninstall");
        assert!(!removed.is_empty());
        for icon in &report.icons {
            assert!(!icon.exists(), "{} is still there", icon.display());
        }
        assert!(!report.desktop_file.expect("entry").exists());
        assert!(!report.binary.expect("binary").exists());
        assert!(!directory.path().join("data/rustphotoshop/assets").exists());
    }

    #[test]
    fn installing_without_a_binary_says_so() {
        let directory = tempfile::tempdir().expect("temp dir");
        let options = InstallOptions::with_root(directory.path());
        let error = install(&options).unwrap_err();
        assert!(matches!(error, DesktopError::MissingBinary(_)), "{error}");
    }
}

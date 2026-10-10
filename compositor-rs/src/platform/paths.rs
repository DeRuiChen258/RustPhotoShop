//! Where the application keeps its files on Linux.
//!
//! Everything follows the XDG base directory specification, so nothing lands in the user's home
//! directory unasked and `XDG_*` overrides work as they should. A launched-from-a-desktop-file
//! process usually has no useful working directory, so no path here is relative to it.

use std::path::{Path, PathBuf};

/// Serializes the tests that move the XDG variables around.
///
/// Environment variables belong to the whole process and `cargo test` runs a file's tests on
/// several threads, so any test that sets one holds this while it does.
#[cfg(test)]
pub(crate) fn test_env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The directory name everything hangs off: `~/.config/rustphotoshop`, `~/.cache/rustphotoshop`, …
pub const DIRECTORY_NAME: &str = super::desktop::APP_ID;

/// Config: preferences and the recent-files list.
#[must_use]
pub fn config_dir() -> PathBuf {
    xdg_dir("XDG_CONFIG_HOME", ".config").join(DIRECTORY_NAME)
}

/// Data: installed assets, and the recent files that belong to the installation.
#[must_use]
pub fn data_dir() -> PathBuf {
    xdg_dir("XDG_DATA_HOME", ".local/share").join(DIRECTORY_NAME)
}

/// Cache: scratch space the app can lose without harm.
#[must_use]
pub fn cache_dir() -> PathBuf {
    xdg_dir("XDG_CACHE_HOME", ".cache").join(DIRECTORY_NAME)
}

/// State: logs and other files worth keeping between runs.
#[must_use]
pub fn state_dir() -> PathBuf {
    xdg_dir("XDG_STATE_HOME", ".local/state").join(DIRECTORY_NAME)
}

/// Where `.desktop` entries are installed for the current user.
#[must_use]
pub fn applications_dir() -> PathBuf {
    xdg_dir("XDG_DATA_HOME", ".local/share").join("applications")
}

/// Where icons are installed for the current user.
#[must_use]
pub fn icons_dir() -> PathBuf {
    xdg_dir("XDG_DATA_HOME", ".local/share").join("icons")
}

/// The signed-in user's Desktop folder. `XDG_DESKTOP_DIR` wins when it is set, which is how
/// desktops with a localized folder name stay correct.
#[must_use]
pub fn desktop_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("XDG_DESKTOP_DIR") {
        let path = PathBuf::from(path);
        if !path.as_os_str().is_empty() {
            return path;
        }
    }
    if let Some(home) = home_dir() {
        let candidate = home.join("Desktop");
        if candidate.is_dir() {
            return candidate;
        }
    }
    home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// The user's home directory.
#[must_use]
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// The log file, under the state directory so `journalctl`/`tail -f` both work.
#[must_use]
pub fn log_file() -> PathBuf {
    state_dir().join("rustphotoshop.log")
}

/// A fresh directory under the cache, for work that should not outlive the session: the
/// self-test's scratch files and the tests' fixtures.
///
/// The name carries the process id, so two runs never share one.
#[must_use]
pub fn scratch_dir(purpose: &str) -> PathBuf {
    let path = cache_dir()
        .join("scratch")
        .join(format!("{purpose}-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&path);
    path
}

/// The recent-files list.
#[must_use]
pub fn recent_files_file() -> PathBuf {
    data_dir().join("recent-files.txt")
}

/// Creates the directories the application writes to, and reports which one failed.
///
/// # Errors
///
/// Fails when a directory cannot be created, for example because of permissions.
pub fn ensure_directories() -> Result<(), (PathBuf, std::io::Error)> {
    for directory in [config_dir(), data_dir(), cache_dir(), state_dir()] {
        if let Err(error) = std::fs::create_dir_all(&directory) {
            return Err((directory, error));
        }
    }
    Ok(())
}

/// The directories searched for the bundled assets, in order: an installation, then a
/// development checkout.
#[must_use]
pub fn asset_search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(explicit) = std::env::var_os("RUSTPHOTOSHOP_ASSETS") {
        paths.push(PathBuf::from(explicit));
    }
    // 1. Where install.sh puts them for the current user.
    paths.push(data_dir().join("assets"));
    // 2. A system-wide installation.
    paths.push(PathBuf::from("/usr/share/rustphotoshop/assets"));
    paths.push(PathBuf::from("/usr/local/share/rustphotoshop/assets"));
    // 3. The source tree, so `cargo run` finds the same files.
    paths.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets"));
    paths
}

/// Finds a file inside the asset directories.
#[must_use]
pub fn find_asset(relative: &str) -> Option<PathBuf> {
    asset_search_paths()
        .into_iter()
        .map(|root| root.join(relative))
        .find(|candidate| candidate.is_file())
}

fn xdg_dir(variable: &str, fallback: &str) -> PathBuf {
    if let Some(value) = std::env::var_os(variable) {
        let path = PathBuf::from(value);
        if !path.as_os_str().is_empty() {
            return path;
        }
    }
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(fallback)
}

/// True when a path can be written to, used to report installation problems early.
#[must_use]
pub fn is_writable(directory: &Path) -> bool {
    let probe = directory.join(".rustphotoshop-write-test");
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs `body` with the XDG variables pointed at a scratch directory.
    fn with_xdg<T>(body: impl FnOnce(&Path) -> T) -> T {
        let directory = tempfile::tempdir().expect("temp dir");
        let _guard = test_env_lock();
        // SAFETY: the lock keeps every other test out of these variables while they are set.
        unsafe {
            std::env::set_var("XDG_DATA_HOME", directory.path().join("data"));
            std::env::set_var("XDG_CONFIG_HOME", directory.path().join("config"));
            std::env::set_var("XDG_CACHE_HOME", directory.path().join("cache"));
            std::env::set_var("XDG_STATE_HOME", directory.path().join("state"));
            std::env::set_var("XDG_DESKTOP_DIR", directory.path().join("桌面"));
        }
        let result = body(directory.path());
        unsafe {
            for variable in [
                "XDG_DATA_HOME",
                "XDG_CONFIG_HOME",
                "XDG_CACHE_HOME",
                "XDG_STATE_HOME",
                "XDG_DESKTOP_DIR",
            ] {
                std::env::remove_var(variable);
            }
        }
        result
    }

    #[test]
    fn directories_honour_the_xdg_variables() {
        with_xdg(|root| {
            assert!(config_dir().starts_with(root.join("config")));
            assert!(data_dir().starts_with(root.join("data")));
            assert!(cache_dir().starts_with(root.join("cache")));
            assert!(state_dir().starts_with(root.join("state")));
            assert!(log_file().starts_with(root.join("state")));
            assert_eq!(desktop_dir(), root.join("桌面"));
        });
    }

    #[test]
    fn installation_directories_are_created_on_demand() {
        with_xdg(|_| {
            ensure_directories().expect("create");
            assert!(config_dir().is_dir());
            assert!(data_dir().is_dir());
            assert!(is_writable(&data_dir()));
        });
    }

    #[test]
    fn asset_search_prefers_an_explicit_override() {
        let directory = tempfile::tempdir().expect("temp dir");
        let custom = directory.path().join("assets");
        std::fs::create_dir_all(custom.join("icons")).expect("create");
        std::fs::write(custom.join("icons/probe.txt"), b"here").expect("write");
        let _guard = test_env_lock();
        // SAFETY: the lock keeps other tests out of this variable while it is set.
        unsafe {
            std::env::set_var("RUSTPHOTOSHOP_ASSETS", &custom);
        }
        assert_eq!(
            find_asset("icons/probe.txt"),
            Some(custom.join("icons/probe.txt"))
        );
        unsafe {
            std::env::remove_var("RUSTPHOTOSHOP_ASSETS");
        }
        assert!(find_asset("icons/probe.txt").is_none());
    }

    #[test]
    fn the_source_tree_assets_are_found_when_running_from_cargo() {
        // The crate ships its icons, so a `cargo run` finds the same asset an install would.
        assert!(find_asset("icons/hicolor/256x256/apps/rustphotoshop.png").is_some());
    }
}

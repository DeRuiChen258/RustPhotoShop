//! Logging that survives a desktop launch.
//!
//! A double-clicked application has no terminal, so its stderr goes nowhere. Everything is
//! therefore written to `~/.local/state/rustphotoshop/rustphotoshop.log` as well, which is the first
//! place to look when the window does not appear.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use log::LevelFilter;

/// Keeps the log file open for the life of the process.
static LOG_FILE: Mutex<Option<std::fs::File>> = Mutex::new(None);

/// Starts logging to stderr and to the log file.
///
/// The level comes from `RUSTPHOTOSHOP_LOG`, then `RUST_LOG`, then the caller's default. The file
/// always gets `debug` lines, because it exists to explain failures after the fact.
pub fn init(default_level: LevelFilter, also_to_stderr: bool) -> Option<PathBuf> {
    let level = std::env::var("RUSTPHOTOSHOP_LOG")
        .or_else(|_| std::env::var("RUST_LOG"))
        .ok()
        .and_then(|value| value.parse::<LevelFilter>().ok())
        .unwrap_or(default_level);

    let path = crate::platform::paths::log_file();
    let file = if let Some(parent) = path.parent() {
        if std::fs::create_dir_all(parent).is_ok() {
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .ok()
        } else {
            None
        }
    } else {
        None
    };
    let have_file = file.is_some();
    if let Ok(mut slot) = LOG_FILE.lock() {
        *slot = file;
    }

    let mut builder = env_logger::Builder::new();
    builder.filter_level(level);
    if !also_to_stderr {
        builder.target(env_logger::Target::Pipe(Box::new(std::io::sink())));
    }
    builder.format(move |buffer, record| {
        let line = format!(
            "{} {:<5} {}: {}\n",
            timestamp(),
            record.level(),
            record.target(),
            record.args()
        );
        if let Ok(mut slot) = LOG_FILE.lock()
            && let Some(file) = slot.as_mut()
        {
            let _ = file.write_all(line.as_bytes());
            let _ = file.flush();
        }
        write!(buffer, "{line}")
    });
    let _ = builder.try_init();

    log::info!(
        "RustPhotoShop {} starting; log level {}, log file {}",
        crate::VERSION,
        level,
        if have_file {
            path.display().to_string()
        } else {
            "unavailable".to_owned()
        }
    );
    have_file.then_some(path)
}

/// Seconds since the epoch with milliseconds, which is enough to line up a crash with a click.
fn timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}.{:03}", now.as_secs(), now.subsec_millis())
}

/// The tail of the log file, for the Help ▸ Diagnostics dialog.
#[must_use]
pub fn tail(lines: usize) -> Vec<String> {
    let path = crate::platform::paths::log_file();
    let Ok(contents) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let all: Vec<&str> = contents.lines().collect();
    let start = all.len().saturating_sub(lines);
    all[start..].iter().map(|line| (*line).to_owned()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tail_of_a_missing_log_is_empty_rather_than_an_error() {
        let _guard = crate::platform::paths::test_env_lock();
        // SAFETY: the lock keeps other tests out of this variable while it is set.
        unsafe {
            std::env::set_var("XDG_STATE_HOME", "/nonexistent-for-this-test");
        }
        assert!(tail(10).is_empty());
        unsafe {
            std::env::remove_var("XDG_STATE_HOME");
        }
    }
}

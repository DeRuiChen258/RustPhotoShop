//! RustPhotoShop — a free, open-source image editor.
//!
//! This crate is the Linux (Ubuntu) desktop port of the macOS application in
//! `../Compositor`. It keeps the original's document model and file formats and reuses the
//! original C pixel kernels unchanged, so projects round-trip between the two builds.
//!
//! Layout:
//!
//! - [`core`] — document model, compositing, editing operations (no UI, no filesystem)
//! - [`io`] — file reading and writing: `.comp` projects, PNG/JPEG and other images, PSD import
//! - [`ffi`] — safe wrappers around the reused upstream C kernels
//! - [`platform`] — Linux integration: XDG paths, logging, desktop entry metadata
//! - [`app`] — application state, command line, event loop
//! - [`ui`] — egui panels, canvas, dialogs

#![forbid(unsafe_op_in_unsafe_fn)]

pub mod app;
pub mod core;
pub mod ffi;
pub mod io;
pub mod platform;
pub mod tests;
pub mod ui;

/// Product name, used for window titles, desktop entries and log lines.
pub const APP_NAME: &str = "RustPhotoShop";

/// Version shown in the About dialog and `--version`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

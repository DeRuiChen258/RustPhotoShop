//! File formats: `.comp` projects, plain images, and Photoshop files.
//!
//! Nothing here draws anything: this is the layer between the document model in
//! [`crate::core`] and the disk.

pub mod image_io;
pub mod project;
pub mod psd;

pub use image_io::{ImageError, is_supported_image};
pub use project::{ProjectError, is_project};

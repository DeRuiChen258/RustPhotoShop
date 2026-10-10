//! The editing engine: pixels, documents, compositing and operations.
//!
//! Nothing in here touches the window system or the filesystem, which keeps the whole editing
//! path unit-testable without a display.

pub mod adjustment;
pub mod blend;
pub mod composite;
pub mod document;
pub mod geometry;
pub mod history;
pub mod ops;
pub mod pixel;
pub mod selection;

pub use adjustment::Adjustment;
pub use blend::BlendMode;
pub use document::{Document, Layer, LayerKind, Sampling, Transform};
pub use geometry::RectI;
pub use history::History;
pub use pixel::{GrayImage, PremulImage, Rgba};
pub use selection::Selection;

//! Linux integration: where files live, what the log says, and how the desktop finds the app.

pub mod desktop;
pub mod logging;
pub mod paths;

pub use desktop::{APP_ID, DesktopError, InstallOptions, InstallReport};

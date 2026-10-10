//! The RustPhotoShop executable.
//!
//! All the work lives in the library so the tests can drive the same code; this is only the
//! process boundary: arguments in, exit code out.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match rustphotoshop::app::run(&args) {
        Ok(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
        Err(message) => {
            eprintln!("rustphotoshop: {message}");
            ExitCode::from(2)
        }
    }
}

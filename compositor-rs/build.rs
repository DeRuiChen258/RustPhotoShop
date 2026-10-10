//! Build script.
//!
//! Two jobs:
//!
//! 1. Compile the upstream macOS pixel kernels (`Compositor/Rendering/*.c`) unchanged, so the
//!    Rust port keeps the exact pixel behavior of the original application instead of
//!    reimplementing it. The copies under `csrc/reused/` are byte-identical to the upstream
//!    files; `scripts/sync-kernels.sh` refreshes them and `tests/kernel_parity.rs` proves they
//!    still match.
//! 2. Report, at build time, which kernels are available. Kernels that need a toolchain this
//!    machine lacks are skipped with a `cargo:warning`, and the matching Rust feature is
//!    disabled through `cfg` so no code references a missing symbol.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Kernels written in portable C: plain `gcc`/`cc` compiles them.
const PORTABLE_KERNELS: &[&str] = &[
    "AdjustPixels.c",
    "BrushPixels.c",
    "ContentFill.c",
    "HealPixels.c",
    "LensPixels.c",
    "LevelsPixels.c",
    "NoisePixels.c",
    "WandPixels.c",
];

/// Kernels that use Blocks (`^...`) and therefore need clang plus a Blocks runtime.
const BLOCKS_KERNELS: &[&str] = &["DitherPixels.c"];

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let reused = manifest_dir.join("csrc/reused");
    let compat = manifest_dir.join("csrc/compat");

    // The dither kernel sets this from build.rs; declare it so `cargo check` stays quiet.
    println!("cargo::rustc-check-cfg=cfg(have_dither_kernel)");

    for kernel in PORTABLE_KERNELS.iter().chain(BLOCKS_KERNELS) {
        println!("cargo:rerun-if-changed=csrc/reused/{kernel}");
    }
    println!("cargo:rerun-if-changed=csrc/compat/dispatch/dispatch.h");
    println!("cargo:rerun-if-changed=build.rs");

    cc::Build::new()
        .files(PORTABLE_KERNELS.iter().map(|name| reused.join(name)))
        .include(&reused)
        .include(&compat)
        .warnings(false)
        // `gnu11` (not strict `c11`) because the kernels use `M_PI`, which Apple's headers
        // always define and glibc only defines for GNU/POSIX source. The C stays untouched.
        .flag_if_supported("-std=gnu11")
        .define("_GNU_SOURCE", None)
        .compile("rustphotoshop_kernels_c");

    let dither_enabled = std::env::var_os("CARGO_FEATURE_DITHER").is_some();
    let blocks_ready = dither_enabled
        && clang_with_blocks().unwrap_or_else(|| {
            println!(
                "cargo:warning=clang with Blocks support was not found; the Dither filter is \
                 unavailable. Install clang and libblocksruntime-dev (see BUILD.md) and rebuild."
            );
            false
        });

    if blocks_ready {
        let mut build = cc::Build::new();
        build
            .compiler(blocks_compiler())
            .files(BLOCKS_KERNELS.iter().map(|name| reused.join(name)))
            .include(&reused)
            .include(&compat)
            .warnings(false)
            .flag("-fblocks")
            .flag_if_supported("-std=gnu11")
            .define("_GNU_SOURCE", None);
        if let Some(runtime) = blocks_runtime() {
            build.include(&runtime.include_dir);
            if let Some(directory) = runtime.library_dir.as_ref() {
                build.flag(format!("-L{}", directory.display()));
            }
        }
        if build.try_compile("rustphotoshop_kernels_blocks").is_ok() {
            match blocks_runtime() {
                Some(runtime) => {
                    if let Some(directory) = runtime.library_dir.as_ref() {
                        println!("cargo:rustc-link-search=native={}", directory.display());
                    }
                    // A static runtime keeps the finished binary self-contained: a shared one
                    // from a toolchain outside the loader's path would break a desktop launch.
                    match runtime.library_kind {
                        LibraryKind::Static => {
                            println!("cargo:rustc-link-lib=static=BlocksRuntime")
                        }
                        LibraryKind::Shared => println!("cargo:rustc-link-lib=BlocksRuntime"),
                    }
                    println!("cargo:rustc-cfg=have_dither_kernel");
                    println!(
                        "cargo:warning=RustPhotoShop: reused DitherPixels.c kernel enabled ({} Blocks runtime at {}).",
                        match runtime.library_kind {
                            LibraryKind::Static => "static",
                            LibraryKind::Shared => "shared",
                        },
                        runtime.library_dir.as_ref().map_or_else(
                            || "the compiler's own path".to_owned(),
                            |dir| dir.display().to_string()
                        )
                    );
                }
                None => {
                    println!("cargo:rustc-cfg=have_dither_kernel");
                    println!(
                        "cargo:warning=RustPhotoShop: reused DitherPixels.c kernel enabled (Blocks runtime from the linker's default path)."
                    );
                }
            }
        } else {
            println!(
                "cargo:warning=the Dither kernel could not be compiled with clang; the Dither \
                 filter is unavailable. Install libblocksruntime-dev and rebuild."
            );
        }
    }
}

/// How the Blocks runtime is linked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LibraryKind {
    Static,
    Shared,
}

/// Where the Blocks runtime was found.
#[derive(Clone, Debug)]
struct BlocksRuntime {
    library_dir: Option<PathBuf>,
    include_dir: PathBuf,
    library_kind: LibraryKind,
}

/// Finds a Blocks runtime.
///
/// A **static** library is preferred: the resulting binary then has no runtime dependency a
/// desktop launch could trip over. `libblocksruntime-dev` (the Ubuntu package) installs a shared
/// one into the standard library path, which works just as well; a Swift toolchain keeps a
/// static copy next to its compiler. Set `RUSTPHOTOSHOP_BLOCKS_RUNTIME_DIR` to point at either.
fn blocks_runtime() -> Option<BlocksRuntime> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(explicit) = std::env::var_os("RUSTPHOTOSHOP_BLOCKS_RUNTIME_DIR") {
        roots.push(PathBuf::from(explicit));
    }
    if let Some(paths) = std::env::var_os("LIBRARY_PATH") {
        roots.extend(std::env::split_paths(&paths));
    }
    roots.extend(
        [
            "/usr/lib/x86_64-linux-gnu",
            "/usr/lib64",
            "/usr/lib",
            "/usr/local/lib",
        ]
        .iter()
        .map(PathBuf::from),
    );
    // Swift toolchains ship the runtime; look in the usual places without hard-coding one user.
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        for base in [home.join(".swiftly/toolchains")] {
            if let Ok(entries) = std::fs::read_dir(&base) {
                for entry in entries.flatten() {
                    roots.push(entry.path().join("usr/lib/swift_static/linux"));
                    roots.push(entry.path().join("usr/lib/swift/linux"));
                }
            }
        }
    }
    roots.push(PathBuf::from("/usr/lib/swift_static/linux"));
    roots.push(PathBuf::from("/usr/lib/swift/linux"));
    roots.extend(compiler_search_dirs());

    // Static first, so the binary needs nothing at run time.
    for kind in [LibraryKind::Static, LibraryKind::Shared] {
        let (prefix, suffix) = match kind {
            LibraryKind::Static => ("lib", ".a"),
            LibraryKind::Shared => ("lib", ".so"),
        };
        for root in &roots {
            if !root
                .join(format!("{prefix}BlocksRuntime{suffix}"))
                .is_file()
            {
                continue;
            }
            // The headers live next to the library in the Swift layout, or in /usr/include.
            let include_dir =
                if root.ends_with("swift/linux") || root.ends_with("swift_static/linux") {
                    root.parent()
                        .and_then(Path::parent)
                        .map_or_else(|| root.clone(), Path::to_path_buf)
                        .join("include")
                } else {
                    PathBuf::from("/usr/include")
                };
            return Some(BlocksRuntime {
                library_dir: Some(root.clone()),
                include_dir,
                library_kind: kind,
            });
        }
    }
    None
}

/// The library directories the Blocks-capable compiler itself looks in, plus the Swift runtime
/// directories beside them. This finds a toolchain wherever it was installed, without guessing
/// at a user's layout.
fn compiler_search_dirs() -> Vec<PathBuf> {
    let Ok(output) = Command::new(blocks_compiler())
        .arg("-print-search-dirs")
        .output()
    else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let Some(libraries) = text
        .lines()
        .find_map(|line| line.strip_prefix("libraries: ="))
    else {
        return Vec::new();
    };
    let mut dirs = Vec::new();
    for entry in libraries.split(':').filter(|entry| !entry.is_empty()) {
        let path = PathBuf::from(entry);
        dirs.push(path.clone());
        // .../usr/lib/clang/21 → the toolchain's own lib directories.
        if let Some(index) = path
            .components()
            .position(|component| component.as_os_str() == "usr")
        {
            // Up to and including `usr`, so the joins below read like the toolchain's own tree.
            let root: PathBuf = path.components().take(index + 1).collect();
            dirs.push(root.join("lib/swift_static/linux"));
            dirs.push(root.join("lib/swift/linux"));
            dirs.push(root.join("lib"));
        }
    }
    dirs
}

/// The compiler that understands `-fblocks`: clang, or whatever `CC` points at.
fn blocks_compiler() -> String {
    std::env::var("CC").unwrap_or_else(|_| "clang".to_owned())
}

/// Probes for a clang that can compile and link a block, so the dither kernel is only built when
/// the whole toolchain is really present.
fn clang_with_blocks() -> Option<bool> {
    let compiler = blocks_compiler();
    let probe = std::env::var("OUT_DIR")
        .ok()
        .map(PathBuf::from)?
        .join("blocks_probe.c");
    std::fs::write(
        &probe,
        "int run(void) { __block int seen = 0; void (^body)(int) = ^(int value) { seen += value; }; \
         body(1); return seen; }\nint main(void) { return run() - 1; }\n",
    )
    .ok()?;

    let mut command = Command::new(&compiler);
    command
        .arg("-fblocks")
        .arg(&probe)
        .arg("-o")
        .arg(probe.with_extension("bin"));
    if let Some(runtime) = blocks_runtime()
        && let Some(directory) = runtime.library_dir.as_ref()
    {
        command.arg(format!("-L{}", directory.display()));
    }
    command.arg("-lBlocksRuntime");
    let output = command.output().ok()?;
    Some(output.status.success())
}

//! Proves that the C kernels this crate compiles are the upstream project's own files.
//!
//! The point of reusing them is that the Linux build paints, wand-selects and heals exactly the
//! way the macOS build does. That promise is only worth anything if the copies have not drifted,
//! so this test compares them byte for byte against `Compositor/Rendering/*.c` in the repository
//! they came from.
//!
//! When the crate is built outside that repository (a source tarball, say) the check reports that
//! it could not run rather than failing: `scripts/sync-kernels.sh` is what refreshes them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The kernels, as `csrc/reused` names them.
const KERNELS: &[&str] = &[
    "AdjustPixels.c",
    "AdjustPixels.h",
    "BrushPixels.c",
    "BrushPixels.h",
    "ContentFill.c",
    "ContentFill.h",
    "DitherPixels.c",
    "DitherPixels.h",
    "HealPixels.c",
    "HealPixels.h",
    "LensPixels.c",
    "LensPixels.h",
    "LevelsPixels.c",
    "LevelsPixels.h",
    "NoisePixels.c",
    "NoisePixels.h",
    "WandPixels.c",
    "WandPixels.h",
];

fn reused_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("csrc/reused")
}

/// The macOS project's own copy of the kernels, when this checkout has one.
fn upstream_dir() -> Option<PathBuf> {
    if let Ok(explicit) = std::env::var("RUSTPHOTOSHOP_UPSTREAM_DIR") {
        let path = PathBuf::from(explicit);
        return path.is_dir().then_some(path);
    }
    let candidate = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .join("Compositor/Rendering");
    candidate.is_dir().then_some(candidate)
}

fn digest(path: &Path) -> String {
    // `sha256sum` is what the sync script uses, so both agree on the answer.
    let output = std::process::Command::new("sha256sum")
        .arg(path)
        .output()
        .unwrap_or_else(|error| panic!("this check needs sha256sum: {error}"));
    assert!(
        output.status.success(),
        "sha256sum failed on {}",
        path.display()
    );
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_owned()
}

#[test]
fn the_reused_kernels_are_the_upstream_files() {
    let Some(upstream) = upstream_dir() else {
        eprintln!("上游 C 源码目录不存在，跳过零改动校验（可用 RUSTPHOTOSHOP_UPSTREAM_DIR 指定）");
        return;
    };
    let mut differences = Vec::new();
    for kernel in KERNELS {
        let copy = reused_dir().join(kernel);
        let original = upstream.join(kernel);
        assert!(copy.is_file(), "{} 缺失", copy.display());
        if !original.is_file() {
            differences.push(format!("上游缺少 {kernel}"));
            continue;
        }
        if digest(&copy) != digest(&original) {
            differences.push(format!("{kernel} 与上游不一致"));
        }
    }
    assert!(
        differences.is_empty(),
        "复用的内核与上游不一致：{differences:#?}\n请运行 scripts/sync-kernels.sh 重新同步"
    );
}

#[test]
fn every_upstream_kernel_is_reused() {
    let Some(upstream) = upstream_dir() else {
        return;
    };
    let mut upstream_files: BTreeMap<String, ()> = BTreeMap::new();
    for entry in std::fs::read_dir(&upstream)
        .expect("read upstream")
        .flatten()
    {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.ends_with(".c") || name.ends_with(".h") {
            upstream_files.insert(name, ());
        }
    }
    let missing: Vec<String> = upstream_files
        .keys()
        .filter(|name| !reused_dir().join(name).is_file())
        .cloned()
        .collect();
    assert!(
        missing.is_empty(),
        "上游内核没有全部复用：{missing:?}\n新增内核后请更新 scripts/sync-kernels.sh 与 build.rs"
    );
}

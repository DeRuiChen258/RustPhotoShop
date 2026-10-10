# RustPhotoShop · Rust / Ubuntu 工程

RustPhotoShop 的 Linux 实现位于本目录，crate 和可执行程序名称均为 `rustphotoshop`。主要介绍、功能表、文件支持、快捷键和上游来源见[项目根 README](../README.md)。

## 构建与桌面安装

以下命令从本目录执行：

```bash
bash "scripts/build.sh"
bash "scripts/install.sh"
```

需要 Rust 1.95+、C 编译器、图形开发库和中文字体。可选抖动内核需要 clang 与 Blocks 运行时。完整依赖与故障排查见 [BUILD.md](BUILD.md)。

安装后，应用列表名称为 RustPhotoShop，桌面图标为 `RustPhotoShop.desktop`；可执行文件默认位于 `~/.local/share/rustphotoshop/bin/rustphotoshop`。安装器会设置桌面文件可执行位并尝试设置 GNOME 可信标记。

```bash
"$HOME/.local/share/rustphotoshop/bin/rustphotoshop"
"$HOME/.local/share/rustphotoshop/bin/rustphotoshop" "/路径/照片.png"
```

用户级目录通常不在 `PATH` 中，请使用上述完整路径。桌面目录可通过 `XDG_DESKTOP_DIR` 指定。

## 开发与验证

```bash
cargo run --locked
cargo test --locked
bash "scripts/test.sh"
```

测试报告写入 [TEST-REPORT.md](TEST-REPORT.md)。报告区分真正通过的检查和缺少显示环境时跳过的 GUI 检查。

## CLI 与打包

```bash
./target/release/rustphotoshop --version
./target/release/rustphotoshop --help
./target/release/rustphotoshop --self-test-verbose
./target/release/rustphotoshop "/路径/图片.png" --export "/路径/导出.jpg" --quality 92
./target/release/rustphotoshop --smoke-test 30 --smoke-report "/tmp/rustphotoshop-smoke.txt"

bash "packaging/build-deb.sh"
bash "scripts/uninstall.sh"
```

deb 产物为 `target/deb/rustphotoshop_<版本>_<架构>.deb`。用户级安装无需 root，系统级安装 deb 需要管理员权限。

## 兼容性与更名

显示名称为 RustPhotoShop；CLI、桌面 ID、图标名称和 XDG 目录使用 `rustphotoshop`。源码目录 `compositor-rs/` 与上游 `../Compositor/` 保留原位置。

`.comp`、`com.compositor.project` 和 `application/x-compositor-project` 保留作为工程格式契约。Linux 当前只支持其中已实现的渲染行为；保留部分未知 JSON 字段不意味着完全支持高级工程。

## 文档

- [BUILD.md](BUILD.md)：构建、安装和资源目录。
- [TEST.md](TEST.md)：测试体系与人工清单。
- [KNOWN-LIMITATIONS.md](KNOWN-LIMITATIONS.md)：高级功能和格式限制。
- [docs/ANALYSIS.md](docs/ANALYSIS.md)：原项目分析，按迁移时的旧名称记录。
- [assets/README.md](assets/README.md)：图标和桌面资源。
- [../LICENSE](../LICENSE)：MIT 许可与上游版权声明。

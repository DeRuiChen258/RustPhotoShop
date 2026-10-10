# 构建、运行与安装

目标平台是 Ubuntu（在 Ubuntu 26.04 + GNOME/Wayland 上验证）。Rust 工具链需要 1.95 或更新。

## 1. 依赖

### 必需

| 依赖 | 用途 | 安装 |
|------|------|------|
| Rust 1.95+（含 cargo） | 构建 | `rustup` 或发行版包 |
| C 编译器（gcc/cc） | 编译复用的 C 内核 | `sudo apt install build-essential` |
| OpenGL 与 X11/Wayland 客户端库 | 窗口与渲染 | `sudo apt install libgl1 libxkbcommon0 libwayland-client0 libx11-xcb1` |
| 一个中文字体 | 界面中文显示 | `sudo apt install fonts-noto-cjk` |

`cargo build` 会自动编译依赖；egui/eframe 的构建需要 `pkg-config` 与上述系统库的开发头
文件时，请一并安装 `libxkbcommon-dev libx11-dev libwayland-dev libgl1-mesa-dev`。

### 可选：抖动（Dither）滤镜的运行时

上游的 `DitherPixels.c` 使用了 clang 的 Blocks 扩展（`^...`）与 Blocks 运行时。这一条是
**可选**的：

```bash
sudo apt install libblocksruntime-dev clang
```

`build.rs` 会自己找 Blocks 运行时：先看标准库路径（apt 包装在那里），再问编译器
（`clang -print-search-dirs`）和 `LIBRARY_PATH`。找到**静态**库时优先静态链接，产物因此
不依赖任何外部 `.so`；找不到时该滤镜不编入，界面会明确告知，其余功能完全不受影响。

也可以显式指定：`RUSTPHOTOSHOP_BLOCKS_RUNTIME_DIR=/path/to/dir`。

### 可选：打包

`.deb` 打包需要 `dpkg-deb`（Ubuntu 自带）与 `fakeroot`。图标重建需要 ImageMagick。

## 2. 构建

```bash
cd compositor-rs
./scripts/build.sh          # 环境报告 + 发布构建 + 自检
```

或者直接：

```bash
cargo build --release       # 产物：target/release/rustphotoshop
```

构建脚本会打印工具链版本、clang 是否可用、产物大小、是否动态依赖 Blocks 运行时，最后跑
一遍内置自检。开发时用 `cargo run`（调试构建的依赖是优化过的，界面不会卡）。

## 3. 运行

```bash
./scripts/run.sh                    # 打开空窗口
./scripts/run.sh 图片.png 图层.png    # 打开文档并把后续文件作为图层导入
./scripts/run.sh 工程.comp
```

日志（含中文字体选择）同时写到终端与 `~/.local/state/rustphotoshop/rustphotoshop.log`。

## 4. 安装（桌面双击启动）

```bash
./scripts/install.sh
```

等价于 `target/release/rustphotoshop --install`，它把下面这些写进**当前用户自己的目录**，
不需要 root：

| 内容 | 位置 |
|------|------|
| 可执行文件 | `~/.local/share/rustphotoshop/bin/rustphotoshop` |
| 图标（16–512 共 7 种） | `~/.local/share/icons/hicolor/<尺寸>/apps/rustphotoshop.png` |
| 应用列表启动项 | `~/.local/share/applications/rustphotoshop.desktop` |
| 桌面图标（可双击） | `~/Desktop/RustPhotoShop.desktop` |
| `.comp` 的 MIME 定义 | `~/.local/share/mime/packages/application-x-compositor-project.xml` |
| 程序自带资源副本 | `~/.local/share/rustphotoshop/assets/` |

安装器还会给桌面图标加上可执行位与 GNOME 的 `metadata::trusted` 标记 —— 少任何一样，
GNOME 双击时都会把它当成文本文件打开。最后刷新桌面数据库与图标缓存（带超时保护，
某个工具卡住不会拖住安装）。

选项：

```bash
rustphotoshop --install --no-desktop-shortcut   # 不创建桌面图标
rustphotoshop --install --bin-dir ~/bin         # 换一个可执行文件目录
rustphotoshop --uninstall                       # 卸载（删除上面列出的所有文件）
```

### 系统级安装（.deb）

```bash
./packaging/build-deb.sh
sudo dpkg -i target/deb/rustphotoshop_1.4.5_amd64.deb
```

包会把可执行文件装到 `/usr/bin/rustphotoshop`，图标装到 `/usr/share/icons/hicolor`，启动项
装到 `/usr/share/applications`，并在安装后刷新桌面数据库。依赖里把 `fonts-noto-cjk`
列为 `Recommends`。

## 5. 故障排查

| 现象 | 原因与处理 |
|------|-----------|
| 双击桌面图标没反应 | 看 `~/.local/state/rustphotoshop/rustphotoshop.log`；确认桌面文件可执行（`ls -l ~/Desktop/RustPhotoShop.desktop` 应有 `x`），必要时右键「允许启动」 |
| 界面中文显示成方框 | 没找到中文字体：`sudo apt install fonts-noto-cjk`，或用 `RUSTPHOTOSHOP_FONT=/path/font.ttf` 指定；`rustphotoshop --self-test` 会报告这个问题 |
| 启动报 `libBlocksRuntime.so` 找不到 | 该二进制是在别的机器上构建的、且动态链接了 Blocks 运行时。在一台有 `libblocksruntime-dev` 的机器上重新构建，或安装同一版本的包 |
| 抖动滤镜提示不可用 | 未安装 clang/Blocks 运行时；`sudo apt install clang libblocksruntime-dev` 后重新构建 |
| `update-desktop-database` 卡住 | 安装器已加 5 秒超时，不会阻塞；桌面数据库只是加速缓存，缺失不影响启动 |
| 窗口能开但画面卡 | 无 GPU 或软件渲染：`LIBGL_ALWAYS_SOFTWARE=1` 可强制软件渲染；合成本身是 CPU 实现，大文档会慢 |
| 找不到资源（图标/模板） | 用 `RUSTPHOTOSHOP_ASSETS=/path/to/assets` 指定资源目录；安装版会自动读 `~/.local/share/rustphotoshop/assets` |

## 6. 目录约定

全部遵循 XDG 规范，可用环境变量覆盖：

| 用途 | 默认 | 覆盖变量 |
|------|------|----------|
| 配置 | `~/.config/rustphotoshop` | `XDG_CONFIG_HOME` |
| 数据（含最近打开记录） | `~/.local/share/rustphotoshop` | `XDG_DATA_HOME` |
| 缓存 / 临时文件 | `~/.cache/rustphotoshop` | `XDG_CACHE_HOME` |
| 日志 | `~/.local/state/rustphotoshop/rustphotoshop.log` | `XDG_STATE_HOME` |

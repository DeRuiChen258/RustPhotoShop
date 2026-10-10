# RustPhotoShop

RustPhotoShop 是基于 Rust 与 egui 构建的 Ubuntu 开源图像编辑器，源自 Compositor。支持图层、蒙版、选区、画笔、修复、调色与常见图片导入导出，提供中文界面、桌面双击启动、用户级安装、deb 打包和自动化测试。沿用 .comp 工程格式；Linux 版仍在完善，PSD/PSB 当前仅导入合成图。

<img src="compositor-rs/assets/icons/hicolor/128x128/apps/rustphotoshop.png" alt="RustPhotoShop 应用图标" width="96">

## 项目定位

RustPhotoShop 面向照片修饰、图层合成和基础图像编辑，采用接近 Photoshop 的菜单和快捷键习惯。项目遵循 MIT 许可，可修改源码和二次开发。

当前 Linux 应用位于 `compositor-rs/`，界面与应用逻辑使用 Rust，复用 Compositor 的 C 像素内核。仓库同时保留上游 macOS 源码 `Compositor/` 和 Xcode 工程。应用名称、Linux 命令和打包名称已统一为 RustPhotoShop / `rustphotoshop`；源码目录名称暂沿用原有结构。

Linux 版已经具备基础编辑与桌面安装路径，高级功能仍在迁移。文字工具、图层特效、Camera Raw、PSD 分层编辑等功能的状态见[功能边界](#功能边界)。

## 快速开始：Ubuntu 桌面安装

以下命令从仓库根目录执行。

### 1. 准备环境

需要 Rust 1.95 或更新版本、C 编译器、OpenGL 图形环境以及中文字体。

```bash
rustc --version
cargo --version

sudo apt install build-essential pkg-config \
  libx11-dev libxkbcommon-dev libwayland-dev libgl1-mesa-dev \
  fonts-noto-cjk
```

Rust 版本过低时，可通过现有的 `rustup` 更新工具链：

```bash
rustup update stable
```

文件对话框使用 XDG Desktop Portal。Ubuntu/GNOME 通常已安装对应服务；如果无法弹出对话框，可安装 `xdg-desktop-portal` 与 `xdg-desktop-portal-gnome`。

抖动滤镜的构建依赖是可选的：

```bash
sudo apt install clang libblocksruntime-dev
```

没有 Blocks 运行时也可以构建应用，但该构建不会启用抖动滤镜。

### 2. 构建

```bash
cd "compositor-rs"
bash "scripts/build.sh"
```

产物为 `compositor-rs/target/release/rustphotoshop`。构建脚本会打印环境信息，执行发布构建并运行内置自检。首次构建需要下载依赖，耗时由网络和机器性能决定。

也可以直接构建或以开发模式运行：

```bash
cargo build --release --locked
cargo run --locked
```

### 3. 安装到当前用户

在 `compositor-rs/` 目录内执行：

```bash
bash "scripts/install.sh"
```

程序安装到用户目录，无需 `sudo`。安装器生成应用列表入口、桌面快捷方式、图标和 MIME 定义；桌面文件设置为可执行，并尝试设置 GNOME 的可信启动标记。

| 内容 | 默认位置 |
| --- | --- |
| 可执行程序 | `~/.local/share/rustphotoshop/bin/rustphotoshop` |
| 应用列表入口 | `~/.local/share/applications/rustphotoshop.desktop` |
| 桌面快捷方式 | `~/Desktop/RustPhotoShop.desktop` |
| 图标 | `~/.local/share/icons/hicolor/<尺寸>/apps/rustphotoshop.png` |
| 程序资源 | `~/.local/share/rustphotoshop/assets/` |
| 日志 | `~/.local/state/rustphotoshop/rustphotoshop.log` |

桌面位置会优先采用 `XDG_DESKTOP_DIR`，其次使用已存在的 `~/Desktop`。只有桌面目录存在时才创建快捷方式；可在安装前显式指定：

```bash
mkdir -p "$HOME/Desktop"
XDG_DESKTOP_DIR="$HOME/Desktop" bash "scripts/install.sh"
```

### 4. 启动

双击桌面上的 **RustPhotoShop** 图标，或在应用列表搜索 RustPhotoShop。

命令行启动会输出日志：

```bash
"$HOME/.local/share/rustphotoshop/bin/rustphotoshop"
"$HOME/.local/share/rustphotoshop/bin/rustphotoshop" "/路径/照片.png"
```

用户级安装目录通常不在 `PATH` 中；以上使用完整路径的命令可以直接执行。系统级 deb 安装后可使用 `rustphotoshop` 命令。

### 5. 卸载

在源码的 `compositor-rs/` 目录中执行：

```bash
bash "scripts/uninstall.sh"
```

该命令移除当前名称下的可执行程序、启动项、图标与安装资源。用户保存的图片和 `.comp` 工程不属于卸载目标。

## 当前 Linux 版功能

| 类别 | 已接入的能力 |
| --- | --- |
| 图层 | 新建、复制、删除、上移/下移、文件夹编组、24 种混合模式、不透明度、向下合并和拼合 |
| 蒙版 | 添加、启用/停用、反相、应用和删除图层蒙版 |
| 选区 | 矩形、椭圆、魔棒、全选、反选、取消选择、羽化、载入图层 Alpha |
| 绘画与修复 | 可调大小、硬度和不透明度的画笔，橡皮擦、污点修复、吸管和颜色选择 |
| 填充 | 前景色/背景色填充、选区内的内容感知填充 |
| 调整 | 色阶、曲线、色相/饱和度、曝光、反相、黑白、色彩平衡、渐变映射 |
| 滤镜 | 高斯模糊、动感模糊、杂色、颗粒、暗角、色调对比、镜头校正；抖动取决于构建环境 |
| 画布 | 裁剪、修整、图像大小、画布大小、移动图层、翻转、旋转 90°、缩放和平移 |
| 编辑历史 | 撤销和重做，历史保留在当前会话中 |
| 桌面交互 | 中文菜单与对话框、中文字体回退、文件打开、图片拖入、最近打开列表 |

图像合成主要在 CPU 上完成；egui 通过 OpenGL 显示界面和画布纹理。目前不包含 Metal 渲染后端的 Linux 移植。

## 文件支持

| 类型 | 打开 | 保存/导出 | 说明 |
| --- | --- | --- | --- |
| `.comp` | 支持版本 1–11 | 保存为工程包 | 保留图层像素及已支持的文档属性 |
| PNG | 支持 | 支持 | 适合保留透明通道 |
| JPEG | 支持 | 支持 | 导出提供 1–100 品质参数，透明区域合成到白底 |
| WebP、TIFF、BMP | 支持 | 支持 | 编码能力以所用 `image` crate 为准 |
| GIF | 支持读取 | 不导出 GIF | 当前流程导入静态画面，不提供动画编辑 |
| PSD、PSB | 有限支持 | 不支持写出 | 8 位 RGB/灰度、raw 或 PackBits；只读取文件中的合成图 |
| HEIC、相机 RAW、SVG | 当前未接入 | 当前未接入 | 上游 macOS 版本的支持不代表 Linux 版已支持 |

`.comp` 是一个目录包：

```text
示例.comp/
├── manifest.json
├── images/
│   ├── <图层 UUID>.png
│   └── <图层 UUID>.mask.png
└── QuickLook/
    └── Preview.jpg
```

更名保留 `.comp` 扩展名、`com.compositor.project` 格式标识和 `application/x-compositor-project` MIME 类型。格式设计继续沿用 Compositor，已验证 Rust 侧往返读写和符合格式文档的手写工程。

未实现的图层属性部分以原始 JSON 保留；这不等于完整渲染兼容。高级工程的跨 macOS 往返、特效显示和所有编辑操作后的元数据一致性仍需单独验证，建议先在副本上操作。

格式说明见 [project-format.md](docs/project-format.md)；脚本构造工程的规则见 [writing-comp-files.md](docs/writing-comp-files.md)。其中实时文件监控等行为描述来自 macOS 原版，不代表 Linux 已实现。

## 基础编辑流程

1. 从桌面启动 RustPhotoShop，通过「文件 → 打开」导入普通图片，或「文件 → 新建」创建画布。
2. 使用图层面板新建图层，调整混合模式与不透明度。
3. 用画笔或橡皮擦绘制；需要限定范围时先创建矩形、椭圆或魔棒选区。
4. 在「调整」或「滤镜」菜单中选择处理操作。
5. 使用「文件 → 另存为」保存到新的 `.comp` 路径；使用「导出」生成 PNG 或 JPEG。
6. 关闭窗口前主动保存需要保留的修改。

图片导入后请使用「另存为」并显式选择 `.comp` 路径，避免把原始图片路径当作工程保存位置。`.comp` 目录可直接作为命令行参数打开：

```bash
"$HOME/.local/share/rustphotoshop/bin/rustphotoshop" "/路径/示例.comp"
```

## 常用快捷键

| 快捷键 | 操作 |
| --- | --- |
| `Ctrl+N` / `Ctrl+O` | 新建 / 打开 |
| `Ctrl+S` / `Ctrl+Shift+S` | 保存 / 另存为 |
| `Ctrl+Shift+E` | 导出图片 |
| `Ctrl+Z` / `Ctrl+Shift+Z` | 撤销 / 重做 |
| `Ctrl+A` / `Ctrl+D` / `Ctrl+Shift+I` | 全选 / 取消选择 / 反选 |
| `Ctrl+J` / `Ctrl+E` | 复制图层 / 向下合并 |
| `B` / `E` / `J` / `I` | 画笔 / 橡皮擦 / 污点修复 / 吸管 |
| `M` / `Shift+M` / `W` | 矩形选区 / 椭圆选区 / 魔棒 |
| `V` / `C` / `H` / `Z` | 移动 / 裁剪 / 手形 / 缩放 |
| `[` / `]` | 缩小 / 放大画笔 |
| `Ctrl+0` / `Ctrl+1` | 适合屏幕 / 实际像素 |
| `Space+拖动` | 平移 |
| `Ctrl+滚轮` | 以指针位置缩放 |

## 命令行与批量导出

以下示例从 `compositor-rs/` 执行：

```bash
# 版本与帮助
./target/release/rustphotoshop --version
./target/release/rustphotoshop --help

# 第一张图片作为文档，其余图片作为新图层导入
./target/release/rustphotoshop "/路径/底图.png" "/路径/叠加.png"

# 无需窗口的批量格式转换
./target/release/rustphotoshop "/路径/原图.png" --export "/路径/结果.jpg" --quality 92

# 内置检查与真实窗口启动测试
./target/release/rustphotoshop --self-test-verbose
./target/release/rustphotoshop --smoke-test 30 --smoke-report "/tmp/rustphotoshop-smoke.txt"

# 调试日志
RUSTPHOTOSHOP_LOG=debug ./target/release/rustphotoshop
```

批量导出会写入指定目标文件；需要保留已有输出时，请使用新的目标路径。

## deb 打包

在 `compositor-rs/` 下执行：

```bash
bash "packaging/build-deb.sh"
```

产物位于 `target/deb/`，名称类似 `rustphotoshop_1.4.5_amd64.deb`，架构由构建机器确定。

```bash
sudo apt install "./target/deb/rustphotoshop_1.4.5_amd64.deb"
```

系统级安装的可执行文件为 `/usr/bin/rustphotoshop`，应用入口为 `/usr/share/applications/rustphotoshop.desktop`。deb 提供应用列表入口；需要桌面快捷方式时，在用户会话中运行：

```bash
/usr/bin/rustphotoshop --install
```

当前提供用户级安装与 deb 打包。AppImage、Flatpak 尚未提供。二进制受构建机器的 glibc 和图形库版本影响，需要支持其它 Ubuntu 版本时，应在目标版本上构建并验证。

## 测试与验证

在 `compositor-rs/` 执行完整验证：

```bash
bash "scripts/test.sh"
```

结果写入 [TEST-REPORT.md](compositor-rs/TEST-REPORT.md)。测试涵盖格式检查、静态检查、单元与集成测试、发布构建、内置自检、桌面文件校验和窗口启动。

也可以按需运行：

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
./target/release/rustphotoshop --self-test-verbose
```

| 测试文件 | 验证内容 |
| --- | --- |
| `src/core/`、`src/ffi/`、`src/io/` 等模块的单元测试 | 编辑模型、图像处理、读写与错误处理 |
| `tests/kernel_parity.rs` | 复用的 C 文件与上游逐字节一致 |
| `tests/project_format.rs` | `.comp` 格式、图层/蒙版/参考线往返及路径检查 |
| `tests/cli_and_desktop.rs` | 新名称二进制、CLI 导出、安装/卸载、桌面入口与图标 |
| `tests/ui_headless.rs` | 菜单、面板、工具和对话框的无头布局 |

本机最近验证环境为 Ubuntu 26.04、Rust 1.96.0、GNOME/Wayland；更名后运行了 157 个单元与集成测试，以及 13 项内置检查。实际结果和运行时间以报告为准。

无头布局测试不能替代真实鼠标操作和视觉验收。无显示环境时启动测试会记录为跳过；完整人工验证步骤见 [TEST.md](compositor-rs/TEST.md)。

## 字体、图标与故障排查

图标源文件为根目录 `图标/30f8cfffc3d09087aa (Edited).png`，原图为 2839×2839 的 PNG。已生成 16、32、48、64、128、256、512 七种尺寸，窗口和桌面入口共用这套图片。

界面通过 fontconfig 寻找中文字体，本机使用 Noto Sans CJK SC。若显示为方框，安装 `fonts-noto-cjk` 后重新启动；也可指定字体：

```bash
RUSTPHOTOSHOP_FONT="/路径/中文字体.ttf" ./target/release/rustphotoshop
```

TTC 集合还可使用 `RUSTPHOTOSHOP_FONT_INDEX` 指定字体索引。

| 问题 | 检查方法 |
| --- | --- |
| 桌面双击无反应 | 查看 `~/.local/state/rustphotoshop/rustphotoshop.log`；GNOME 可右键快捷方式选择「允许启动」 |
| 中文显示方框 | 运行 `--self-test-verbose` 查看中文字体结果，检查 `fc-match "sans-serif:lang=zh-cn"` |
| 文件对话框不出现 | 检查当前桌面的 XDG Desktop Portal 服务 |
| 缺少图标或资源 | 检查安装资源；可用 `RUSTPHOTOSHOP_ASSETS` 指定 `assets/` 目录 |
| 抖动滤镜不可用 | 安装 clang 和 Blocks 运行时后重新构建 |
| 无图形显示环境 | 使用批量导出或自检；GUI 启动需要有效的 X11/Wayland 会话 |

「帮助 → 诊断信息」会展示资源路径、字体和日志。XDG 路径可通过 `XDG_DATA_HOME`、`XDG_CONFIG_HOME`、`XDG_CACHE_HOME`、`XDG_STATE_HOME` 覆盖。

## 功能边界

Linux 版当前尚未接入以下上游能力：

- 可编辑文字、图层特效、剪贴蒙版和 Camera Raw；
- 形状/渐变工具、仿制图章、对象/主体选择；
- 自由变换手柄、多文档标签页、参考线与网格交互；
- PSD 分层编辑与 PSD/PSB 导出；
- 上游 Metal GPU 图像合成与大文档加速。

对已变换图层的选区编辑、复杂分组与高级元数据保存等路径，也应按具体操作检查结果。详细限制见 [KNOWN-LIMITATIONS.md](compositor-rs/KNOWN-LIMITATIONS.md)。

## 工程结构

```text
.
├── README.md                     项目首页
├── DESCRIPTION.md                少于 300 字符的项目简介
├── LICENSE                       MIT 许可与上游版权声明
├── Compositor/                   保留的 macOS Swift/AppKit/Metal 源码
├── Compositor.xcodeproj/          上游 Xcode 工程
├── CompositorTests/              上游 macOS 测试
├── docs/                         上游工程格式文档
├── 图标/                          图标原图
└── compositor-rs/                RustPhotoShop Linux 工程
    ├── Cargo.toml / Cargo.lock    crate rustphotoshop 与锁定依赖
    ├── build.rs                  C 内核编译与 Blocks 运行时检测
    ├── src/main.rs               进程入口
    ├── src/app/                  状态、命令行、窗口、自检
    ├── src/ui/                   菜单、画布、面板、对话框、字体
    ├── src/core/                 文档、图层、选区、合成、历史、编辑
    ├── src/io/                   工程和图片读写、PSD/PSB 读取
    ├── src/ffi/                  C 内核封装
    ├── src/platform/             Linux 路径、日志、桌面集成
    ├── csrc/reused/              9 个上游 C 内核及头文件
    ├── assets/                   图标、桌面项模板、MIME 定义
    ├── scripts/                  构建、启动、安装、测试、资源生成
    ├── packaging/build-deb.sh     Debian 包构建
    └── tests/                    CLI、桌面、格式和 UI 集成测试
```

## macOS 上游源码

上游 [Compositor](https://github.com/robbietilton/Compositor) 使用 SwiftUI、AppKit 和 Metal，功能范围比当前 Linux 版更完整。保留其源码便于对照语义、验证工程格式和继续迁移。

仓库内的上游源码是 1.4.5 时期的快照（对应本仓库版本号），上游此后仍在继续更新。README 与测试所依据的上游行为以这份快照为准；若要跟进较新实现，需要重新获取上游源码并运行 `compositor-rs/scripts/sync-kernels.sh` 同步 C 内核，再核对迁移中的功能差异。

原版要求 Apple silicon、macOS 26.0+ 和 Xcode 26+：

```bash
xcodebuild -project "Compositor.xcodeproj" \
  -scheme Compositor -destination 'platform=macOS' build

xcodebuild -project "Compositor.xcodeproj" \
  -scheme Compositor -destination 'platform=macOS' \
  test -only-testing:CompositorTests
```

上游下载与 Homebrew 安装入口面向 Compositor 的 macOS 发行版，不提供本项目的 Linux 安装包。根目录 `scripts/release.sh` 用于原版 macOS 签名、公证与 dmg 打包；Linux 请使用 `compositor-rs/packaging/build-deb.sh`。

## 开发与文档

修改图像处理时，优先复用已有 C 内核和 `core/` 操作；界面、文件 IO 与平台集成保持独立。`csrc/reused/` 源码通过一致性测试与上游对照，更新时运行 `scripts/sync-kernels.sh`。图标生成脚本为 `scripts/make-icons.sh`，二者均从 `compositor-rs/` 执行。

| 文档 | 内容 |
| --- | --- |
| [Linux 工程入口](compositor-rs/README.md) | RustPhotoShop 开发、运行与脚本入口 |
| [BUILD.md](compositor-rs/BUILD.md) | 构建依赖、安装、路径与故障排查 |
| [TEST.md](compositor-rs/TEST.md) | 测试体系与人工验证清单 |
| [TEST-REPORT.md](compositor-rs/TEST-REPORT.md) | 最近一次验证结果 |
| [ANALYSIS.md](compositor-rs/docs/ANALYSIS.md) | 原仓库分析与迁移决策 |
| [KNOWN-LIMITATIONS.md](compositor-rs/KNOWN-LIMITATIONS.md) | 未迁移功能与行为差异 |
| [资源说明](compositor-rs/assets/README.md) | 图标、桌面模板和 MIME 定义 |

## 许可与来源

项目采用 [MIT License](LICENSE)。原 Compositor 源码和 C 内核保留原版权声明，感谢上游作者与贡献者。

本地图标采用本仓库提供的原图；软件的 MIT 许可不自动声明该图像的独立授权范围。

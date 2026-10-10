# 仓库现状分析与迁移方案

这份文档回答「现状是什么、哪些复用、哪些重写」的问题，也是本目录（`compositor-rs`）存在
的理由。分析的对象是仓库根目录的 macOS 项目（`Compositor/`、`CompositorTests/`）。

## 1. 项目当前结构是什么

```
Compositor-1.4.5/
├── Compositor/                 142 个 Swift 文件，33,658 行
│   ├── CompositorApp.swift     @main 入口（SwiftUI）
│   ├── ContentView.swift       主窗口布局
│   ├── Document/               文档模型与编辑命令（58 个文件）
│   ├── IO/                     工程读写、PSD 读取、导入导出（12 个文件）
│   │   └── PSD/                Photoshop 解析（6 个文件）
│   ├── Rendering/              画布与 GPU 渲染（Metal），外加 9 个 C 像素内核
│   └── UI/                     面板、工具条、对话框（39 个文件）
├── CompositorTests/            96 个 Swift 测试文件（XCTest / Swift Testing）
├── CompositorUITests/          XCUITest 界面测试
├── Config/                     Info.plist、entitlements
├── docs/                       project-format.md、writing-comp-files.md、brush-performance.md
├── scripts/                    dmg 打包与发布脚本
├── Compositor.xcodeproj/       Xcode 工程
└── 图标/                        应用图标源图（2839×2839 PNG）
```

## 2. 入口程序在哪里

`Compositor/CompositorApp.swift`（SwiftUI `@main`）+ `Compositor/IO/CompositorApplicationDelegate.swift`。
窗口、菜单、快捷键都挂在 SwiftUI 场景上。

## 3. GUI 使用了什么技术栈

SwiftUI + AppKit（窗口、面板、拖动、上下文菜单），画布与图层特效走 Metal（`GPUCanvas.swift`、
`MetalLayerEffects.swift`、`MetalWarp.swift`），部分合成走 Core Graphics/Core Image；文字用
Core Text，图片编解码用 ImageIO。**这套栈在 Linux 上没有任何可移植性**。

## 4. 核心图像编辑能力有哪些

按 README 与源码归纳：

- 图层：图层与文件夹（直通式）、24 种混合模式、不透明度、图层蒙版（含未链接蒙版）、
  剪贴蒙版、调整图层、图层特效（描边/阴影/发光等）、合并/拼合、拖拽排序与编组
- 变换：移动/缩放/旋转/翻转/自由变形、吸附与参考线、数值精确调整
- 选区：矩形/椭圆选框、套索、魔棒、对象选择、主体选择、扩展/收缩/羽化、内容感知填充
- 绘画与修饰：画笔（硬度/不透明度/平滑）、橡皮擦、污点修复、仿制图章、模糊工具、
  渐变/形状工具、文字工具、吸管与拾色器
- 调整与滤镜：Camera Raw、色阶、曲线、色相/饱和度、曝光、渐变映射、颗粒、黑白、
  色彩平衡、反相、高斯模糊、动感模糊、添加杂色、暗角、镜头校正、去背等
- 画布与文件：多标签工程、参考线与网格、裁剪/画布大小/图像大小/修整、PSD/PSB 与各类
  图片导入、JPEG 导出、`.comp` 工程格式、Photoshop 风格快捷键

## 5. 当前是否已经支持 Linux

不支持。源码依赖 Foundation/CoreGraphics/Metal/ImageIO/UniformTypeIdentifiers，CI 也只跑
macOS（`.github/workflows/*.yml`，`runs-on: macos-26`）。仓库内没有 Linux 相关的构建、
打包或平台适配代码。

## 6. 当前是否已有图标、桌面文件、启动器、安装脚本

- 图标：`Compositor/Assets.xcassets/AppIcon.appiconset`（macOS `AppIcon` 资源）与根目录
  `图标/30f8cfffc3d09087aa (Edited).png`（2839×2839，sRGB，完全不透明）
- 桌面文件/启动器/安装脚本：没有。`scripts/` 只包含 dmg 打包与 Sparkle 更新发布脚本

## 7. 当前构建方式是什么

`xcodebuild -project Compositor.xcodeproj -scheme Compositor`（本机需要 Xcode 26 与
macOS 26 SDK）。发布走 `scripts/release.sh`：签名 → 公证 → dmg。

## 8. 当前测试体系是什么

`CompositorTests`（96 个文件）覆盖文档模型、渲染、PSD、导出、工具与快捷键等；
`CompositorUITests` 做界面测试；CI 在每次 push/PR 上执行。

## 9. 哪些部分适合直接保留，哪些适合重构为 Rust

| 部分 | 结论 | 理由 |
|------|------|------|
| `Compositor/Rendering/*.c`（9 个内核，2,284 行） | **原样保留并复用** | 纯 C，只用地道头文件，操作的是预乘 RGBA，平台无关 |
| `.comp` 工程格式（manifest.json + PNG 图层） | **原样保留** | 格式与实现语言无关，是可跨平台互通的契约 |
| PSD/PSB 读取逻辑 | **重写** | Swift + ImageIO/CoreGraphics 实现，无法跨平台；格式本身是公开规范 |
| 文档模型与编辑命令（Document/） | **重写为 Rust core** | 与 AppKit 类型（CGImage/CGPoint）耦合，但语义可以照搬 |
| 图层面板/工具条/对话框（UI/） | **重写** | SwiftUI 专有 |
| Metal/GPU 渲染（Rendering/*.swift） | **重写** | Metal 专有；Linux 侧用 CPU 合成 + egui 纹理 |
| 文本工具、图层特效、Camera Raw 等 | **暂缓** | 工程量最大、且不影响核心编辑路径 |

## 最小风险迁移方案（实际采用的方案）

1. **包裹而非拆改**：不触碰 macOS 工程，在同一仓库新建 `compositor-rs/`，两边共用
   `.comp` 格式与 C 内核。
2. **零改动复用 C 内核**：`csrc/reused/` 与上游逐字节一致，`build.rs` 直接编译它们；
   `tests/kernel_parity.rs` 用 sha256 守住「零改动」这条线，`scripts/sync-kernels.sh` 负责
   在 C 内核更新后重新同步。
3. **Rust 侧只做包装**：`src/ffi` 提供安全封装（尺寸、生命周期、错误），不重写像素算法。
4. **重写的是外壳**：文档模型、合成器、编辑命令、文件读写、界面（egui），全部可以从
   Swift 侧对照语义实现，并且都有单元测试守着。
5. **先能跑，再补全**：第一阶段保证「能构建、能启动、能双击、能打开/编辑/保存」，
   未实现的高级功能在 `KNOWN-LIMITATIONS.md` 里逐条列出，并且在读入工程时**保留原始
   元数据**，不因移植而丢数据。

### 各模块的处理方式

| 模块 | 处理 |
|------|------|
| `src/ffi/` + `csrc/reused/` | 上游 C 内核（魔棒、内容感知填充、污点修复、色阶、噪点、镜头畸变、颗粒、渐变映射、黑白、色彩平衡、暗角、色调对比、抖动、alpha 边界与提取） |
| `src/core/` | 新写：像素缓冲、24 种混合模式、文档/图层/蒙版/组、合成器、选区、历史、编辑命令、调整与滤镜 |
| `src/io/` | 新写：`.comp` 读写（兼容 macOS、保留未知字段）、PNG/JPEG/WebP/TIFF/BMP、PSD/PSB 导入 |
| `src/app/` | 新写：状态机、命令行、窗口、自检 |
| `src/ui/` | 新写：菜单、工具条、画布、图层面板、对话框、字体回退（中文界面） |
| `src/platform/` | 新写：XDG 目录、日志、`.desktop`/图标/MIME 安装器 |

### 需要补测试的部分（已补齐）

- 内核层：`tests/kernel_parity.rs`（与上游 sha256 一致）＋ 各内核的行为测试
- 编辑层：`src/core/ops.rs`、`src/core/composite.rs`、`src/core/document.rs` 的单元测试
- 格式层：`tests/project_format.rs`（手写工程可开、本版本工程合规、组/蒙版/调整图层往返）
- 界面层：`tests/ui_headless.rs`（无需窗口即可跑完整帧，校验面板与中文文案）
- 交付层：`tests/cli_and_desktop.rs`（真实二进制、批量导出、安装器与 `.desktop` 校验）
- 启动层：`--smoke-test`（真实开窗绘制若干帧后退出）

### 需要补打包的部分（已补齐）

`compositor --install`（用户级安装：可执行文件、7 种尺寸图标、`.desktop`、桌面图标、MIME）、
`scripts/install.sh`、`scripts/uninstall.sh`、`packaging/build-deb.sh`（.deb 包）。

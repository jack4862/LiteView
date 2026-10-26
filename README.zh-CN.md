[English](README.md) | 简体中文

# LiteView

LiteView 是一个本地优先、只读的 Windows 10/11 x64 桌面图片查看器。它使用 Rust、
`eframe`/`egui` 和 `wgpu` 构建。

0.1 版实现了可用的功能 MVP：快速查看单张图片、浏览当前目录、后台解码、大尺寸 JPEG 细节
瓦片、偏好设置持久化，以及当前用户级 Windows 安装。可选格式、HDR、编辑、云服务和 AI 都
刻意排除在当前范围之外。

## 功能

- 支持通过命令行、文件对话框、拖放和 Windows“打开方式”打开 JPEG、PNG 和静态 WebP。
- 应用全部 8 类 EXIF 方向，保留透明通道，显示灰度图像，并将嵌入 ICC 配置转换为 SDR sRGB。
  ICC 数据异常时给出警告，但不会丢弃图像像素。
- 仅浏览当前目录，并按自然文件名顺序排列；支持延迟缩略图网格或紧凑列表。前后图片会预加载，
  过期的后台任务会被取消。
- 提供适应窗口、100%、以光标为中心的滚轮/触控板缩放、拖拽平移、仅显示层 90 度旋转和全屏。
- 大尺寸 JPEG 使用 512 像素细节瓦片；预览、瓦片和缩略图 GPU 缓存共用可配置的预算。
- 显示原始/显示尺寸、文件大小、解码耗时、颜色状态和常见 EXIF 元数据。
- 可在“设置”中选择 English 或简体中文；默认跟随系统语言。

## 安装

先构建发布包。发布步骤会生成 `dist\LiteView-Setup.exe`，双击它即可按当前用户安装。传统的脚本
安装方式仍可在仓库根目录使用：

```powershell
powershell.exe -ExecutionPolicy Bypass -File .\packaging\build-release.ps1
powershell.exe -ExecutionPolicy Bypass -File .\dist\LiteView\install.ps1
```

安装器按当前用户运行，不需要管理员权限。它安装到 `%LOCALAPPDATA%\Programs\LiteView`，把
LiteView 注册为 `.jpg`、`.jpeg`、`.png` 和 `.webp` 的可选处理程序，并且不会修改现有默认
文件关联。升级、默认应用和卸载说明见 `packaging\README.md`。

## 快捷键

| 操作 | 快捷键 |
| --- | --- |
| 打开图片 | `Ctrl+O` |
| 上一张 / 下一张 | `Left` / `Right` |
| 在目录视图中移动选择 | `Left` / `Right`，然后 `Enter` 打开 |
| 放大 / 缩小 | `+` / `-` 或鼠标滚轮 |
| 适应窗口 | `0` |
| 实际尺寸（100%） | `1` |
| 顺时针旋转视图 | `R` |
| 显示 / 隐藏图像信息 | `I` |
| 显示 / 隐藏目录浏览器 | `D` |
| 打开设置 | `Ctrl+Shift+,` |
| 切换全屏 | `F` |
| 关闭浮层或退出全屏 | `Esc` |

双击查看区域可在“适应窗口”和“100%”之间切换。旋转和其他查看操作永远不会修改源文件。

## 构建与测试

仓库锁定 `stable-x86_64-pc-windows-gnu`。请使用较新的 WinLibs MinGW-w64 工具链，并保持检出
路径只包含 ASCII 字符：

```powershell
$mingw = "C:\tools\winlibs\mingw64\bin"
$env:PATH = "$mingw;$env:PATH"

cargo run -- "C:\images\sample.jpg"
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --release -- -D warnings
cargo test --all-targets -j 1
cargo build --release --locked
```

`packaging\build-release.ps1` 会执行 release 构建、生成许可证说明，并创建 `dist\LiteView`。
同时会创建一键安装器 `dist\LiteView-Setup.exe`。它默认使用已安装的 WinLibs 16.2 工具链和
仓库内 `target` 目录；两者都可以通过脚本参数覆盖。

基准命令会反复加载并准备预览，但不会测量完整 UI 帧时间、GPU 上传或首次冷启动：

```powershell
cargo run --release -- --benchmark "C:\images\large.jpg" --iterations 20
```

若要构建发布版并对本地目录中的所有支持基准图生成汇总报告，请使用：

```powershell
.\packaging\run-benchmarks.ps1 -ImagesDirectory "C:\path\to\liteview-benchmarks" -Iterations 20
```

基准报告会写入 `docs\benchmarks\benchmark-results.json` 和 `benchmark-results.md`。推荐基准集
见 `docs\benchmarks\README.md`。

## 项目结构

- `src/main.rs`：命令行入口、基准入口和原生窗口启动。
- `src/app.rs` 和 `src/app/`：UI、输入、目录与缩略图服务、后台任务协调、预览/瓦片缓存和绘制。
- `src/loader.rs`：内容类型探测、解码限制、EXIF 方向、ICC 转换和预览缩放。
- `src/wic.rs` 与 `src/tile_*.rs`：Windows JPEG 缩放/局部解码和瓦片细节渲染。
- `src/config.rs`：原子化 INI 设置读写。用户设置位于 `%LOCALAPPDATA%\LiteView\settings.ini`。
- `tests/`：使用生成夹具的单元测试和契约测试，覆盖全部 EXIF 方向、目录导航、ICC 降级、缓存
  限制、后台任务取消和瓦片行为。
- `packaging/`：当前用户级安装、卸载和发布脚本。

## 文档

- 产品与架构规格：[`docs/product-spec.zh-CN.md`](docs/product-spec.zh-CN.md)。
- 打包、安装与卸载说明：[`packaging/README.md`](packaging/README.md)。
- 基准测试流程：[`docs/benchmarks/README.md`](docs/benchmarks/README.md)。

## 参与贡献

提交前请阅读 [`CONTRIBUTING.zh-CN.md`](CONTRIBUTING.zh-CN.md) 了解配置和验证要求。打开 PR 前，请运行格式化、严格
Clippy、测试和发布构建。生成的基准报告不要提交到仓库。

## 已知限制

- 当前仅支持 Windows x64 上的 JPEG、PNG 和静态 WebP。动画 WebP、AVIF、HEIC、JPEG XL、RAW、
  HDR、编辑、批量操作和 AI 均未实现。
- 输入上限为 1.5 亿像素和 512 MiB 解码内存分配；预览长边上限为 4096 像素。
- 细节瓦片当前只用于 JPEG。PNG 和 WebP 可能需要完整解码后才能生成预览缩放，也不会提供
  按需高分辨率瓦片。
- 颜色输出是 8 位 SDR sRGB。显示器 ICC 配置、广色域输出和 HDR 均不受管理。
- 每次启动都会打开独立进程；没有单实例 IPC。PowerShell 安装器未签名，也不是 MSI。

## 隐私

LiteView 不发起网络请求，也不修改打开的图片或其所在目录。设置和窗口状态只保存在当前用户
配置目录中。

# LiteView

English | [简体中文](README.zh-CN.md)

LiteView is a local-first, read-only desktop image viewer for Windows 10/11 x64. It is built with
Rust, `eframe`/`egui`, and `wgpu`.

Version 0.1 implements the functional MVP: fast single-image viewing, current-directory browsing,
background decoding, large-JPEG detail tiles, persisted preferences, and current-user Windows
installation. Optional formats, HDR, editing, cloud services, and AI are intentionally out of scope.

## Features

- Open JPEG, PNG, and static WebP from the command line, file dialog, drag and drop, or Windows
  "Open with" integration.
- Apply all eight EXIF orientation values, preserve alpha, display grayscale images, and convert
  embedded ICC profiles to SDR sRGB. Invalid ICC data produces a warning without discarding pixels.
- Browse only the current directory in natural filename order, with a lazy thumbnail grid or compact
  list. Previous and next previews are preloaded and stale background work is cancelled.
- Fit, 100%, cursor-centered wheel/pinch zoom, drag panning, display-only 90-degree rotation, and
  fullscreen.
- Use 512-pixel detail tiles for large JPEGs while keeping preview, tile, and thumbnail GPU caches
  within a configurable budget.
- Show source/display dimensions, file size, decode time, color status, and common EXIF metadata.
- Choose English or Simplified Chinese from Settings. By default, the UI follows the system language.

## Install

Build the package first. The release step creates `dist\LiteView-Setup.exe`; double-click it to
install for the current user. The traditional script remains available from the repository root:

```powershell
powershell.exe -ExecutionPolicy Bypass -File .\packaging\build-release.ps1
powershell.exe -ExecutionPolicy Bypass -File .\dist\LiteView\install.ps1
```

The installer is per-user and requires no administrator access. It installs under
`%LOCALAPPDATA%\Programs\LiteView`, registers LiteView as an available handler for
`.jpg`, `.jpeg`, `.png`, and `.webp`, and leaves existing default associations unchanged.
See `packaging\README.md` for upgrade, default-app, and uninstall instructions.

## Controls

| Action | Shortcut |
| --- | --- |
| Open file | `Ctrl+O` |
| Previous / next image | `Left` / `Right` |
| Move selection in directory view | `Left` / `Right`, then `Enter` to open |
| Zoom in / out | `+` / `-` or mouse wheel |
| Fit to window | `0` |
| Actual size (100%) | `1` |
| Rotate view clockwise | `R` |
| Toggle image information | `I` |
| Toggle directory browser | `D` |
| Open settings | `Ctrl+Shift+,` |
| Toggle fullscreen | `F` |
| Close overlay or leave fullscreen | `Esc` |

Double-click the viewer to toggle fit and 100%. Rotation and all other viewing operations never
modify source files.

## Build And Test

The repository pins `stable-x86_64-pc-windows-gnu`. Use a current WinLibs MinGW-w64 toolchain and
keep the checkout path ASCII-only:

```powershell
$mingw = "C:\tools\winlibs\mingw64\bin"
$env:PATH = "$mingw;$env:PATH"

cargo run -- "C:\images\sample.jpg"
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --release -- -D warnings
cargo test --all-targets -j 1
cargo build --release --locked
```

`packaging\build-release.ps1` performs the release build, generates the license notice, and creates
`dist\LiteView` plus the one-click `dist\LiteView-Setup.exe`. It defaults to the installed WinLibs
16.2 toolchain and the repository-local `target` directory; both can be overridden with script
parameters.

The benchmark command loads and prepares a preview repeatedly, but does not measure complete UI
frame time, GPU upload, or first-run cold start:

```powershell
cargo run --release -- --benchmark "C:\images\large.jpg" --iterations 20
```

To build the release executable and summarize every supported fixture in a local directory, use:

```powershell
.\packaging\run-benchmarks.ps1 -ImagesDirectory "C:\path\to\liteview-benchmarks" -Iterations 20
```

The harness writes `docs\benchmarks\benchmark-results.json` and `benchmark-results.md`; see
`docs\benchmarks\README.md` for the recommended fixture set.

## Project Layout

- `src/main.rs`: CLI, benchmark entry point, and native window startup.
- `src/app.rs` and `src/app/`: UI, input, directory and thumbnail services, worker coordination,
  preview/tile caches, and painting.
- `src/loader.rs`: content detection, decode limits, EXIF orientation, ICC conversion, and preview
  scaling.
- `src/wic.rs` and `src/tile_*.rs`: Windows JPEG scaled/region decoding and tiled detail rendering.
- `src/config.rs`: atomic INI settings load/save. User settings live in
  `%LOCALAPPDATA%\LiteView\settings.ini`.
- `tests/`: generated-fixture unit and contract tests, including all EXIF orientations, navigation,
  ICC fallback, cache limits, worker cancellation, and tile behavior.
- `packaging/`: per-user install, uninstall, and release scripts.

## Documentation

- Product and architecture specification: [`docs/product-spec.zh-CN.md`](docs/product-spec.zh-CN.md).
- Packaging, installation, and uninstall details: [`packaging/README.md`](packaging/README.md).
- Benchmark workflow: [`docs/benchmarks/README.md`](docs/benchmarks/README.md).

## Contributing

Read [`CONTRIBUTING.md`](CONTRIBUTING.md) for setup and verification expectations. Before opening
a pull request, run formatting, strict Clippy, tests, and the release build. Generated benchmark
reports should not be committed.

## Known Limitations

- Supported input is limited to JPEG, PNG, and static WebP on Windows x64. Animated WebP, AVIF,
  HEIC, JPEG XL, RAW, HDR, editing, batch operations, and AI are not implemented.
- Inputs are capped at 150 million pixels and 512 MiB of decoder allocation. Previews are capped at
  4096 pixels on the long edge.
- Detail tiles currently apply to JPEG only. PNG and WebP can require a full decode before preview
  scaling and do not receive on-demand high-resolution tiles.
- Color output is 8-bit SDR sRGB. Monitor ICC profiles, wide-gamut output, and HDR are not managed.
- Each launch opens an independent process; there is no single-instance IPC. The PowerShell
  installer is not code-signed and is not an MSI.

## Privacy

LiteView makes no network requests and does not modify opened images or their directories. Settings
and window state are stored only in the current user's profile.

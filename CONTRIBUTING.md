English | [简体中文](CONTRIBUTING.zh-CN.md)

# Contributing to LiteView

LiteView is a local-first, read-only image viewer. Keep changes scoped to that promise: no editing
of source images, no network calls, and no bundled AI models.

## Before You Start

Read [`AGENTS.md`](AGENTS.md) for repository conventions. For product and architecture boundaries,
see [`docs/product-spec.zh-CN.md`](docs/product-spec.zh-CN.md).

## Development Checks

Use a current WinLibs MinGW-w64 toolchain and keep the checkout path ASCII-only:

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --release -- -D warnings
cargo test --all-targets -j 1
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\packaging\build-release.ps1
```

## Pull Requests

- Use Conventional Commits, for example `feat: add language switcher`.
- Explain the user-visible behavior and tradeoffs.
- Link related issues.
- Include screenshots for UI changes and benchmark results for performance-sensitive changes.
- Keep generated benchmark reports and local fixtures out of the repository.

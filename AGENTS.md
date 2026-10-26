# Repository Guidelines

LiteView is an early-stage, local-first desktop image viewer specified for Rust and `egui`. This checkout currently contains only `deepseek_markdown_20261025_5f18da.md`; there is no Cargo manifest, source tree, tests, or Git history. Treat the specification as the product scope and do not silently broaden it.

## Project Structure & Module Organization

- `deepseek_markdown_20261025_5f18da.md` is the current product and architecture specification.
- Keep `src/main.rs` as a thin desktop entry point. Place UI and application state in `src/app/`, decoding and preloading in `src/image/`, GPU tiled rendering in `src/render/`, and EXIF/filesystem work in `src/metadata/`.
- Keep optional, default-disabled AI features isolated in `src/ai/`.
- Put cross-module tests in `tests/` and small, redistributable fixtures in `assets/`.

## Build, Test, and Development Commands

No runnable commands exist until `Cargo.toml` is added. After scaffolding, use:

```sh
cargo run                 # Run the desktop application
cargo build --release     # Produce an optimized build
cargo test                # Run unit and integration tests
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
```

## Coding Style & Naming Conventions

Follow default `rustfmt` formatting with four-space indentation. Use `snake_case` for functions, variables, and modules; `PascalCase` for types and traits; and `SCREAMING_SNAKE_CASE` for constants. Return `Result` for recoverable failures, avoid panics and unchecked `unwrap` calls in runtime paths, and keep decode or render work off the UI thread. Network access must remain opt-in and outside default viewing flows.

## Testing Guidelines

Use Rust's built-in test framework. Keep focused unit tests beside implementations with `#[cfg(test)]` and put cross-module workflows in `tests/`. Name tests by behavior, for example `rejects_corrupt_icc_profile`, and test ICC handling, tiled rendering, cache eviction, malformed images, and adjacent-image preloading. Tests must be deterministic and offline; use licensed fixtures and verify at least 80% coverage for changed code. Run `cargo test` before submission.

## Commit & Pull Request Guidelines

No commit history is available in this checkout, so use Conventional Commits: `feat: add ICC profile conversion`, `fix: cap tile VRAM usage`, or `docs: clarify offline behavior`. Pull requests should explain scope and tradeoffs, link the relevant issue, list verification commands, and include screenshots for UI changes or benchmark results for performance-sensitive work.

## Security & Configuration

Never commit credentials or user images. Validate file paths and decode untrusted formats defensively. Keep AI models unbundled, explicitly downloaded only after user consent, and disabled by default.

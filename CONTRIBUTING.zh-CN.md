[English](CONTRIBUTING.md) | 简体中文

# 参与 LiteView 开发

LiteView 是本地优先、只读的图片查看器。请让改动保持在这个边界内：不修改源图片、不发起网络
请求、不内置 AI 模型。

## 开始之前

请先阅读 [`AGENTS.md`](AGENTS.md) 了解仓库约定。产品与架构边界见
[`docs/product-spec.zh-CN.md`](docs/product-spec.zh-CN.md)。

## 开发检查

请使用较新的 WinLibs MinGW-w64 工具链，并保持仓库路径只包含 ASCII 字符：

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --release -- -D warnings
cargo test --all-targets -j 1
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\packaging\build-release.ps1
```

## Pull Request

- 使用 Conventional Commits，例如 `feat: add language switcher`。
- 说明用户可见行为和取舍。
- 关联相关 issue。
- UI 改动提供截图；性能敏感改动提供基准结果。
- 生成的基准报告和本地测试图片不要提交到仓库。

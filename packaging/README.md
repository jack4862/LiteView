# LiteView Windows 发布与安装

本目录提供无管理员权限的 Windows 发布、安装和卸载脚本。安装范围仅为当前用户，
不会写入 `Program Files`，也不会修改图片内容。

## 构建发布包

在仓库根目录运行 PowerShell：

```powershell
pwsh -File .\packaging\build-release.ps1
```

Windows PowerShell 5.1 可使用：

```powershell
powershell.exe -ExecutionPolicy Bypass -File .\packaging\build-release.ps1
```

脚本默认使用现有的 WinLibs MinGW 16.2 工具链和仓库内 `target` 构建缓存，执行
`cargo build --release --locked`。
如需其他工具链或缓存位置，可显式传入参数：

```powershell
pwsh -File .\packaging\build-release.ps1 `
  -MinGWPath "D:\tools\winlibs\mingw64\bin" `
  -TargetDirectory "D:\cargo-target\liteview"
```

若 release 版 `liteview.exe` 不存在或为空，脚本会报错退出。成功后可分发
`dist\LiteView`，其中包含程序、说明、许可证说明以及安装/卸载脚本，并生成一键安装器
`dist\LiteView-Setup.exe`。若仓库位于非 ASCII
路径，可用 `-TargetDirectory` 指定 ASCII 构建缓存；最终安装位置仍由用户目录决定。
Release 程序使用 Windows GUI 子系统，从资源管理器启动时不会出现控制台窗口。
发布包同时包含 `README.md`（English）和 `README.zh-CN.md`（简体中文）。
发布目录中的 `PACKAGING.md` 是本文档的随包副本。

## 安装与文件关联

最简单的安装方式是双击发布目录旁边的：

```text
dist\LiteView-Setup.exe
```

它会解压发布文件到临时目录，调用内置安装脚本，然后显示安装结果。整个过程按当前用户执行，
不会要求管理员权限。

脚本方式仍可在仓库根目录使用：

```powershell
powershell.exe -ExecutionPolicy Bypass -File ".\dist\LiteView\install.ps1"
```

重复运行会安全覆盖当前用户的 LiteView 文件，但安装前应先关闭 LiteView。脚本安装到
`%LOCALAPPDATA%\Programs\LiteView`，并注册以下当前用户项：

- `.jpg`、`.jpeg`、`.png`、`.webp` 的 `OpenWithProgids`
- `LiteView.Image` ProgID 及打开命令
- `RegisteredApplications` 中的 LiteView 默认应用能力
- `App Paths` 中的 `liteview.exe`

安装器不会写入扩展名默认值，也不会覆盖其他查看器的默认关联。安装后可在
“设置 > 应用 > 默认应用”中选择 LiteView，或运行
`install.ps1 -OpenDefaultApps` 直接打开设置页。资源管理器未立即刷新时，重新登录即可。

## 卸载与设置

```powershell
powershell.exe -ExecutionPolicy Bypass `
  -File "$env:LOCALAPPDATA\Programs\LiteView\uninstall.ps1"
```

卸载会删除程序目录、上述 LiteView 注册项，并仅清除明确指向 `LiteView.Image`
的扩展名默认值。用户图片和其他查看器关联不会被删除。

应用设置保存在 `%LOCALAPPDATA%\LiteView\settings.ini`。卸载默认保留该目录，
只有在明确需要清理设置时使用：

```powershell
powershell.exe -ExecutionPolicy Bypass `
  -File "$env:LOCALAPPDATA\Programs\LiteView\uninstall.ps1" -RemoveSettings
```

许可证信息由发布脚本生成到 `dist\LiteView\LICENSE-NOTICE.txt`。项目自身采用
`MIT OR Apache-2.0`，第三方 Rust 组件的许可证表达式也会列在该文件中。

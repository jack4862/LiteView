[CmdletBinding()]
param(
    [string]$MinGWPath = 'C:\Users\3286\AppData\Local\Microsoft\WinGet\Packages\BrechtSanders.WinLibs.POSIX.MSVCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\mingw64\bin',
    [string]$TargetDirectory = ''
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Get-RequiredFile {
    param([Parameter(Mandatory = $true)][string]$Path)

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "Required file not found: $Path"
    }

    return (Get-Item -LiteralPath $Path).FullName
}

function New-LiteViewInstaller {
    param(
        [Parameter(Mandatory = $true)][string]$SetupExecutable,
        [Parameter(Mandatory = $true)][string]$StagingDirectory,
        [Parameter(Mandatory = $true)][string]$Destination
    )

    $payloadFiles = @(Get-ChildItem -LiteralPath $StagingDirectory -File |
        Sort-Object -Property Name)
    if ($payloadFiles.Count -eq 0) {
        throw 'Cannot build the installer because the package staging directory is empty.'
    }

    $stream = [System.IO.File]::Create($Destination)
    $writer = [System.IO.BinaryWriter]::new($stream)
    try {
        $setupBytes = [System.IO.File]::ReadAllBytes($SetupExecutable)
        $writer.BaseStream.Write($setupBytes, 0, $setupBytes.Length)
        $payloadStart = $writer.BaseStream.Position
        foreach ($file in $payloadFiles) {
            $name = [System.Text.Encoding]::UTF8.GetBytes($file.Name)
            $contents = [System.IO.File]::ReadAllBytes($file.FullName)
            $writer.Write([uint32]$name.Length)
            $writer.Write([uint64]$contents.Length)
            $writer.BaseStream.Write($name, 0, $name.Length)
            $writer.BaseStream.Write($contents, 0, $contents.Length)
        }

        $writer.Write([uint32]$payloadFiles.Count)
        $writer.Write([uint64]$payloadStart)
        $writer.Write([byte[]](0x4C, 0x56, 0x50, 0x4B, 0x47, 0x30, 0x30, 0x31))
        $writer.Flush()
    }
    finally {
        $writer.Dispose()
        $stream.Dispose()
    }
}

if ($env:OS -ne 'Windows_NT') {
    throw 'Windows is required to build the LiteView release package.'
}

$repoRoot = [System.IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
$manifestPath = Get-RequiredFile -Path (Join-Path $repoRoot 'Cargo.toml')
$readmePath = Get-RequiredFile -Path (Join-Path $repoRoot 'README.md')
$readmeChinesePath = Get-RequiredFile -Path (Join-Path $repoRoot 'README.zh-CN.md')
$packagingReadmePath = Get-RequiredFile -Path (Join-Path $PSScriptRoot 'README.md')
$installScriptPath = Get-RequiredFile -Path (Join-Path $PSScriptRoot 'install.ps1')
$uninstallScriptPath = Get-RequiredFile -Path (Join-Path $PSScriptRoot 'uninstall.ps1')

$mingwBin = [System.IO.Path]::GetFullPath($MinGWPath)
if (-not (Test-Path -LiteralPath $mingwBin -PathType Container)) {
    throw "MinGW bin directory not found: $mingwBin. Pass -MinGWPath to select a current MinGW-w64 toolchain."
}

$gccPath = Join-Path $mingwBin 'gcc.exe'
if (-not (Test-Path -LiteralPath $gccPath -PathType Leaf)) {
    throw "gcc.exe was not found in the MinGW bin directory: $mingwBin"
}

$targetRoot = if ([string]::IsNullOrWhiteSpace($TargetDirectory)) {
    Join-Path $repoRoot 'target'
}
else {
    [System.IO.Path]::GetFullPath($TargetDirectory)
}
$targetProfile = 'x86_64-pc-windows-gnu'
$releaseDirectory = Join-Path (Join-Path $targetRoot $targetProfile) 'release'
$releaseExe = Join-Path $releaseDirectory 'liteview.exe'
$setupExecutable = Join-Path $releaseDirectory 'liteview-setup.exe'
$distRoot = Join-Path $repoRoot 'dist'
$distDirectory = [System.IO.Path]::GetFullPath((Join-Path (Join-Path $repoRoot 'dist') 'LiteView'))
$installerPath = Join-Path $distRoot 'LiteView-Setup.exe'
if ((-not [StringComparer]::OrdinalIgnoreCase.Equals((Split-Path -Parent $distDirectory), $distRoot)) -or
    ((Split-Path -Leaf $distDirectory) -ne 'LiteView')) {
    throw "Refusing unexpected release directory: $distDirectory"
}

$env:PATH = "$mingwBin;$env:PATH"
$env:CARGO_TARGET_DIR = $targetRoot
if (([string]$env:RUSTFLAGS) -notmatch '(^|\s)-A\s+linker_messages(\s|$)') {
    $env:RUSTFLAGS = "$([string]$env:RUSTFLAGS) -A linker_messages".Trim()
}

Push-Location -LiteralPath $repoRoot
try {
    & cargo build --release --locked --target $targetProfile --manifest-path $manifestPath
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build --release --locked failed with exit code $LASTEXITCODE."
    }

    $metadataText = (& cargo metadata --locked --format-version 1 --manifest-path $manifestPath | Out-String)
    if ($LASTEXITCODE -ne 0) {
        throw "cargo metadata failed with exit code $LASTEXITCODE; cannot generate the license notice."
    }
}
finally {
    Pop-Location
}

if (-not (Test-Path -LiteralPath $releaseExe -PathType Leaf)) {
    throw "Release executable not found after the build: $releaseExe"
}
if ((Get-Item -LiteralPath $releaseExe).Length -le 0) {
    throw "Release executable is empty: $releaseExe"
}
if (-not (Test-Path -LiteralPath $setupExecutable -PathType Leaf)) {
    throw "Installer executable not found after the build: $setupExecutable"
}
if ((Get-Item -LiteralPath $setupExecutable).Length -le 0) {
    throw "Installer executable is empty: $setupExecutable"
}

$metadata = $metadataText | ConvertFrom-Json
$projectPackage = @($metadata.packages | Where-Object { $_.name -eq 'liteview' }) | Select-Object -First 1
if ($null -eq $projectPackage) {
    throw 'LiteView package metadata was not found in cargo metadata output.'
}

$projectVersion = [string]$projectPackage.version
$projectLicense = [string]$projectPackage.license
if ([string]::IsNullOrWhiteSpace($projectLicense)) {
    $projectLicense = 'No SPDX expression declared'
}

$licenseLines = @()
foreach ($package in @($metadata.packages | Sort-Object -Property name, version)) {
    if (($package.name -eq 'liteview') -and ($package.version -eq $projectVersion)) {
        continue
    }

    $license = [string]$package.license
    if ([string]::IsNullOrWhiteSpace($license)) {
        if (-not [string]::IsNullOrWhiteSpace([string]$package.license_file)) {
            $license = "See license file $($package.license_file)"
        }
        else {
            $license = 'No SPDX expression declared; consult the package source'
        }
    }
    $licenseLines += "- $($package.name) $($package.version): $license"
}
$thirdPartyLicenses = $licenseLines -join [Environment]::NewLine

$licenseNotice = @"
LiteView $projectVersion - License Notice

LiteView is licensed under $projectLicense.
The bundled application also contains third-party Rust crates. Their declared
license expressions are listed below; the license text and notices supplied by
each crate govern that crate.

Third-party components:
$thirdPartyLicenses

This notice is generated by packaging/build-release.ps1. It is informational
and does not replace the complete license texts distributed by each project.
"@

New-Item -ItemType Directory -Path $distRoot -Force | Out-Null
$stagingDirectory = Join-Path $distRoot ('.LiteView-staging-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $stagingDirectory -Force | Out-Null

try {
    Copy-Item -LiteralPath $releaseExe -Destination (Join-Path $stagingDirectory 'liteview.exe') -Force
    Copy-Item -LiteralPath $readmePath -Destination (Join-Path $stagingDirectory 'README.md') -Force
    Copy-Item -LiteralPath $readmeChinesePath -Destination (Join-Path $stagingDirectory 'README.zh-CN.md') -Force
    Copy-Item -LiteralPath $packagingReadmePath -Destination (Join-Path $stagingDirectory 'PACKAGING.md') -Force
    Copy-Item -LiteralPath $installScriptPath -Destination (Join-Path $stagingDirectory 'install.ps1') -Force
    Copy-Item -LiteralPath $uninstallScriptPath -Destination (Join-Path $stagingDirectory 'uninstall.ps1') -Force
    Set-Content -LiteralPath (Join-Path $stagingDirectory 'LICENSE-NOTICE.txt') -Value $licenseNotice -Encoding UTF8

    New-LiteViewInstaller -SetupExecutable $setupExecutable -StagingDirectory $stagingDirectory -Destination $installerPath

    if (Test-Path -LiteralPath $distDirectory) {
        Remove-Item -LiteralPath $distDirectory -Recurse -Force
    }

    Move-Item -LiteralPath $stagingDirectory -Destination $distDirectory
    $stagingDirectory = $null
}
finally {
    if (($null -ne $stagingDirectory) -and (Test-Path -LiteralPath $stagingDirectory)) {
        Remove-Item -LiteralPath $stagingDirectory -Recurse -Force
    }
}

Write-Host "Release package created: $distDirectory"
Write-Host "Single-file installer created: $installerPath"
Get-ChildItem -LiteralPath $distDirectory -File |
    Sort-Object -Property Name |
    ForEach-Object { Write-Host "  $($_.Name)" }

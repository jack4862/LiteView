[CmdletBinding()]
param(
    [switch]$OpenDefaultApps
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Get-RequiredSourceFile {
    param([Parameter(Mandatory = $true)][string]$FileName)

    $path = Join-Path $PSScriptRoot $FileName
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "Package file not found: $path. Run this script from the extracted dist\LiteView directory."
    }

    return (Get-Item -LiteralPath $path).FullName
}

if ($env:OS -ne 'Windows_NT') {
    throw 'LiteView can only be installed on Windows.'
}

$localAppData = [Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)
if ([string]::IsNullOrWhiteSpace($localAppData)) {
    throw 'The LOCALAPPDATA directory could not be determined.'
}

$localAppDataFull = [System.IO.Path]::GetFullPath($localAppData)
$programsDirectory = [System.IO.Path]::GetFullPath((Join-Path $localAppDataFull 'Programs'))
$installDirectory = [System.IO.Path]::GetFullPath((Join-Path $programsDirectory 'LiteView'))
if ((-not [StringComparer]::OrdinalIgnoreCase.Equals((Split-Path -Parent $installDirectory), $programsDirectory)) -or
    ((Split-Path -Leaf $installDirectory) -ne 'LiteView')) {
    throw "Refusing unexpected install directory: $installDirectory"
}

$packageFiles = @(
    (Get-RequiredSourceFile -FileName 'liteview.exe'),
    (Get-RequiredSourceFile -FileName 'README.md'),
    (Get-RequiredSourceFile -FileName 'README.zh-CN.md'),
    (Get-RequiredSourceFile -FileName 'PACKAGING.md'),
    (Get-RequiredSourceFile -FileName 'LICENSE-NOTICE.txt'),
    (Get-RequiredSourceFile -FileName 'install.ps1'),
    (Get-RequiredSourceFile -FileName 'uninstall.ps1')
)

$installedExe = Join-Path $installDirectory 'liteview.exe'
$runningFromInstall = $false
foreach ($process in @(Get-Process -Name 'liteview' -ErrorAction SilentlyContinue)) {
    $processPath = $null
    try {
        $processPath = $process.Path
    }
    catch {
        continue
    }

    if ((-not [string]::IsNullOrWhiteSpace($processPath)) -and
        [StringComparer]::OrdinalIgnoreCase.Equals(
            [System.IO.Path]::GetFullPath($processPath),
            [System.IO.Path]::GetFullPath($installedExe)
        )) {
        $runningFromInstall = $true
        break
    }
}
if ($runningFromInstall) {
    throw "LiteView is running from $installedExe. Close it before installing or upgrading."
}

New-Item -ItemType Directory -Path $programsDirectory -Force | Out-Null
$stagingDirectory = Join-Path $programsDirectory ('.LiteView-staging-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $stagingDirectory -Force | Out-Null

try {
    foreach ($sourceFile in $packageFiles) {
        Copy-Item -LiteralPath $sourceFile -Destination (Join-Path $stagingDirectory (Split-Path -Leaf $sourceFile)) -Force
    }

    New-Item -ItemType Directory -Path $installDirectory -Force | Out-Null
    foreach ($sourceFile in $packageFiles) {
        Copy-Item -LiteralPath (Join-Path $stagingDirectory (Split-Path -Leaf $sourceFile)) -Destination $installDirectory -Force
    }
}
finally {
    if (Test-Path -LiteralPath $stagingDirectory) {
        Remove-Item -LiteralPath $stagingDirectory -Recurse -Force
    }
}

$progId = 'LiteView.Image'
$classesRoot = 'HKCU:\Software\Classes'
$progIdKey = Join-Path $classesRoot $progId
$commandKey = Join-Path (Join-Path (Join-Path $progIdKey 'shell') 'open') 'command'
$capabilitiesKey = Join-Path $progIdKey 'Capabilities'
$fileAssociationsKey = Join-Path $capabilitiesKey 'FileAssociations'
$applicationKey = Join-Path $progIdKey 'Application'
$appPathsKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\App Paths\liteview.exe'
$registeredApplicationsKey = 'HKCU:\Software\RegisteredApplications'
$exePath = Join-Path $installDirectory 'liteview.exe'
$supportedExtensions = @('.jpg', '.jpeg', '.png', '.webp')

New-Item -Path $progIdKey -Force | Out-Null
New-Item -Path $commandKey -Force | Out-Null
New-Item -Path $fileAssociationsKey -Force | Out-Null
New-Item -Path $applicationKey -Force | Out-Null
New-Item -Path $appPathsKey -Force | Out-Null
New-Item -Path $registeredApplicationsKey -Force | Out-Null

Set-Item -Path $progIdKey -Value 'LiteView Image'
$defaultIconKey = Join-Path $progIdKey 'DefaultIcon'
New-Item -Path $defaultIconKey -Force | Out-Null
Set-Item -Path $defaultIconKey -Value "`"$exePath`",0"
Set-Item -Path $commandKey -Value "`"$exePath`" `"%1`""
Set-Item -Path $appPathsKey -Value $exePath
New-ItemProperty -Path $applicationKey -Name 'ApplicationName' -Value 'LiteView' -PropertyType String -Force | Out-Null
New-ItemProperty -Path $applicationKey -Name 'ApplicationIcon' -Value "`"$exePath`",0" -PropertyType String -Force | Out-Null
New-ItemProperty -Path $capabilitiesKey -Name 'ApplicationName' -Value 'LiteView' -PropertyType String -Force | Out-Null
New-ItemProperty -Path $capabilitiesKey -Name 'ApplicationDescription' -Value 'Fast, local-first image viewer' -PropertyType String -Force | Out-Null
New-ItemProperty -Path $capabilitiesKey -Name 'ApplicationIcon' -Value "`"$exePath`",0" -PropertyType String -Force | Out-Null
New-ItemProperty -Path $appPathsKey -Name 'Path' -Value $installDirectory -PropertyType String -Force | Out-Null
New-ItemProperty -Path $registeredApplicationsKey -Name 'LiteView' -Value "Software\Classes\$progId\Capabilities" -PropertyType String -Force | Out-Null

foreach ($extension in $supportedExtensions) {
    New-ItemProperty -Path $fileAssociationsKey -Name $extension -Value $progId -PropertyType String -Force | Out-Null

    $openWithKeyPath = Join-Path (Join-Path $classesRoot $extension) 'OpenWithProgids'
    New-Item -Path $openWithKeyPath -Force | Out-Null

    # OpenWithProgids entries use REG_NONE, so set this value through the .NET registry API.
    $openWithKey = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey(
        "Software\Classes\$extension\OpenWithProgids"
    )
    if ($null -eq $openWithKey) {
        throw "Could not open registry key for $extension."
    }
    try {
        $openWithKey.SetValue($progId, [byte[]]@(), [Microsoft.Win32.RegistryValueKind]::None)
    }
    finally {
        $openWithKey.Dispose()
    }
}

Write-Host "LiteView installed for the current user: $installDirectory"
Write-Host 'Registered .jpg, .jpeg, .png, and .webp as available LiteView associations.'
Write-Host 'Existing default file associations were not changed.'
Write-Host 'To choose defaults, open Settings > Apps > Default apps, or rerun install.ps1 with -OpenDefaultApps.'

if ($OpenDefaultApps) {
    Start-Process -FilePath 'ms-settings:defaultapps'
}

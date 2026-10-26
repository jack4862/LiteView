[CmdletBinding()]
param(
    [switch]$RemoveSettings
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Remove-CurrentUserValue {
    param(
        [Parameter(Mandatory = $true)][string]$SubKey,
        [Parameter(Mandatory = $true)][string]$Name
    )

    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($SubKey, $true)
    if ($null -eq $key) {
        return
    }

    try {
        $key.DeleteValue($Name, $false)
    }
    finally {
        $key.Dispose()
    }
}

function Remove-CurrentUserTree {
    param([Parameter(Mandatory = $true)][string]$SubKey)

    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($SubKey, $false)
    if ($null -eq $key) {
        return
    }

    $key.Dispose()
    [Microsoft.Win32.Registry]::CurrentUser.DeleteSubKeyTree($SubKey, $false)
}

function Remove-OwnedDefaultAssociation {
    param(
        [Parameter(Mandatory = $true)][string]$Extension,
        [Parameter(Mandatory = $true)][string]$ProgId
    )

    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey("Software\Classes\$Extension", $true)
    if ($null -eq $key) {
        return
    }

    try {
        $currentValue = $key.GetValue('', $null)
        if (($null -ne $currentValue) -and
            [string]::Equals([string]$currentValue, $ProgId, [StringComparison]::OrdinalIgnoreCase)) {
            $key.DeleteValue('', $false)
        }
    }
    finally {
        $key.Dispose()
    }
}

if ($env:OS -ne 'Windows_NT') {
    throw 'LiteView can only be uninstalled on Windows.'
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

$settingsDirectory = [System.IO.Path]::GetFullPath((Join-Path $localAppDataFull 'LiteView'))
if ((-not [StringComparer]::OrdinalIgnoreCase.Equals((Split-Path -Parent $settingsDirectory), $localAppDataFull)) -or
    ((Split-Path -Leaf $settingsDirectory) -ne 'LiteView')) {
    throw "Refusing unexpected settings directory: $settingsDirectory"
}

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
    throw "LiteView is running from $installedExe. Close it before uninstalling."
}

$progId = 'LiteView.Image'
$supportedExtensions = @('.jpg', '.jpeg', '.png', '.webp')

foreach ($extension in $supportedExtensions) {
    Remove-CurrentUserValue -SubKey "Software\Classes\$extension\OpenWithProgids" -Name $progId
    Remove-OwnedDefaultAssociation -Extension $extension -ProgId $progId
}
Remove-CurrentUserValue -SubKey 'Software\RegisteredApplications' -Name 'LiteView'
Remove-CurrentUserTree -SubKey "Software\Classes\$progId"
Remove-CurrentUserTree -SubKey 'Software\Microsoft\Windows\CurrentVersion\App Paths\liteview.exe'

if (Test-Path -LiteralPath $installDirectory -PathType Container) {
    $originalLocation = Get-Location
    Set-Location -LiteralPath $localAppData
    try {
        Remove-Item -LiteralPath $installDirectory -Recurse -Force
    }
    finally {
        if (Test-Path -LiteralPath $originalLocation.Path -PathType Container) {
            Set-Location -LiteralPath $originalLocation.Path
        }
    }
}

if ($RemoveSettings -and (Test-Path -LiteralPath $settingsDirectory -PathType Container)) {
    Remove-Item -LiteralPath $settingsDirectory -Recurse -Force
    Write-Host "Removed LiteView settings: $settingsDirectory"
}
else {
    Write-Host "Preserved LiteView settings: $settingsDirectory"
}

Write-Host 'LiteView registration and application files were removed for the current user.'
Write-Host 'User images were not processed or deleted.'

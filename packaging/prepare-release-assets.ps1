[CmdletBinding()]
param(
    [string]$RepositoryRoot = ''
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if ($env:OS -ne 'Windows_NT') {
    throw 'Release assets can only be prepared on Windows.'
}

if ([string]::IsNullOrWhiteSpace($RepositoryRoot)) {
    $RepositoryRoot = [System.IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
}

$repoRoot = [System.IO.Path]::GetFullPath($RepositoryRoot)
$packageDirectory = Join-Path $repoRoot 'dist\LiteView'
$installer = Join-Path $repoRoot 'dist\LiteView-Setup.exe'
$archive = Join-Path $repoRoot 'dist\LiteView-windows-x64.zip'
$checksums = Join-Path $repoRoot 'dist\SHA256SUMS.txt'

if (-not (Test-Path -LiteralPath $packageDirectory -PathType Container)) {
    throw "Release package not found: $packageDirectory"
}
if (-not (Test-Path -LiteralPath $installer -PathType Leaf)) {
    throw "Release installer not found: $installer"
}

if (Test-Path -LiteralPath $archive -PathType Leaf) {
    Remove-Item -LiteralPath $archive -Force
}

Compress-Archive -Path (Join-Path $packageDirectory '*') -DestinationPath $archive -Force

$hashedFiles = @($installer, $archive)
$checksumLines = foreach ($file in $hashedFiles) {
    $hash = (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash
    "$hash  $(Split-Path -Leaf $file)"
}
$checksumLines | Set-Content -LiteralPath $checksums -Encoding ASCII

Write-Host "Release archive created: $archive"
Write-Host "SHA256 checksums created: $checksums"

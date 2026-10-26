[CmdletBinding()]
param(
    [string]$ImagesDirectory,
    [string]$OutputDirectory = '',
    [ValidateRange(1, 1000)]
    [int]$Iterations = 5,
    [string]$MinGWPath = '',
    [string]$TargetDirectory = '',
    [switch]$SkipBuild
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Read-BenchmarkOutput {
    param([Parameter(Mandatory = $true)][string]$Path)

    $values = @{}
    foreach ($line in @(Get-Content -LiteralPath $Path)) {
        if ($line -match '^([^=]+)=(.*)$') {
            $values[$Matches[1]] = $Matches[2]
        }
    }

    $required = @(
        'format', 'source_dimensions', 'display_dimensions', 'preview_dimensions',
        'file_bytes', 'icc', 'downsampled', 'iterations', 'elapsed_ms_p50', 'elapsed_ms_p95'
    )
    foreach ($key in $required) {
        if (-not $values.ContainsKey($key)) {
            throw "Benchmark output is missing '$key'."
        }
    }

    return [pscustomobject]@{
        Format = [string]$values['format']
        SourceDimensions = [string]$values['source_dimensions']
        DisplayDimensions = [string]$values['display_dimensions']
        PreviewDimensions = [string]$values['preview_dimensions']
        FileBytes = [int64]$values['file_bytes']
        Icc = [string]$values['icc']
        Downsampled = [string]$values['downsampled']
        Iterations = [int]$values['iterations']
        ElapsedP50Ms = [double]$values['elapsed_ms_p50']
        ElapsedP95Ms = [double]$values['elapsed_ms_p95']
        ElapsedMaxMs = [double]$values['elapsed_ms_max']
    }
}

function ConvertTo-MarkdownTable {
    param([Parameter(Mandatory = $true)][AllowEmptyCollection()][object[]]$Results)

    $lines = @(
        '| Image | Format | Source | Preview | p50 (ms) | p95 (ms) | ICC |',
        '| --- | --- | --- | --- | ---: | ---: | --- |'
    )
    foreach ($result in $Results) {
        $lines += "| $($result.Image) | $($result.Format) | $($result.SourceDimensions) | " +
            "$($result.PreviewDimensions) | $($result.ElapsedP50Ms) | $($result.ElapsedP95Ms) | $($result.Icc) |"
    }

    return $lines
}

function Get-GitCommit {
    param([Parameter(Mandatory = $true)][string]$RepositoryRoot)

    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = 'git'
    $startInfo.Arguments = 'rev-parse HEAD'
    $startInfo.WorkingDirectory = $RepositoryRoot
    $startInfo.UseShellExecute = $false
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    $startInfo.CreateNoWindow = $true

    try {
        $process = [System.Diagnostics.Process]::Start($startInfo)
        $output = $process.StandardOutput.ReadToEnd().Trim()
        $process.StandardError.ReadToEnd() | Out-Null
        $process.WaitForExit() | Out-Null
        if ($process.ExitCode -eq 0 -and -not [string]::IsNullOrWhiteSpace($output)) {
            return $output
        }
    }
    catch {
        return ''
    }

    return ''
}

if ($env:OS -ne 'Windows_NT') {
    throw 'The LiteView benchmark harness requires Windows.'
}

$repoRoot = [System.IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
$executable = Join-Path $repoRoot 'dist\LiteView\liteview.exe'
if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
    $OutputDirectory = Join-Path $repoRoot 'docs\benchmarks'
}
$outputDirectoryFull = [System.IO.Path]::GetFullPath($OutputDirectory)
if (-not $outputDirectoryFull.StartsWith(
        ([System.IO.Path]::GetFullPath($repoRoot) + [System.IO.Path]::DirectorySeparatorChar),
        [System.StringComparison]::OrdinalIgnoreCase
    )) {
    throw "Refusing benchmark output outside the repository: $outputDirectoryFull"
}

if (-not $SkipBuild) {
    $buildArguments = @()
    if (-not [string]::IsNullOrWhiteSpace($MinGWPath)) {
        $buildArguments += @('-MinGWPath', $MinGWPath)
    }
    if (-not [string]::IsNullOrWhiteSpace($TargetDirectory)) {
        $buildArguments += @('-TargetDirectory', $TargetDirectory)
    }

    & (Join-Path $PSScriptRoot 'build-release.ps1') @buildArguments
}

if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) {
    throw "LiteView executable not found: $executable. Run without -SkipBuild first."
}
if ([string]::IsNullOrWhiteSpace($ImagesDirectory)) {
    throw 'ImagesDirectory is required. Point it at a local benchmark fixture directory.'
}

$imagesDirectoryFull = [System.IO.Path]::GetFullPath($ImagesDirectory)
if (-not (Test-Path -LiteralPath $imagesDirectoryFull -PathType Container)) {
    throw "Benchmark fixture directory not found: $imagesDirectoryFull"
}

$extensions = @('.jpg', '.jpeg', '.png', '.webp')
$images = @(
    Get-ChildItem -LiteralPath $imagesDirectoryFull -File |
        Where-Object { $extensions -contains $_.Extension.ToLowerInvariant() } |
        Sort-Object -Property Name
)
if ($images.Count -eq 0) {
    throw "No supported benchmark images found in $imagesDirectoryFull."
}

New-Item -ItemType Directory -Path $outputDirectoryFull -Force | Out-Null
$results = @()
foreach ($image in $images) {
    $temporaryRoot = Join-Path ([System.IO.Path]::GetTempPath()) ('liteview-benchmark-' + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $temporaryRoot -Force | Out-Null
    $stdoutPath = Join-Path $temporaryRoot 'stdout.txt'
    $stderrPath = Join-Path $temporaryRoot 'stderr.txt'

    try {
        $process = Start-Process `
            -FilePath $executable `
            -ArgumentList @('--benchmark', "`"$($image.FullName)`"", '--iterations', "$Iterations") `
            -NoNewWindow `
            -Wait `
            -PassThru `
            -RedirectStandardOutput $stdoutPath `
            -RedirectStandardError $stderrPath

        if ($process.ExitCode -ne 0) {
            $stderr = if (Test-Path -LiteralPath $stderrPath) { Get-Content -LiteralPath $stderrPath -Raw } else { '' }
            throw "Benchmark failed for $($image.Name) with exit code $($process.ExitCode). $stderr"
        }

        $measurement = Read-BenchmarkOutput -Path $stdoutPath
        $results += [pscustomobject]@{
            Image = $image.Name
            Path = $image.FullName
            Format = $measurement.Format
            SourceDimensions = $measurement.SourceDimensions
            DisplayDimensions = $measurement.DisplayDimensions
            PreviewDimensions = $measurement.PreviewDimensions
            FileBytes = $measurement.FileBytes
            Icc = $measurement.Icc
            Downsampled = $measurement.Downsampled
            Iterations = $measurement.Iterations
            ElapsedP50Ms = $measurement.ElapsedP50Ms
            ElapsedP95Ms = $measurement.ElapsedP95Ms
            ElapsedMaxMs = $measurement.ElapsedMaxMs
        }
    }
    finally {
        if (Test-Path -LiteralPath $temporaryRoot -PathType Container) {
            Remove-Item -LiteralPath $temporaryRoot -Recurse -Force
        }
    }
}

$rustVersion = (& cargo -vV | Select-String -Pattern '^release:').Line
$gitCommit = Get-GitCommit -RepositoryRoot $repoRoot

$report = [pscustomobject]@{
    GeneratedAtUtc = [DateTime]::UtcNow.ToString('o')
    Executable = $executable
    RustVersion = $rustVersion
    GitCommit = $gitCommit
    IterationsRequested = $Iterations
    Results = $results
}

$jsonPath = Join-Path $outputDirectoryFull 'benchmark-results.json'
$markdownPath = Join-Path $outputDirectoryFull 'benchmark-results.md'
$report | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $jsonPath -Encoding UTF8
$markdown = @(
    '# LiteView Benchmark Results',
    '',
    "- Generated: $($report.GeneratedAtUtc)",
    "- Toolchain: $($report.RustVersion)",
    "- Git commit: $(if ([string]::IsNullOrWhiteSpace($gitCommit)) { 'uncommitted' } else { $gitCommit })",
    "- Iterations requested: $Iterations",
    ''
) + (ConvertTo-MarkdownTable -Results $results)
$markdown | Set-Content -LiteralPath $markdownPath -Encoding UTF8

Write-Host "Benchmark report written: $jsonPath"
Write-Host "Benchmark summary written: $markdownPath"

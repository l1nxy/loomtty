<#
.SYNOPSIS
    Build the loomtty Windows installer (loomtty-<version>-x86_64-setup.exe).

.DESCRIPTION
    Wraps Inno Setup's ISCC compiler with the right defines. By default it uses
    the release binaries already in target\release; pass -Build to compile them
    first. The version is read from the workspace Cargo.toml unless overridden.

    Requires Inno Setup 6 (ISCC.exe). If it's missing, install it with:
        winget install -e --id JRSoftware.InnoSetup

.PARAMETER Version
    Override the version string. Defaults to [workspace.package] version in Cargo.toml.

.PARAMETER BinDir
    Directory holding loomtty.exe / loomtty-server.exe. Defaults to <repo>\target\release.

.PARAMETER Build
    Run `cargo build --release` for both binaries before packaging.

.PARAMETER Iscc
    Explicit path to ISCC.exe (otherwise auto-detected).

.EXAMPLE
    dist\windows\build-installer.ps1 -Build
#>
[CmdletBinding()]
param(
    [string]$Version,
    [string]$BinDir,
    [switch]$Build,
    [string]$Iscc
)

$ErrorActionPreference = 'Stop'

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$IssPath  = Join-Path $PSScriptRoot 'loomtty.iss'

if (-not $BinDir) {
    $BinDir = Join-Path $RepoRoot 'target\release'
} elseif (-not [System.IO.Path]::IsPathRooted($BinDir)) {
    # A relative BinDir (CI passes "target\<triple>\release") is interpreted
    # relative to the repo root. Make it absolute: ISCC resolves a relative
    # [Files] Source against the .iss directory (dist\windows), not our CWD, so
    # a relative BinDir would make Inno look under dist\windows\target\... and
    # the compile fails with "source file not found".
    $BinDir = Join-Path $RepoRoot $BinDir
}

# ── Version ────────────────────────────────────────────────────────────
if (-not $Version) {
    $cargo = Get-Content (Join-Path $RepoRoot 'Cargo.toml') -Raw
    $m = [regex]::Match($cargo, '(?ms)^\[workspace\.package\].*?^version\s*=\s*"([^"]+)"')
    if (-not $m.Success) { throw "Could not read version from Cargo.toml" }
    $Version = $m.Groups[1].Value
}
Write-Host "loomtty installer  version=$Version" -ForegroundColor Cyan

# ── Build binaries (optional) ──────────────────────────────────────────
if ($Build) {
    Write-Host "Building release binaries..." -ForegroundColor Cyan
    Push-Location $RepoRoot
    try {
        # gui-bin enables the GUI-subsystem launcher (loomtty-gui.exe).
        cargo build --release -p loomtty -p loomtty-server --features loomtty/gui-bin
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
    } finally { Pop-Location }
}

foreach ($exe in 'loomtty.exe', 'loomtty-gui.exe', 'loomtty-server.exe') {
    $p = Join-Path $BinDir $exe
    if (-not (Test-Path $p)) {
        throw "Missing $p`nBuild it first:  cargo build --release --features loomtty/gui-bin  (or pass -Build)"
    }
}

# ── Locate ISCC ────────────────────────────────────────────────────────
if (-not $Iscc) {
    $candidates = @(
        (Get-Command iscc -ErrorAction SilentlyContinue).Source,
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
        "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
        "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"  # winget per-user install
    )
    $Iscc = $candidates | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
}
if (-not $Iscc) {
    throw "ISCC.exe (Inno Setup 6) not found. Install it with:`n    winget install -e --id JRSoftware.InnoSetup"
}

# ── Compile ────────────────────────────────────────────────────────────
$OutputDir = Join-Path $RepoRoot 'target\installer'
New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null

& $Iscc `
    "/DMyAppVersion=$Version" `
    "/DSourceRoot=$RepoRoot" `
    "/DBinDir=$BinDir" `
    "/DOutputDir=$OutputDir" `
    $IssPath
if ($LASTEXITCODE -ne 0) { throw "ISCC failed with exit code $LASTEXITCODE" }

$setup = Join-Path $OutputDir "loomtty-$Version-x86_64-setup.exe"
Write-Host "`nInstaller built:" -ForegroundColor Green
Write-Host "  $setup"

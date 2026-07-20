<#
.SYNOPSIS
  Build a single OneAsr release installer (one binary with CUDA engines + CPU fallback).

.DESCRIPTION
  One product package (no portable zip, no separate CPU/CUDA setups):

    release\OneAsr_<ver>_setup.exe

  Install layout:
    OneAsr/
      oneasr.exe          # UI assets embedded; CUDA engines in binary
      bin/ffmpeg.exe
      dll/                # empty — Settings「安装组件」puts CUDA runtime here
      models/ output/ runs/

  No assets/ folder — SVG/WAV are compile-time embedded.

.PARAMETER Version
  Default: workspace Cargo.toml version.

.PARAMETER SkipBuild
  Re-use existing target\release\oneasr.exe.

.PARAMETER SkipInstaller
  Only stage dist\OneAsr (no Inno).
#>
param(
  [string]$Version = "",
  [switch]$SkipBuild,
  [switch]$SkipInstaller
)

$ErrorActionPreference = "Stop"
$Root = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $Root

function Get-WorkspaceVersion {
  $toml = Join-Path $Root "Cargo.toml"
  $m = Select-String -Path $toml -Pattern '^\s*version\s*=\s*"([^"]+)"' | Select-Object -First 1
  if ($m) { return $m.Matches[0].Groups[1].Value }
  return "0.1.0"
}

function Find-Iscc {
  $candidates = @(
    "$env:LocalAppData\Programs\Inno Setup 6\ISCC.exe",
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
  )
  foreach ($c in $candidates) {
    if (Test-Path -LiteralPath $c) { return $c }
  }
  $fromPath = Get-Command iscc -ErrorAction SilentlyContinue
  if ($fromPath) { return $fromPath.Source }
  return $null
}

if ([string]::IsNullOrWhiteSpace($Version)) {
  $Version = Get-WorkspaceVersion
}
$Version = $Version.Trim()
Write-Host "==> OneAsr pack-release  version=$Version  (single installer)"

$ffmpeg = Join-Path $Root "bin\ffmpeg.exe"
if (-not (Test-Path -LiteralPath $ffmpeg)) {
  throw "Missing bin\ffmpeg.exe"
}
# UI assets are embedded in the binary; only need source assets at compile time.
$ico = Join-Path $Root "assets\icons\app-icon.ico"
if (-not (Test-Path -LiteralPath $ico)) {
  Write-Host "==> generating app-icon.ico"
  python (Join-Path $Root "scripts\gen-app-icon.py")
  if ($LASTEXITCODE -ne 0) { throw "gen-app-icon.py failed" }
}

# ── Build (CUDA engines in binary; runtime DLLs optional) ────────────
$exeSrc = Join-Path $Root "target\release\oneasr.exe"
if (-not $SkipBuild) {
  Write-Host "==> cargo build -p oneasr --release  (default features include cuda)"
  cargo build -p oneasr --release
  if ($LASTEXITCODE -ne 0) { throw "cargo build failed (exit $LASTEXITCODE)" }
}
if (-not (Test-Path -LiteralPath $exeSrc)) {
  throw "Release binary not found: $exeSrc"
}

# ── Stage ────────────────────────────────────────────────────────────
$stage = Join-Path $Root "dist\OneAsr"
Write-Host "==> Staging $stage"
if (Test-Path -LiteralPath $stage) {
  Remove-Item -Recurse -Force -LiteralPath $stage
}
New-Item -ItemType Directory -Force -Path $stage | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $stage "bin") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $stage "dll") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $stage "models") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $stage "output") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $stage "runs") | Out-Null

Copy-Item -LiteralPath $exeSrc -Destination (Join-Path $stage "oneasr.exe") -Force
Copy-Item -LiteralPath $ffmpeg -Destination (Join-Path $stage "bin\ffmpeg.exe") -Force

"" | Set-Content -Path (Join-Path $stage "dll\.keep") -Encoding ascii
"" | Set-Content -Path (Join-Path $stage "models\.keep") -Encoding ascii
"" | Set-Content -Path (Join-Path $stage "output\.keep") -Encoding ascii
"" | Set-Content -Path (Join-Path $stage "runs\.keep") -Encoding ascii

$readme = @"
OneAsr $Version
================

Local A/V → SRT (Qwen3-ASR + ForcedAligner)

Layout
------
  oneasr.exe      main app (icons/sfx embedded)
  bin\ffmpeg.exe  media convert
  dll\            CUDA runtime components (Settings → 安装组件)
  models\         ASR / Aligner weights (Settings → 下载模型)
  output\         exported *.srt
  runs\           intermediate work files (safe to delete)

First run
---------
1. Start oneasr.exe
2. Settings → download ASR + Aligner models
3. Optional GPU: Settings → 安装组件 (CUDA), backend = Auto/GPU
4. Add media → Start all

"@
$utf8NoBom = New-Object System.Text.UTF8Encoding $false
[System.IO.File]::WriteAllText((Join-Path $stage "README.txt"), $readme, $utf8NoBom)

Write-Host "    staged:"
Get-ChildItem -Recurse $stage -File | ForEach-Object {
  $rel = $_.FullName.Substring($stage.Length + 1)
  Write-Host ("      {0,12:N0}  {1}" -f $_.Length, $rel)
}

# ── Installer ────────────────────────────────────────────────────────
$releaseDir = Join-Path $Root "release"
New-Item -ItemType Directory -Force -Path $releaseDir | Out-Null

if (-not $SkipInstaller) {
  $iscc = Find-Iscc
  if (-not $iscc) {
    Write-Warning "Inno Setup 6 (ISCC.exe) not found — only dist\OneAsr staged."
    Write-Warning "Install from https://jrsoftware.org/isinfo.php"
  } else {
    $iss = Join-Path $Root "installer\OneAsr.iss"
    Write-Host "==> ISCC $iscc"
    & $iscc "/DMyAppVersion=$Version" $iss
    if ($LASTEXITCODE -ne 0) { throw "ISCC failed (exit $LASTEXITCODE)" }
    $setup = Join-Path $releaseDir "OneAsr_${Version}_setup.exe"
    if (Test-Path -LiteralPath $setup) {
      Write-Host "    installer: $setup"
    } else {
      Write-Warning "Expected: $setup"
    }
  }
}

Write-Host ""
Write-Host "==> Done."
Write-Host "    Portable stage: dist\OneAsr\"
Write-Host "    Installer:      release\OneAsr_${Version}_setup.exe"

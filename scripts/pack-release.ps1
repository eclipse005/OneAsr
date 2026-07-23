<#
.SYNOPSIS
  Build a single OneAsr release installer (one binary with CUDA engines + CPU fallback).

.DESCRIPTION
  One product package (CUDA engines in binary + CPU fallback):

    release\OneAsr_<ver>_setup.exe
    release\OneAsr_<ver>_portable.zip

  Install / portable layout:
    OneAsr/
      oneasr.exe          # GUI subsystem (no black console); assets embedded
      bin/ffmpeg.exe
      dll/                # empty — Settings「安装组件」puts CUDA runtime here
      models/ output/ runs/

  No assets/ folder — SVG/WAV are compile-time embedded.

.PARAMETER Version
  Release version (e.g. 0.1.4). Positional arg works:

    .\scripts\pack-release.ps1 0.1.4
    .\scripts\pack-release.ps1 -Version 0.1.4

  Default (no arg): read [workspace.package] version from root Cargo.toml.
  When you pass a version, Cargo.toml is updated to match before build so
  artifact names and workspace version stay in sync.

.PARAMETER SkipBuild
  Re-use existing target\release\oneasr.exe.

.PARAMETER SkipInstaller
  Only stage dist\OneAsr (no Inno).

.EXAMPLE
  .\scripts\pack-release.ps1 0.1.4
.EXAMPLE
  .\scripts\pack-release.ps1 -Version 0.1.4 -SkipBuild
#>
param(
  [Parameter(Position = 0)]
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

function Set-WorkspaceVersion([string]$NewVersion) {
  $toml = Join-Path $Root "Cargo.toml"
  $utf8NoBom = New-Object System.Text.UTF8Encoding $false
  $content = [System.IO.File]::ReadAllText((Resolve-Path -LiteralPath $toml), $utf8NoBom)
  $pattern = '(?m)^(\s*version\s*=\s*")[^"]+(")'
  if (-not [regex]::IsMatch($content, $pattern)) {
    throw "version field not found in Cargo.toml"
  }
  $updated = [regex]::Replace($content, $pattern, '${1}' + $NewVersion + '${2}', 1)
  if ($updated -ne $content) {
    [System.IO.File]::WriteAllText((Resolve-Path -LiteralPath $toml), $updated, $utf8NoBom)
    Write-Host "    Cargo.toml version -> $NewVersion"
  }
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

# PE Optional Header Subsystem: 2 = IMAGE_SUBSYSTEM_WINDOWS_GUI, 3 = CONSOLE.
function Get-PeSubsystem([string]$ExePath) {
  $bytes = [System.IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $ExePath))
  if ($bytes.Length -lt 0x40) {
    throw "PE too small to parse: $ExePath"
  }
  if ($bytes[0] -ne 0x4D -or $bytes[1] -ne 0x5A) {
    throw "Not an MZ executable: $ExePath"
  }
  $pe = [BitConverter]::ToInt32($bytes, 0x3C)
  if ($pe -lt 0 -or ($pe + 0x5E) -ge $bytes.Length) {
    throw "Invalid PE e_lfanew=$pe for $ExePath"
  }
  $sig = [BitConverter]::ToUInt32($bytes, $pe)
  if ($sig -ne 0x00004550) {
    throw "Missing PE signature at offset $pe for $ExePath"
  }
  return [BitConverter]::ToUInt16($bytes, $pe + 0x5C)
}

function Assert-GuiSubsystem([string]$ExePath) {
  $sub = Get-PeSubsystem $ExePath
  if ($sub -ne 2) {
    throw "Refusing to pack console binary (subsystem=$sub, want 2/GUI): $ExePath. Build with cargo build -p oneasr --release."
  }
  Write-Host "    PE subsystem=GUI (2)  $ExePath"
}

$versionFromArg = -not [string]::IsNullOrWhiteSpace($Version)
if (-not $versionFromArg) {
  $Version = Get-WorkspaceVersion
}
$Version = $Version.Trim()
# Strip a leading "v" if the user typed v0.1.4
if ($Version -match '^[vV](.+)$') {
  $Version = $Matches[1]
}
if ($Version -notmatch '^\d+\.\d+\.\d+([\-+][0-9A-Za-z\.-]+)?$') {
  throw "Invalid version: '$Version'  (expected e.g. 0.1.4 or 1.0.0-beta.1)"
}
if ($versionFromArg) {
  Set-WorkspaceVersion $Version
}
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
Assert-GuiSubsystem $exeSrc

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
Assert-GuiSubsystem (Join-Path $stage "oneasr.exe")

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

# ── Portable zip + Installer ─────────────────────────────────────────
$releaseDir = Join-Path $Root "release"
New-Item -ItemType Directory -Force -Path $releaseDir | Out-Null

$portableZip = Join-Path $releaseDir "OneAsr_${Version}_portable.zip"
Write-Host "==> Portable zip $portableZip"
if (Test-Path -LiteralPath $portableZip) {
  Remove-Item -Force -LiteralPath $portableZip
}
# Zip folder contents so extract yields oneasr.exe at top level of the folder.
$zipStage = Join-Path $Root "dist\OneAsr_portable_stage"
if (Test-Path -LiteralPath $zipStage) {
  Remove-Item -Recurse -Force -LiteralPath $zipStage
}
New-Item -ItemType Directory -Force -Path $zipStage | Out-Null
Copy-Item -Recurse -Force -Path (Join-Path $stage "*") -Destination $zipStage
Compress-Archive -Path (Join-Path $zipStage "*") -DestinationPath $portableZip -Force
Remove-Item -Recurse -Force -LiteralPath $zipStage
Write-Host "    portable: $portableZip  ($([math]::Round((Get-Item $portableZip).Length/1MB, 1)) MB)"

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
Write-Host "    Portable zip:   release\OneAsr_${Version}_portable.zip"
Write-Host "    Installer:      release\OneAsr_${Version}_setup.exe"

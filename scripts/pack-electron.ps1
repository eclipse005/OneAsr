# Build the Electron release artifacts (Windows setup + portable zip).
#
#   .\scripts\pack-electron.ps1
#   .\scripts\pack-electron.ps1 -SkipBuild -SkipInstaller
#
# Requires: cargo, Node 24, apps\electron\node_modules (npm ci), Inno Setup 6.
# Version comes only from Cargo.toml; apps\electron\package.json is asserted to match.

param(
  [switch]$SkipBuild,
  [switch]$SkipInstaller
)

$ErrorActionPreference = "Stop"
$Root = Resolve-Path (Join-Path $PSScriptRoot "..")
$AppDir = Join-Path $Root "apps\electron"
$Runtime = Join-Path $AppDir "runtime"
$ReleaseDir = Join-Path $Root "release"

function Get-WorkspaceVersion {
  $toml = Join-Path $Root "Cargo.toml"
  $m = Select-String -Path $toml -Pattern '^\s*version\s*=\s*"([^"]+)"' | Select-Object -First 1
  if (-not $m) { throw "[workspace.package] version not found in $toml" }
  return $m.Matches[0].Groups[1].Value
}

function Get-PackageVersion {
  $pkg = Get-Content -Raw -LiteralPath (Join-Path $AppDir "package.json") | ConvertFrom-Json
  return [string]$pkg.version
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

$Version = (Get-WorkspaceVersion).Trim()
$PackageVersion = (Get-PackageVersion).Trim()
if ($Version -notmatch '^\d+\.\d+\.\d+([\-+][0-9A-Za-z\.-]+)?$') {
  throw "Invalid Cargo.toml version: '$Version'"
}
if ($PackageVersion -ne $Version) {
  throw "apps\electron\package.json version is '$PackageVersion' but Cargo.toml is '$Version'. Bump both (verify-version.py)."
}
if (-not (Test-Path -LiteralPath (Join-Path $AppDir "node_modules\electron-builder"))) {
  throw "apps\electron dependencies are missing. Run: cd apps\electron && npm ci"
}

# Electron 44 起 npm 包不再带 postinstall（只剩 install-electron 入口），
# 而 electron-builder 一旦指定 electronDist 就不会自己下载 —— 打包前必须显式装一次。
# install.js 幂等：已装好且版本一致时直接退出。
Write-Host "==> ensure Electron binary"
node (Join-Path $AppDir "node_modules\electron\install.js")
if ($LASTEXITCODE -ne 0) { throw "Electron binary install failed (exit $LASTEXITCODE)" }

Write-Host "==> OneAsr Electron pack  version=$Version"

if (-not $SkipBuild) {
  Write-Host "==> cargo build --locked -p oneasr-core --release --bin oneasr-cli"
  cargo build --locked -p oneasr-core --release --bin oneasr-cli
  if ($LASTEXITCODE -ne 0) { throw "cargo build failed (exit $LASTEXITCODE)" }

  Write-Host "==> npm run build (electron-vite)"
  Push-Location $AppDir
  try {
    npm run build
    if ($LASTEXITCODE -ne 0) { throw "npm run build failed (exit $LASTEXITCODE)" }
  } finally {
    Pop-Location
  }
}

$Cli = Join-Path $Root "target\release\oneasr-cli.exe"
if (-not (Test-Path -LiteralPath $Cli)) { throw "CLI not found: $Cli" }

Write-Host "==> staging runtime (CLI + verified ffmpeg)"
if (Test-Path -LiteralPath $Runtime) { Remove-Item -Recurse -Force -LiteralPath $Runtime }
New-Item -ItemType Directory -Force -Path (Join-Path $Runtime "bin") | Out-Null
Copy-Item -LiteralPath $Cli -Destination (Join-Path $Runtime "oneasr-cli.exe") -Force
node (Join-Path $AppDir "scripts\fetch-ffmpeg.mjs") --os windows --arch x64 --dest (Join-Path $Runtime "bin\ffmpeg.exe")
if ($LASTEXITCODE -ne 0) { throw "fetch-ffmpeg failed (exit $LASTEXITCODE)" }

Write-Host "==> electron-builder (win dir)"
$BuildOut = Join-Path $AppDir ("dist/build-" + (Get-Date -Format "yyyyMMddHHmmss"))
Push-Location $AppDir
try {
  npx --no-install electron-builder --win dir --x64 --config.directories.output=$BuildOut
  if ($LASTEXITCODE -ne 0) { throw "electron-builder failed (exit $LASTEXITCODE)" }
} finally {
  Pop-Location
}

$Unpacked = Join-Path $BuildOut "win-unpacked"
if (-not (Test-Path -LiteralPath $Unpacked)) { throw "electron-builder output missing: $Unpacked" }

$Stage = Join-Path $Root ("dist/stage-" + (Get-Date -Format "yyyyMMddHHmmss"))
if (Test-Path -LiteralPath $Stage) { Remove-Item -Recurse -Force -LiteralPath $Stage }
New-Item -ItemType Directory -Force -Path $Stage | Out-Null
Copy-Item -Recurse -Force -Path (Join-Path $Unpacked "*") -Destination $Stage
New-Item -ItemType Directory -Force -Path (Join-Path $Stage "models") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $Stage "output") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $Stage "runs") | Out-Null

$Exe = Join-Path $Stage "OneAsr.exe"
if (-not (Test-Path -LiteralPath $Exe)) {
  $found = @(Get-ChildItem -LiteralPath $Stage -File -Filter "*.exe" | Where-Object { $_.Name -ieq "OneAsr.exe" })
  if ($found.Count -ne 1) { throw "expected OneAsr.exe in $Stage" }
  $Exe = $found[0].FullName
}
if (-not (Test-Path -LiteralPath (Join-Path $Stage "resources\runtime\oneasr-cli.exe"))) {
  throw "runtime/oneasr-cli.exe missing from packaged resources"
}
if (-not (Test-Path -LiteralPath (Join-Path $Stage "resources\runtime\bin\ffmpeg.exe"))) {
  throw "runtime/bin/ffmpeg.exe missing from packaged resources"
}

$readme = @"
OneAsr $Version (Windows x64)
=============================

Local A/V to subtitles (Qwen3-ASR + CTC aligner).

Layout
------
  OneAsr.exe        main app
  resources\runtime\oneasr-cli.exe   headless CLI
  resources\runtime\bin\ffmpeg.exe   media convert
  models\           model weights (download in Settings)
  output\           exported subtitles
  runs\             intermediate files (safe to delete)

First run
---------
1. Start OneAsr.exe
2. Settings -> Models -> download ASR + aligner models
3. Add media -> Start all

Models, output and settings live next to the executable when that folder is
writable (this portable folder / the per-user installer). If the app is put in
a read-only location, they go to your user data directory instead.
"@
$utf8NoBom = New-Object System.Text.UTF8Encoding $false
[System.IO.File]::WriteAllText((Join-Path $Stage "README.txt"), $readme, $utf8NoBom)

New-Item -ItemType Directory -Force -Path $ReleaseDir | Out-Null
$portableZip = Join-Path $ReleaseDir "OneAsr_${Version}_windows_portable.zip"
if (Test-Path -LiteralPath $portableZip) { Remove-Item -Force -LiteralPath $portableZip }
# Compress-Archive can keep an open handle to app.asar; .NET's ZipFile writes
# straight from the stage and never leaves a temp folder behind.
Add-Type -AssemblyName System.IO.Compression.FileSystem
[System.IO.Compression.ZipFile]::CreateFromDirectory(
  $Stage,
  $portableZip,
  [System.IO.Compression.CompressionLevel]::Optimal,
  $false
)
Write-Host "    portable: $portableZip"

if (-not $SkipInstaller) {
  $iscc = Find-Iscc
  if (-not $iscc) {
    throw "Inno Setup 6 (ISCC.exe) not found. Install it or pass -SkipInstaller."
  }
  $setup = Join-Path $ReleaseDir "OneAsr_${Version}_windows_setup.exe"
  if (Test-Path -LiteralPath $setup) { Remove-Item -Force -LiteralPath $setup }
  & $iscc "/DMyAppVersion=$Version" "/DMyStageDir=$Stage" (Join-Path $Root "installer\OneAsr.iss")
  if ($LASTEXITCODE -ne 0) { throw "ISCC failed (exit $LASTEXITCODE)" }
  if (-not (Test-Path -LiteralPath $setup)) { throw "installer missing: $setup" }
  Write-Host "    installer: $setup"
}

if (-not (Test-Path -LiteralPath $portableZip)) { throw "portable zip missing" }
Write-Host "==> Done. Artifacts in $ReleaseDir"

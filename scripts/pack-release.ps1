<#
.SYNOPSIS
  Build a single OneAsr release (wgpu GPU + CPU in one binary).

.DESCRIPTION
  One product package:

    release\OneAsr_<ver>_windows_setup.exe
    release\OneAsr_<ver>_windows_portable.zip

  Install / portable layout:
    OneAsr/
      oneasr.exe          # GUI subsystem (no black console); assets embedded
      oneasr-cli.exe      # console CLI (same pipeline; Python / scripts)
      bin/ffmpeg.exe
      models/ output/ runs/

  No assets/ folder — SVG/WAV are compile-time embedded.

版本号只从 Cargo.toml 的 [workspace.package] version 读，本脚本不写它。
（以前可以传 -Version，脚本会把版本号写回 Cargo.toml —— 那是两处真相：
产物名、README 下载链接、git tag 可以互相对不上而没人察觉。现在改版本 =
改 Cargo.toml 一处，CI 再断言 tag / 两份 README 与它一致。）

ffmpeg 来自上游 Tyrrrz/FFmpegBin 的版本化 tag（tag 名 = ffmpeg 版本），URL 与哈希都只
声明在 scripts/ffmpeg-checksums.txt 里。这里下的是**归档**，放进产物前会强制校验归档
的 sha256，再只解出需要的那枚 ffmpeg（不匹配直接失败，绝不放行）。

.PARAMETER SkipBuild
  Re-use existing target\release\oneasr.exe (and oneasr-cli.exe).

.PARAMETER SkipInstaller
  只暂存 dist\OneAsr，不产出 setup.exe。这是显式放弃安装器；不加这个开关时，
  缺 ISCC.exe 或缺 setup.exe 都直接失败，不再 Write-Warning 之后「成功」退出。

.EXAMPLE
  .\scripts\pack-release.ps1
.EXAMPLE
  .\scripts\pack-release.ps1 -SkipBuild
#>
param(
  [switch]$SkipBuild,
  [switch]$SkipInstaller
)

$ErrorActionPreference = "Stop"
$Root = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $Root

# 版本号的唯一来源：根 Cargo.toml 的 [workspace.package] version。
# 读不到直接失败，不再回退到 "0.1.0" —— 那样产物名会和仓库版本悄悄脱节。
function Get-WorkspaceVersion {
  $toml = Join-Path $Root "Cargo.toml"
  $m = Select-String -Path $toml -Pattern '^\s*version\s*=\s*"([^"]+)"' | Select-Object -First 1
  if (-not $m) { throw "[workspace.package] version not found in $toml" }
  return $m.Matches[0].Groups[1].Value
}

# ffmpeg 的来源 URL 只声明在清单文件里（`# base-url <URL>` 一行），本脚本不再抄一份。
function Get-FfmpegBase {
  $manifest = Join-Path $Root "scripts\ffmpeg-checksums.txt"
  if (-not (Test-Path -LiteralPath $manifest)) {
    throw "Checksum manifest missing: $manifest"
  }
  $entry = Select-String -Path $manifest -Pattern '^\s*#\s*base-url\s+(\S+)\s*$' | Select-Object -First 1
  if (-not $entry) { throw "No '# base-url <URL>' line in $manifest" }
  return $entry.Matches[0].Groups[1].Value
}

# 校验某个文件与 scripts/ffmpeg-checksums.txt 里该归档钉的 sha256 是否一致。
# 清单缺失、清单里没有这个归档、内容不匹配 —— 全部 throw；绝不放行。
# 为什么：GitHub 的 release 资产默认仍可被重新上传，URL 不保证内容，
# 唯一可信的锚点是哈希。
function Assert-Checksum([string]$Path, [string]$Asset) {
  $manifest = Join-Path $Root "scripts\ffmpeg-checksums.txt"
  if (-not (Test-Path -LiteralPath $manifest)) {
    throw "Checksum manifest missing: $manifest"
  }
  if (-not (Test-Path -LiteralPath $Path)) {
    throw "Cannot checksum missing file: $Path"
  }
  $pattern = "^\s*([0-9a-fA-F]{64})\s+" + [regex]::Escape($Asset) + "\s*$"
  $entry = Select-String -Path $manifest -Pattern $pattern | Select-Object -First 1
  if (-not $entry) {
    throw "No sha256 for '$Asset' in manifest"
  }
  $want = $entry.Matches[0].Groups[1].Value.ToLower()
  $got = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLower()
  if ($got -ne $want) {
    throw ("Checksum mismatch for {0}`n  file:     {1}`n  expected: {2}`n  actual:   {3}`n" +
      "The ffmpeg archive does not match scripts/ffmpeg-checksums.txt. Either it is not`n" +
      "the pinned file, or the asset behind`n" +
      "  {4}/{0}`n" +
      "was re-uploaded. Do not ship it. Delete dist\ffmpeg-cache\{0} and re-run; if`n" +
      "upstream legitimately changed, refresh the manifest:`n" +
      "  python scripts/verify-ffmpeg-manifest.py --print-manifest") -f $Asset, $Path, $want, $got, (Get-FfmpegBase)
  }
  Write-Host "    sha256 ok  $Asset  $want"
}

# 取一份**已校验**的 ffmpeg 归档（优先用缓存）。
# 缓存命中与否不影响正确性：内容哈希才是可信凭证，所以命中也照样校验一次，
# 不对就删掉重下。缓存省的是重复打包时那 60~80 MB 下载。
function Get-FfmpegArchive([string]$Asset) {
  $cacheDir = Join-Path $Root "dist\ffmpeg-cache"
  $archive = Join-Path $cacheDir $Asset
  New-Item -ItemType Directory -Force -Path $cacheDir | Out-Null
  if (Test-Path -LiteralPath $archive) {
    try {
      Assert-Checksum $archive $Asset
      Write-Host "    cached   $Asset"
      return $archive
    } catch {
      Write-Host "    cached copy rejected, re-downloading"
      Remove-Item -Force -LiteralPath $archive
    }
  }
  Write-Host "==> downloading $Asset"
  $url = "$(Get-FfmpegBase)/$Asset"
  & curl.exe -L --fail --retry 3 -o $archive $url
  if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $archive)) {
    throw "Failed to download $url"
  }
  # 快速失败：先校验再解压/编译，不让后续几分钟的构建白做。
  Assert-Checksum $archive $Asset
  return $archive
}

# 从归档里解出 ffmpeg.exe 到 $Dest。
# 按文件名精确匹配而不是假设它在归档根目录：上游改归档布局时不会静默解错文件。
# 一个归档里有 ffmpeg / ffplay / ffprobe，产物只需要 ffmpeg —— 顺手只取它。
function Expand-Ffmpeg([string]$Archive, [string]$Dest) {
  $extractDir = Join-Path $env:TEMP ("oneasr-ffmpeg-" + [System.IO.Path]::GetRandomFileName())
  try {
    Expand-Archive -LiteralPath $Archive -DestinationPath $extractDir -Force
    $found = @(Get-ChildItem -LiteralPath $extractDir -Recurse -File -Filter "ffmpeg.exe")
    if ($found.Count -ne 1) {
      throw "Expected exactly one ffmpeg.exe inside $Archive, found $($found.Count)"
    }
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Dest) | Out-Null
    Copy-Item -LiteralPath $found[0].FullName -Destination $Dest -Force
    Write-Host "    extracted ffmpeg.exe -> $Dest"
  } finally {
    Remove-Item -Recurse -Force -LiteralPath $extractDir -ErrorAction SilentlyContinue
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

function Assert-ConsoleSubsystem([string]$ExePath) {
  $sub = Get-PeSubsystem $ExePath
  if ($sub -ne 3) {
    throw "Refusing to pack GUI binary as CLI (subsystem=$sub, want 3/CONSOLE): $ExePath. Build with cargo build -p oneasr-core --release --bin oneasr-cli."
  }
  Write-Host "    PE subsystem=CONSOLE (3)  $ExePath"
}

$Version = (Get-WorkspaceVersion).Trim()
if ($Version -notmatch '^\d+\.\d+\.\d+([\-+][0-9A-Za-z\.-]+)?$') {
  throw "Invalid [workspace.package] version in Cargo.toml: '$Version'"
}
Write-Host "==> OneAsr pack-release  version=$Version  (from Cargo.toml; single installer)"

# ffmpeg 是随产物分发的第三方二进制，来自上游 Tyrrrz/FFmpegBin 的版本化 tag ——
# 归档下多少次都不保证内容，所以信任锚点是 scripts/ffmpeg-checksums.txt 里的 sha256。
#
# 产物里那枚 ffmpeg **一律从已校验的归档现场解出**，不复用 bin\ffmpeg.exe：这样
# 「产物里那枚就是钉住的那一枚」不依赖任何本地状态（旧的 bin\ffmpeg.exe 可能是上一版
# 归档解出来的，也可能被人动过）。bin\ffmpeg.exe 随后被刷新成同一枚，供从源码直接
# 跑应用时用（app_root/bin 是第一查找位置）。
$ffmpegAsset = "ffmpeg-windows-x64.zip"
$archive = Get-FfmpegArchive $ffmpegAsset
$verifiedFfmpeg = Join-Path $Root "dist\ffmpeg-cache\ffmpeg.exe"
Expand-Ffmpeg $archive $verifiedFfmpeg
$ffmpeg = Join-Path $Root "bin\ffmpeg.exe"
New-Item -ItemType Directory -Force -Path (Join-Path $Root "bin") | Out-Null
Copy-Item -LiteralPath $verifiedFfmpeg -Destination $ffmpeg -Force
# UI assets are embedded in the binary; only need source assets at compile time.
$ico = Join-Path $Root "assets\icons\app-icon.ico"
if (-not (Test-Path -LiteralPath $ico)) {
  Write-Host "==> generating app-icon.ico"
  python (Join-Path $Root "scripts\gen-app-icon.py")
  if ($LASTEXITCODE -ne 0) { throw "gen-app-icon.py failed" }
}

# ── Build (wgpu GPU + CPU in one binary) ─────────────────────────────
$exeSrc = Join-Path $Root "target\release\oneasr.exe"
$cliSrc = Join-Path $Root "target\release\oneasr-cli.exe"
if (-not $SkipBuild) {
  # --locked：Cargo.lock 已入库，发布构建必须严格按锁定的版本编译。
  # 不加 --locked 时，锁文件与 Cargo.toml 一旦不同步，cargo 会静默重新
  # 解析依赖 —— 同一个 tag 的两台机器可能装上两套不同的依赖。
  Write-Host "==> cargo build --locked -p oneasr --release"
  cargo build --locked -p oneasr --release
  if ($LASTEXITCODE -ne 0) { throw "cargo build failed (exit $LASTEXITCODE)" }
  Write-Host "==> cargo build --locked -p oneasr-core --release --bin oneasr-cli"
  cargo build --locked -p oneasr-core --release --bin oneasr-cli
  if ($LASTEXITCODE -ne 0) { throw "cargo build oneasr-cli failed (exit $LASTEXITCODE)" }
}
if (-not (Test-Path -LiteralPath $exeSrc)) {
  throw "Release binary not found: $exeSrc"
}
if (-not (Test-Path -LiteralPath $cliSrc)) {
  throw "CLI binary not found: $cliSrc"
}
Assert-GuiSubsystem $exeSrc
Assert-ConsoleSubsystem $cliSrc

# ── Stage ────────────────────────────────────────────────────────────
$stage = Join-Path $Root "dist\OneAsr"
Write-Host "==> Staging $stage"
if (Test-Path -LiteralPath $stage) {
  Remove-Item -Recurse -Force -LiteralPath $stage
}
New-Item -ItemType Directory -Force -Path $stage | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $stage "bin") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $stage "models") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $stage "output") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $stage "runs") | Out-Null

Copy-Item -LiteralPath $exeSrc -Destination (Join-Path $stage "oneasr.exe") -Force
Copy-Item -LiteralPath $cliSrc -Destination (Join-Path $stage "oneasr-cli.exe") -Force
Copy-Item -LiteralPath $ffmpeg -Destination (Join-Path $stage "bin\ffmpeg.exe") -Force
Assert-GuiSubsystem (Join-Path $stage "oneasr.exe")
Assert-ConsoleSubsystem (Join-Path $stage "oneasr-cli.exe")

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
  oneasr-cli.exe  headless CLI (same pipeline; for scripts / Python)
  bin\ffmpeg.exe  media convert
  models\         ASR / Aligner weights (Settings -> download models)
  output\         exported *.srt
  runs\           intermediate work files (safe to delete)

First run
---------
1. Start oneasr.exe
2. Settings -> download ASR + Aligner models
3. Backend Auto/GPU uses the graphics driver (no CUDA Toolkit)
4. Add media -> Start all

Keep the install folder somewhere you can write and models, output and settings
land next to the app. Installed somewhere read-only? They then go to your user
data directory instead; the exact path is shown in Settings and in the startup log.

CLI / Python
------------
  oneasr-cli.exe transcribe --input video.mp4 --language zh --backend auto --output out.srt

  Default --app-root is this folder (bin/ffmpeg + models/). Download models via the GUI first.

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

$portableZip = Join-Path $releaseDir "OneAsr_${Version}_windows_portable.zip"
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
    throw ("Inno Setup 6 (ISCC.exe) not found. Install it from https://jrsoftware.org/isinfo.php, " +
      "or pass -SkipInstaller if you only want dist\OneAsr staged. A release without " +
      "OneAsr_${Version}_windows_setup.exe is not a release.")
  }
  $iss = Join-Path $Root "installer\OneAsr.iss"
  Write-Host "==> ISCC $iscc"
  & $iscc "/DMyAppVersion=$Version" $iss
  if ($LASTEXITCODE -ne 0) { throw "ISCC failed (exit $LASTEXITCODE)" }
  # ISCC 退出码为 0 不等于产物存在（源目录缺失等情况以前会走到这里）。
  $setup = Join-Path $releaseDir "OneAsr_${Version}_windows_setup.exe"
  if (-not (Test-Path -LiteralPath $setup)) {
    throw "ISCC exited 0 but no installer was produced: $setup"
  }
  Write-Host "    installer: $setup"
}

Write-Host ""
Write-Host "==> Done."
Write-Host "    Portable stage: dist\OneAsr\"
# 只报告真正产出的东西：以前缺安装器也照样打印它的路径，本地很容易误判成成功。
if (-not (Test-Path -LiteralPath $portableZip)) {
  throw "Portable zip missing after packaging: $portableZip"
}
Write-Host "    Portable zip:   $portableZip"
$setupPath = Join-Path $releaseDir "OneAsr_${Version}_windows_setup.exe"
if (Test-Path -LiteralPath $setupPath) {
  Write-Host "    Installer:      $setupPath"
} else {
  Write-Host "    Installer:      skipped (-SkipInstaller)"
}

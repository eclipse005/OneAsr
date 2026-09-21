# OneAsr

**本地 · 离线 · 批量音视频 → SRT / TXT 字幕**

支持 Windows / Linux / macOS。不上传云端，不依赖在线 API。基于阿里通义 [Qwen3-ASR](https://github.com/QwenLM/Qwen3-ASR) 与 ForcedAligner，从音视频直接产出带时间轴的字幕文件。GPU 加速：NVIDIA、AMD、Intel、Mac M 芯片。

```text
音视频  →  [人声分离]  →  VAD 分段  →  识别  →  对齐打轴  →  智能断句  →  *.srt / *.txt
```

任意时刻显存中只驻留 **一个** 大模型（先 ASR，再 Aligner），4GB 级显卡也能跑通 0.6B 组合。

---

## 为什么选 OneAsr

| | |
|---|---|
| **完全本地** | 识别与对齐均在本机完成，素材不出机 |
| **批量友好** | 列表队列一次丢多个文件，进度与阶段一目了然 |
| **时间轴可靠** | ForcedAligner 词级对齐 + 字幕长度预设，不是“整段瞎估时间” |
| **GPU 加速** | NVIDIA、AMD、Intel、Mac M 芯片；无独显走 CPU |
| **轻量界面** | 原生 GPUI 亮色列表，无 Electron 臃肿壳 |

---

## 功能

- **11 种源语言**：中文普通话、English、粤语、日本語、한국어、Français、Deutsch、Italiano、Español、Português、Русский  
  （与对齐模型能力对齐；**需手动指定语种**，不做自动语种检测）
- **双规格 ASR**：Qwen3-ASR **0.6B**（更快 / 省显存）· **1.7B**（更准 / 更吃资源）
- **词级时间轴**：Qwen3-ForcedAligner 对齐；日语分词已内嵌，无需额外模型目录
- **VAD 智能分段**：目标段长 **30–180 秒**（默认 60），长音频更稳
- **输出格式可选**：SRT 字幕 / TXT 纯文本（逐句一行）可同时选，至少保留一种
- **中文字形可选**：原文（默认，保持模型输出）/ 简体 / 繁体，仅对中文、粤语素材生效；转换在导出前完成，不影响时间轴
- **人声分离（可选）**：内置 HTDemucs v4（人声权重），转录前压掉背景音乐 / 噪声；与识别/对齐同一套 GPU / CPU
- **字幕长度预设**：短 / 标准 / 松，控制单行信息量
- **处理明细**：各阶段耗时可看，方便对比机器与参数
- **字幕输出位置**：默认与视频同目录，设置里可切换到指定文件夹（如 `{安装目录}/output/`）
- **提示音**：一个开关管全部（交互反馈 + 整批处理完成的提醒，设置 → 提示音），默认开启；提醒音约 3.7 秒，成功与失败同一个音，结果看提示条颜色与文字
- **本地统计**：状态栏常驻「已省 X」，点开是累计省下的时间、年内每天的处理时长与明细；账本只记数字（时长、语种、句数），不含文件名与路径，也不上传
- **无卡可用**：无可用 GPU 时走 CPU（更慢，但功能完整）

---

## 下载与安装

当前版本 **[v1.0.1](https://github.com/eclipse005/OneAsr/releases/tag/v1.0.1)**（点文件名即下载）：

| 平台 | 下载 |
|------|------|
| Windows | [安装包](https://github.com/eclipse005/OneAsr/releases/download/v1.0.1/OneAsr_1.0.1_windows_setup.exe) · [便携包](https://github.com/eclipse005/OneAsr/releases/download/v1.0.1/OneAsr_1.0.1_windows_portable.zip) |
| Linux x64 | [安装包](https://github.com/eclipse005/OneAsr/releases/download/v1.0.1/OneAsr_1.0.1_linux_x64.deb) · [便携包](https://github.com/eclipse005/OneAsr/releases/download/v1.0.1/OneAsr_1.0.1_linux_x64.tar.gz) |
| macOS（M 芯片） | [磁盘映像](https://github.com/eclipse005/OneAsr/releases/download/v1.0.1/OneAsr_1.0.1_macos.dmg) |

[历史版本](https://github.com/eclipse005/OneAsr/releases)

**首次使用：**

1. 运行主程序（Windows `oneasr.exe`，Linux / macOS `oneasr`）
2. **设置** → 下载 **ASR**（建议先 0.6B）+ **ForcedAligner**（必下）
3. 选好源语言 → 添加音视频 → 开始
4. 完成后在输出目录查看同名 `.srt`

macOS 包未签名：磁盘映像里附有「安装 OneAsr.command」和「安装说明.txt」。先右键该脚本 → 打开；macOS 15 起这条路被系统封了（弹框只剩「完成」），改用终端：`bash ` 后面把脚本文件拖进终端窗口再回车，或者手动 `sudo xattr -cr /Applications/OneAsr.app`。脚本做的事就是把 App 装进「应用程序」并清掉下载隔离标记，否则会误报「已损坏，无法打开」。Linux 建议把 tar.gz 解压到用户目录（模型写入应用旁的 `models/`）。

---

## 模型与磁盘

安装包 **不含** 权重，需在设置中从 ModelScope 下载（可断点续传）。

| 组件 | 名称 | 约体积 | 说明 |
|------|------|--------|------|
| ASR（二选一） | Qwen3-ASR-0.6B-hf | ~1.6 GB | 默认推荐 |
| | Qwen3-ASR-1.7B-hf | ~4.1 GB | 更高精度 |
| 对齐（必需） | Qwen3-ForcedAligner-0.6B-hf | ~1.8 GB | 所有 ASR 共用 |
| 人声分离（可选） | HTDemucs v4 vocals (htdemucs_ft) | ~84 MB | 带 BGM / 噪声素材启用 |

常用组合约 **3.5～6.0 GB**，另建议预留任务临时空间。

路径约定：

```text
{app}/
  oneasr / oneasr.exe
  oneasr-cli / oneasr-cli.exe   # 无界面命令行，与 GUI 同一套流水线
  bin/ffmpeg[.exe]              # 安装包已带；源码开发需自行放置
  models/                       # ASR / Aligner 权重（官方 Qwen `-hf`）
  output/                       # 可选的统一输出目录（默认字幕存视频同目录）
  runs/                         # 中间文件（任务结束自动清理）
  oneasr-error.log              # 崩溃/错误日志（闪退时可附上）
```

---

## 配置要求

| 项目 | 说明 |
|------|------|
| 系统 | Windows 10 / 11、Linux、macOS（Apple Silicon） |
| 加速 | NVIDIA、AMD、Intel、Mac M 芯片；推荐 **4GB+** 显存（0.6B），1.7B 建议 **6GB+** |
| 无独显 | 走 CPU，速度明显更慢 |
| 网络 | 仅首次下模型需要；识别过程可离线 |

---

## 使用提示

- **语种选错**是时间轴/识别异常的最常见原因；粤语用「粤语」，其它中文方言一般选「中文普通话」
- 同一时刻只加载一个大模型，内存友好，但长队列会按文件串行处理
- 速度慢：检查后端是否为 Auto/GPU、显卡驱动是否过旧
- 失败任务可在列表中重试；`runs/` 任务结束会自动清理（排查问题可设 `ONEASR_KEEP_SCRATCH=1` 保留）

---

## 命令行 / Python

安装包都带 `oneasr-cli`（Windows 为 `oneasr-cli.exe`），与 GUI 同一条流水线。先用 GUI 把模型下载到安装目录，再调用 CLI。默认 `--app-root` 就是程序所在目录。

```powershell
oneasr-cli.exe transcribe --input "C:\path\to\video.mp4" --language zh --backend auto --output "C:\path\to\out.srt"
```

常用开关：`--txt`（额外输出同名 `.txt`）、`--no-srt`（只出文本，需配合 `--txt`）、
`--script original|simplified|traditional`（中文字形，默认 `original`）、`--vocal-separation`
（先做人声分离，需已下载 HTDemucs 权重）。

Python：

```python
import subprocess

app = r"C:\Path\To\OneAsr"  # 安装目录或便携包解压目录
subprocess.run(
    [
        fr"{app}\oneasr-cli.exe",
        "transcribe",
        "--input", r"C:\path\to\video.mp4",
        "--app-root", app,
        "--language", "zh",
        "--backend", "auto",
        "--output", r"C:\path\to\out.srt",
    ],
    check=True,
)
```

退出码 0 为成功。阶段日志在 stderr；字幕写到 `--output`（缺省则 `{app}/output/{stem}.srt`）。

```powershell
oneasr-cli.exe --help
```

---

## 流水线（技术）

```text
ffmpeg → 16 kHz mono
  → VAD 分段（30–180s）
  → 加载 ASR → 逐段转写 → 释放 ASR
  → 加载 Aligner → 逐段对齐 → 释放 Aligner
  → 标点还原 + 词元规范化
  → 断句（标点硬切 + 长度 DP）
  → {output}/{stem}.srt
```

引擎仓库（Rust，与产品共用）：

| 组件 | 仓库 |
|------|------|
| ASR | [qwen3-asr-wgpu](https://github.com/eclipse005/qwen3-asr-wgpu) |
| Aligner | [qwen3-aligner-wgpu](https://github.com/eclipse005/qwen3-aligner-wgpu) |
| 人声分离 | [demucs-wgpu](https://github.com/eclipse005/demucs-wgpu) |

---

## 从源码构建

**环境：** Rust（edition 2024）· CMake / C 编译器（引擎会编一份 soxr）。Windows 用 [VS 2022 C++ 生成工具](https://visualstudio.microsoft.com/downloads/)；Linux / macOS 用 clang 或 gcc。

### 1. 准备 ffmpeg（必需）

源码仓库**不含** ffmpeg 二进制。五个平台的构建都在 **[构建工具 Release](https://github.com/eclipse005/OneAsr/releases/tag/tools)**（来源 [Tyrrrz/FFmpegBin](https://github.com/Tyrrrz/FFmpegBin) 7.1.2，各平台版本一致，GPLv3 构建）：

| 平台 | 放置路径 | Release 产物 |
| --- | --- | --- |
| Windows x64 | `OneAsr/bin/ffmpeg.exe` | `ffmpeg.exe` |
| Linux x64 | `OneAsr/bin/ffmpeg` | `ffmpeg-linux-x64` |
| Linux arm64 | `OneAsr/bin/ffmpeg` | `ffmpeg-linux-arm64` |
| macOS Apple Silicon | `OneAsr/bin/ffmpeg` | `ffmpeg-macos-arm64` |

Windows：

```powershell
New-Item -ItemType Directory -Force -Path bin | Out-Null
Invoke-WebRequest -Uri "https://github.com/eclipse005/OneAsr/releases/download/tools/ffmpeg.exe" -OutFile "bin\ffmpeg.exe"
```

Linux / macOS（把 `ffmpeg-linux-x64` 换成上表对应产物）：

```bash
mkdir -p bin
curl -L -o bin/ffmpeg https://github.com/eclipse005/OneAsr/releases/download/tools/ffmpeg-linux-x64
chmod +x bin/ffmpeg
```

`bin/` 中没有可用二进制时，运行时会回退到 `PATH` 上的系统 ffmpeg（源码开发、发行版打包场景）。
非 Windows 平台若可执行位丢失（zip 解压等），程序会在启动时自动补上；目录只读则会明确报错而不是静默失败。

<details>
<summary>SHA256（可选校验）</summary>

```text
ffmpeg.exe           6E1C77F66726DCAFB8008D9081972D5B28590110EB7BF906EAEEF9298D366359
ffmpeg-linux-x64     ECE002A9EEC0AC763A0A5FBC24FCCFFF6466425A8BED18D5C2836F2B79470352
ffmpeg-linux-arm64   22F924A690E283B8907C70E6D892586A448AB1BC62013DC8D1BA7FE77E058313
ffmpeg-macos-arm64   7A6F6EDCCFB4B6E5AB2D4EA4F6882092B43933537A6819476D5361CBF6EED4DB
```
</details>

> 安装包 / 便携包已自带 ffmpeg，**普通用户无需**这一步。

### 2. 运行

```powershell
cargo run -p oneasr --release

# 无界面 CLI（脚本 / 回归）
cargo run -p oneasr-core --release --bin oneasr-cli -- `
  transcribe --input "C:\path\to\video.mp4" --app-root "." `
  --language zh --chunk-seconds 60 --backend auto
```

```powershell
cargo test -p oneasr-core
cargo build -p oneasr --release
cargo build -p oneasr-core --release --bin oneasr-cli
```

同一二进制含 GPU 与 CPU；无可用 GPU 时自动走 CPU。

### 3. 发 GitHub Release

改好 `Cargo.toml` 版本并打 tag，Action 会编出各平台安装包并挂到 Release：

```powershell
# 1. Cargo.toml 的 version 改成 1.0.1（或让我改）
# 2. 提交、推送
git tag v1.0.1
git push origin v1.0.1
```

产物：Windows 安装包 / 便携包、Linux `.deb` / `.tar.gz`、macOS `.dmg`（Apple Silicon）。macOS 包未签名，映像内附「安装 OneAsr.command」+「安装说明.txt」：右键打开脚本，或在终端里 `bash` 它，装好即解除 Gatekeeper 拦截。每个文件单独下载，不用下一个打包在一起的压缩包。

---

## 许可

- 本项目代码：**MIT**
- 识别 / 对齐模型权重与协议以 [Qwen3-ASR](https://github.com/QwenLM/Qwen3-ASR) 及对应模型托管方为准（Apache-2.0 等）

---

<p align="center">
  <sub>Made for offline subtitling · 问题反馈请附系统版本、显卡与显存、所用模型（0.6B / 1.7B）与报错信息</sub>
</p>

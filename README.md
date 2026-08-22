# OneAsr

**本地 · 离线 · 批量音视频 → SRT 字幕**

Windows 桌面端。不上传云端，不依赖在线 API。基于阿里通义 [Qwen3-ASR](https://github.com/QwenLM/Qwen3-ASR) 与 ForcedAligner，从音视频直接产出带时间轴的字幕文件。

```text
音视频  →  VAD 分段  →  识别  →  对齐打轴  →  智能断句  →  *.srt
```

任意时刻显存中只驻留 **一个** 大模型（先 ASR，再 Aligner），4GB 级显卡也能跑通 0.6B 组合。

---

## 为什么选 OneAsr

| | |
|---|---|
| **完全本地** | 识别与对齐均在本机完成，素材不出机 |
| **批量友好** | 列表队列一次丢多个文件，进度与阶段一目了然 |
| **时间轴可靠** | ForcedAligner 词级对齐 + 字幕长度预设，不是“整段瞎估时间” |
| **一安装包两后端** | 同一二进制含 CUDA 引擎；无 N 卡自动走 CPU |
| **轻量界面** | 原生 GPUI 亮色列表，无 Electron 臃肿壳 |

---

## 功能

- **11 种源语言**：中文普通话、English、粤语、日本語、한국어、Français、Deutsch、Italiano、Español、Português、Русский  
  （与对齐模型能力对齐；**需手动指定语种**，不做自动语种检测）
- **双规格 ASR**：Qwen3-ASR **0.6B**（更快 / 省显存）· **1.7B**（更准 / 更吃资源）
- **词级时间轴**：Qwen3-ForcedAligner 对齐；日语分词已内嵌，无需额外模型目录
- **VAD 智能分段**：目标段长 **30–180 秒**（默认 60），长音频更稳
- **字幕长度预设**：短 / 标准 / 松，控制单行信息量
- **处理明细**：各阶段耗时可看，方便对比机器与参数
- **自定义输出目录**：默认 `{安装目录}/output/`，设置里可改到任意文件夹
- **无卡可用**：无 NVIDIA 时走 CPU（更慢，但功能完整）

---

## 下载与安装

> 源码仓库不含模型与安装包。请从 **[Releases](https://github.com/eclipse005/OneAsr/releases)** 获取 `setup` / 便携包。

| 包类型 | 说明 |
|--------|------|
| **安装包** `OneAsr_*_setup.exe` | 按向导安装（含 GUI + `oneasr-cli.exe`） |
| **便携包** `OneAsr_*_portable.zip` | 解压后运行 `oneasr.exe`（同样含 CLI） |

**首次使用：**

1. 运行 `oneasr.exe`
2. **设置** → 下载 **ASR**（建议先 0.6B）+ **ForcedAligner**（必下）
3. 有 N 卡 → 再装 **CUDA 运行库**（约 820MB，仅需较新显卡驱动，**不必**装 CUDA Toolkit）
4. 选好源语言 → 添加音视频 → 开始
5. 完成后在输出目录查看同名 `.srt`

---

## 模型与磁盘

安装包 **不含** 权重，需在设置中从 ModelScope 下载（可断点续传）。

| 组件 | 名称 | 约体积 | 说明 |
|------|------|--------|------|
| ASR（二选一） | Qwen3-ASR-0.6B | ~1.8 GB | 默认推荐 |
| | Qwen3-ASR-1.7B | ~4.4 GB | 更高精度 |
| 对齐（必需） | Qwen3-ForcedAligner-0.6B | ~1.7 GB | 所有 ASR 共用 |
| GPU（可选） | CUDA 12.x 运行库 | ~820 MB | cudart / cublas 等 |

常用组合约 **3.5～6.9 GB**，另建议预留任务临时空间。

路径约定：

```text
{app}/
  oneasr.exe
  oneasr-cli.exe      # 无界面命令行，与 GUI 同一套流水线
  bin/ffmpeg.exe      # 安装包已带；源码开发需自行放置
  dll/                # CUDA 运行库（设置内下载）
  models/             # ASR / Aligner 权重
  output/             # 默认字幕输出
  runs/               # 中间文件（成功后会清理）
  oneasr-error.log    # 崩溃/错误日志（闪退时可附上）
```

---

## 配置要求

| 项目 | 说明 |
|------|------|
| 系统 | Windows 10 / 11（64 位） |
| 显卡 | 推荐 NVIDIA，**4GB+** 显存（0.6B）；1.7B 建议 **6GB+** |
| 无独显 | 可用 CPU，速度明显更慢 |
| 驱动 | 保持较新即可（能正常玩游戏一般够用） |
| 网络 | 仅首次下模型 / CUDA 库需要；识别过程可离线 |

---

## 使用提示

- **语种选错**是时间轴/识别异常的最常见原因；粤语用「粤语」，其它中文方言一般选「中文普通话」
- 同一时刻只加载一个大模型，内存友好，但长队列会按文件串行处理
- 速度慢：检查是否已装 CUDA 运行库、后端是否为 Auto/GPU、驱动是否过旧
- 失败任务可在列表中重试；`runs/` 在失败时可能残留，便于排查

---

## 命令行 / Python

安装包和便携包都带 `oneasr-cli.exe`，与 GUI 同一条流水线。先用 GUI 把模型（和可选的 CUDA 组件）下载到安装目录，再调用 CLI。默认 `--app-root` 就是 exe 所在目录。

```powershell
oneasr-cli.exe transcribe --input "C:\path\to\video.mp4" --language zh --backend auto --output "C:\path\to\out.srt"
```

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
| ASR | [qwen3-asr-rs](https://github.com/eclipse005/qwen3-asr-rs) |
| Aligner | [qwen-aligner-rs](https://github.com/eclipse005/qwen-aligner-rs) |

---

## 从源码构建

**环境：** Rust（edition 2024）· [VS 2022 C++ 生成工具](https://visualstudio.microsoft.com/downloads/) · 可选 CUDA Toolkit 12.x（仅本机编 CUDA 特性时）

### 1. 准备 ffmpeg（必需）

源码仓库**不含** `ffmpeg.exe`（体积约 56MB）。请从 **[构建工具 Release](https://github.com/eclipse005/OneAsr/releases/tag/tools)** 下载后放到：

```text
OneAsr/bin/ffmpeg.exe
```

PowerShell 一键下载示例：

```powershell
New-Item -ItemType Directory -Force -Path bin | Out-Null
Invoke-WebRequest -Uri "https://github.com/eclipse005/OneAsr/releases/download/tools/ffmpeg.exe" -OutFile "bin\ffmpeg.exe"
```

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

默认 feature 含 CUDA 引擎；运行时无 DLL / 无 GPU 时仍可走 CPU。

---

## 许可

- 本项目代码：**MIT**
- 识别 / 对齐模型权重与协议以 [Qwen3-ASR](https://github.com/QwenLM/Qwen3-ASR) 及对应模型托管方为准（Apache-2.0 等）

---

<p align="center">
  <sub>Made for offline subtitling · 问题反馈请附系统版本、显卡与显存、所用模型（0.6B / 1.7B）与报错信息</sub>
</p>

# OneAsr

本地音视频批量转字幕。亮色列表 UI + **Qwen3-ASR** + **Qwen3-ForcedAligner**（与 VoxTrans 同款流水线与断句逻辑）。

## 依赖

| 组件 | Git |
|------|-----|
| ASR | https://github.com/eclipse005/qwen3-asr-rs.git |
| Aligner | https://github.com/eclipse005/qwen-aligner-rs.git |

默认模型路径（安装/开发布局）：

- ASR: `{app}/models/Qwen3-ASR-0.6B`（可切换 **1.7B**）
- Aligner: `{app}/models/Qwen3-ForcedAligner-0.6B`
- 日语对齐额外需要：`{app}/models/Qwen3-ForcedAligner-0.6B/nagisa/`（nagisa 分词权重）

**识别语言**需在设置中指定（与 VoxTrans 一致，对齐模型支持 11 种）：  
中文普通话、English、粤语、日本語、한국어、Français、Deutsch、Italiano、Español、Português、Русский。  
**无自动识别**——对齐阶段需要固定语种。

设置面板可一键从 ModelScope 下载到 `{app}/models/`（逻辑同 VoxTrans），也可手动选已有目录。  
字幕默认写到 `{app}/output/`，可在 **设置 → 字幕输出目录** 改到任意文件夹。

## 运行

准备：将 `ffmpeg.exe` 放到 `bin/ffmpeg.exe`。

### GUI

```powershell
cd D:\OneAsr
cargo run -p oneasr --release
```

### CLI（无界面，便于回归 / 脚本）

```powershell
# 全流程 → {app-root}/output/{stem}.srt
cargo run -p oneasr-core --release --bin oneasr-cli --features cuda -- `
  transcribe --input "C:\path\to\video.mp4" --app-root "D:\OneAsr" `
  --language zh --chunk-seconds 60 --backend cuda

# 单段 ASR 文本（排查循环 / 过长）
cargo run -p oneasr-core --release --bin oneasr-cli --features cuda -- `
  asr-chunk --wav "D:\OneAsr\runs\...\input_16k.wav" `
  --start 722.75 --end 842.75 --language zh

# 详细分块日志
$env:ONEASR_PIPELINE_TRACE = "1"
```

产物二进制：`target\release\oneasr-cli.exe`。  
`oneasr-core` 默认启用 `cuda` feature。

## 打包（单一安装包）

**一个 setup**，二进制含 CUDA 引擎；无卡/无 DLL 时仍可用 CPU。  
GPU 用户在 **设置 → 下载 CUDA 运行库**（与 VoxTrans 相同的 ModelScope DLL：cudart/cublas/cublasLt/curand）。

需要：Rust、`bin/ffmpeg.exe`、[Inno Setup 6](https://jrsoftware.org/isinfo.php)。

```powershell
.\scripts\pack-release.ps1
# 产物：release\OneAsr_<ver>_setup.exe
```

| 路径 | 说明 |
|------|------|
| `dist\OneAsr\` | 暂存安装内容 |
| `release\OneAsr_<ver>_setup.exe` | 安装包 |

安装后：`models\` 下 ASR/Aligner，默认 `output\` 出 SRT，`runs\` 中间产物；CUDA 运行库下载到 `{app}\dll\`（与 exe 同级的 dll 目录）。  
UI 图标/音效在编译期嵌入 exe，安装目录**无** `assets\`。

## 输出

默认：

```text
{app_root}/output/{视频名}.srt
```

GUI：**设置 → 字幕输出目录** 可改到其它路径（仍为 `{所选目录}/{视频名}.srt`）。  
CLI：与 `--app-root` 对齐，写到 `{app-root}/output/{stem}.srt`（可用 `--output` 再复制一份）。

`runs/` 为任务 scratch：运行时临时目录，**成功后自动删除**；失败时可能残留便于排查。

## 流程

```text
ffmpeg → 16k mono
  → VAD 分段规划（30–180s 可调）
  → load ASR 一次 → 各段转写 → drop ASR
  → load Aligner 一次 → 各段对齐 → drop Aligner
  → 标点还原 + normalize_word_tokens
  → 断句（标点硬切 + 字幕长度 DP）
  → {output_dir}/{stem}.srt
```

任意时刻显存中只有一个大模型。

## 测试

```powershell
cargo test -p oneasr-core
cargo build -p oneasr
```

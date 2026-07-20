# OneAsr

本地音视频批量转字幕。亮色列表 UI + **Qwen3-ASR** + **Qwen3-ForcedAligner**（与 VoxTrans 同款流水线与断句逻辑）。

## 依赖

| 组件 | Git |
|------|-----|
| ASR | https://github.com/eclipse005/qwen3-asr-rs.git |
| Aligner | https://github.com/eclipse005/qwen-aligner-rs.git |

默认模型路径（安装/开发布局）：

- ASR: `{app}/models/Qwen3-ASR-0.6B`
- Aligner: `{app}/models/Qwen3-ForcedAligner-0.6B`

设置面板可一键从 ModelScope 下载到 `{app}/models/`（逻辑同 VoxTrans），也可手动选已有目录。

> 当前为开发/便携布局（exe + `bin/` + `assets/` + `models/`）。Windows 安装包（Inno/NSIS）待后续补充。

## 运行

准备：将 `ffmpeg.exe` 放到 `bin/ffmpeg.exe`。

```powershell
cd D:\OneAsr
cargo run -p oneasr
```

`oneasr-core` 默认启用 `cuda` feature。

## 输出

```text
{app_root}/output/{视频名}.srt
```

中间产物在 `runs/`（chunk wav、转写 txt、align json 等）。

## 流程

```text
ffmpeg → 16k mono
  → VAD 分段（~30–180s）
  → load ASR 一次 → 各段转写 → drop ASR
  → load Aligner 一次 → 各段对齐 → drop Aligner
  → 标点还原 + normalize_word_tokens
  → VoxTrans 断句（标点硬切 + 字幕长度 DP）
  → output/{stem}.srt
```

任意时刻显存中只有一个大模型。

## 测试

```powershell
cargo test -p oneasr-core
cargo build -p oneasr
```

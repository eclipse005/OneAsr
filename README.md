<div align="center">
  <h1>OneAsr</h1>
  <p>Local · Offline · Batch audio/video → SRT / TXT subtitles</p>
  <p>Windows / Linux / macOS · Built on Qwen3-ASR + ForcedAligner · Nothing leaves your machine</p>

  [Download](#download) · [Quick start](#quick-start) · [Screenshots](#screenshots) · [CLI](#cli) · [Release](https://github.com/eclipse005/OneAsr/releases)

  **English** · [简体中文](README.zh-CN.md)
</div>

```text
Media → [vocal separation] → VAD segmentation → transcription → word-level alignment → smart sentence segmentation → *.srt / *.txt
```

## Download

Current version **[v1.1.0](https://github.com/eclipse005/OneAsr/releases/tag/v1.1.0)** (click a file name to download):

| Platform | Download |
|------|------|
| Windows | [Installer](https://github.com/eclipse005/OneAsr/releases/download/v1.1.0/OneAsr_1.1.0_windows_setup.exe) · [Portable](https://github.com/eclipse005/OneAsr/releases/download/v1.1.0/OneAsr_1.1.0_windows_portable.zip) |
| Linux x64 | [.deb](https://github.com/eclipse005/OneAsr/releases/download/v1.1.0/OneAsr_1.1.0_linux_x64.deb) · [Portable](https://github.com/eclipse005/OneAsr/releases/download/v1.1.0/OneAsr_1.1.0_linux_x64.tar.gz) |
| macOS (Apple Silicon) | [Disk image](https://github.com/eclipse005/OneAsr/releases/download/v1.1.0/OneAsr_1.1.0_macos.dmg) |

## Quick start

1. Run the app (`oneasr.exe` on Windows, `oneasr` on Linux / macOS)
2. **Settings** → download the **ASR** model (0.6B recommended first) + **ForcedAligner** (required)
3. Pick the source language → add audio/video → Start
4. Grab the matching `.srt` from the output folder

## Screenshots

Main window (empty state) and the task list + settings drawer (same window size, side by side):

| Main window (empty state) | Tasks & settings |
|---|---|
| <img src="docs/images/main.png" width="420" alt="Main window, empty state" /> | <img src="docs/images/tasks-settings.png" width="420" alt="Task list and settings drawer" /> |

## Features

- **Fully local**: recognition and alignment run on your machine; after the models are downloaded you can go offline
- **Batch queue**: drop in many files at once; progress and stage are always visible
- **Reliable timing**: ForcedAligner word-level alignment + smart sentence segmentation — not whole-segment guesses
- **11 source languages**: 中文普通话, English, 粤语, 日本語, 한국어, Français, Deutsch, Italiano, Español, Português, Русский (set manually per task; no auto-detection)
- **Two ASR sizes**: 0.6B (faster / less VRAM) · 1.7B (more accurate / more resources)
- **Output options**: SRT / TXT, either or both; saved next to the video by default
- **Chinese script choice**: original / Simplified / Traditional (Chinese & Cantonese only; timing is unaffected)
- **Vocal separation (optional)**: built-in HTDemucs v4 strips BGM / noise before transcribing
- **GPU acceleration**: NVIDIA / AMD / Intel / Apple Silicon; falls back to CPU automatically when no discrete GPU is present

## Models & sizes

The installer ships **no** weights; models download from ModelScope on first use (resumable):

| Component | Approx. size | Notes |
|------|--------|------|
| Qwen3-ASR-0.6B-hf | ~1.6 GB | Default recommendation |
| Qwen3-ASR-1.7B-hf | ~4.1 GB | Higher accuracy |
| Qwen3-ForcedAligner-0.6B-hf | ~1.8 GB | Required, shared by all ASR sizes |
| HTDemucs v4 vocal weights | ~84 MB | Optional |

Common setups total about 3.5–6.0 GB. Only the model download needs network; transcription itself runs offline.

## CLI

The installer ships `oneasr-cli` (same pipeline as the GUI). Download the models via the GUI first, then:

```powershell
oneasr-cli.exe transcribe --input "video.mp4" --language zh --backend auto --output "out.srt"
```

`--txt` also writes a text transcript, `--script original|simplified|traditional` switches the Chinese glyph set, `--vocal-separation` separates vocals first. See `--help` for all options.

## Requirements

- OS: Windows 10 / 11, Linux, macOS (Apple Silicon)
- GPU: 4 GB+ VRAM recommended for 0.6B, 6 GB+ for 1.7B; CPU works without a discrete GPU (slower but usable)
- Network: only for the first model download

## Tips

- **Picking the wrong source language** is the most common failure; choose 粤语 for Cantonese, and 中文普通话 for other Chinese dialects
- With heavy BGM / noise, turn on vocal separation first — accuracy improves a lot
- The macOS build is unsigned: the DMG includes 安装 OneAsr.command + 安装说明.txt; install once following the notes
- On Linux, extract the tar.gz into your home folder
- On Linux, run `bash install-desktop.sh` from the portable package on first use — it registers the app icon and launcher (Linux does not embed icons in ELF executables)

## Community

Announced and discussed at: [LINUX DO](https://linux.do/) · [52pojie](https://www.52pojie.cn/) · [Appinn forums](https://meta.appinn.net/)

## License

- This project's code: **MIT**
- Model weights and their terms follow [Qwen3-ASR](https://github.com/QwenLM/Qwen3-ASR) and their hosts
- Engines: [qwen3-asr-wgpu](https://github.com/eclipse005/qwen3-asr-wgpu) · [qwen3-aligner-wgpu](https://github.com/eclipse005/qwen3-aligner-wgpu) · [demucs-wgpu](https://github.com/eclipse005/demucs-wgpu)

<p align="center">
  <sub>When reporting an issue, please include your OS version, GPU & VRAM, the model used (0.6B / 1.7B) and the error message</sub>
</p>

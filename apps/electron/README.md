# OneAsr Electron UI

Modern **light** Electron + Vite + React + TypeScript shell that drives the Rust
engine through `oneasr-cli`. The GPUI shell under `crates/oneasr` has been
removed on this branch; `master` keeps it as the GPUI line.

> **DO NOT PUSH** this branch for 大宝 until explicitly asked. Local commits only.

## Branch

`feat/electron-ui`

## Visual identity (浅色青绿)

Aligned with the previous UI theme tokens:

- Background `#f4f6f8`, panels white, text `#0f172a`
- Accent `#0d9488` / soft `#ccfbf1` / mist `#e6fffa`
- Frameless titlebar, teal logo mark, soft shadows, spring drawer / modal motion

## How to run

Requires Node 22.12+ (Electron 44); this repo is developed on Node 24.

```bash
# 引擎侧只需要 CLI；从仓库根构建
cargo build -p oneasr-core --bin oneasr-cli

cd apps/electron
npm install && npm run build
export DISPLAY=:4 ELECTRON_DISABLE_GPU=1 ONEASR_APP_ROOT=/workspace/oneasr-runtime
npx electron .
```

`resolveCli()` 会依次找 app root、`target/release`、`target/debug` 下的
`oneasr-cli[.exe]`，开发期不必手工拷贝二进制。

## Surfaces

Settings / Hotwords / Stats / Guide / Transcript matching 均为**居中弹窗**（非右侧抽屉），spring 缩放淡入。

| Surface | Notes |
|---|---|
| Empty + drag-drop | Light mist dropzone; media + matching transcripts can be dropped together |
| Task list | Start / Cancel / Remove / progress bar / stage / errors; per-row transcript chip |
| Transcript panel | Summary + speech-rate check + editor + smart split + re-read + remove + save & realign |
| Settings modal | Language, chunk, outputs, script, beautify, paths, ASR size/quant, aligner, demucs, SenseVoice path, backend, sound; Unsaved/Save/Reset; model ready pills; in-app download for missing catalog entries; open models folder |
| Hotwords | Basic `hotword.json` editor (groups / add / delete / default toggle); per-row group binding |
| Stats | Reads/writes `{app-root}/stats.jsonl`; KPI cards, 30d sparkline, 90d heatmap, language bars, session recent |
| Guide | Short real guide |
| Status bar | Models ready, ASR/aligner/demucs/SenseVoice, backend, running/queued counts, last message |
| Toasts | Dialog failures, save/reset feedback |

## Transcript matching

- Drop a media file with a same-stem `.txt` / `.srt` in one go, or drop a transcript onto an existing row. A single media + single transcript also pairs even when the names differ.
- One transcript line = one subtitle line. An `.srt`'s own timings are ignored and re-aligned with CTC; ASR and hotwords are skipped entirely.
- The row chip opens a panel with line/char counts, a speech-rate sanity check, the normalized text in an editor, smart sentence splitting, re-read from the source, and remove.
- Edits stay in the app's staged copy. The source transcript is **never written**; output uses `<stem>.aligned.srt` (and `.aligned.timeline.json`) so a same-named transcript is not overwritten.
- After a completed alignment, editing and saving marks the row as needing a realign. The panel only saves; the row’s start button runs align or transcribe for that task.

## Smoke

```bash
npm run smoke:settings
npm run smoke:transcribe
DISPLAY=:4 node scripts/launch-smoke.mjs
```

## Known gaps vs the previous UI

- **完成反馈不完整**：提示音开关会持久化，但没有任何播放实现；批量结束也没有汇总或提醒音。
- **模型下载限制**：一次只能下一个，只列 catalog 里缺失的模型；没有重新下载与速度显示；「打开模型目录」固定打开 `appRoot/models`。
- **Shell 健壮性**：没有单实例锁与版本号标签；渲染进程崩溃不进 `oneasr-error.log`；设置保存失败没有 UI 提示。
- **设置护栏较弱**：UI 允许把 SRT/TXT/ASS 全部关掉（运行时 core 会强制恢复 SRT）；下载中仍可改 ASR 尺寸/对齐器；Demucs 可以在缺权重时打开；热词可以绑到不支持的语言。
- **ASS 只生成不预览**：可以写 `.ass`，但没有内置播放器（旧版也没有）。

## Layout

```
apps/electron/src/main|preload|renderer|shared
apps/electron/scripts/smoke-*.mjs
```


## Screenshots

```bash
npm run build && npm run shots
# writes zh/en sets under apps/electron/shots/
```

UI language: Settings → UI language (`system` | `zh` | `en`). Draft changes switch copy immediately; **Save** writes `ui_language` into `settings.json`.

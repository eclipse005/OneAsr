# OneAsr Electron UI

Modern **light** Electron + Vite + React + TypeScript shell that reuses `/workspace/oneasr-runtime/oneasr-cli`. GPUI under `crates/oneasr` is left in place but should not be launched alongside this UI.

> **DO NOT PUSH** this branch for 大宝 until explicitly asked. Local commits only.

## Branch

`feat/electron-ui`

## Visual identity (浅色青绿)

Aligned with GPUI theme tokens:

- Background `#f4f6f8`, panels white, text `#0f172a`
- Accent `#0d9488` / soft `#ccfbf1` / mist `#e6fffa`
- Frameless titlebar, teal logo mark, soft shadows, spring drawer / modal motion

## How to run

```bash
cd /workspace/OneAsr/apps/electron
npm install && npm run build
export DISPLAY=:4 ELECTRON_DISABLE_GPU=1 ONEASR_APP_ROOT=/workspace/oneasr-runtime
npx electron .
```

Kill any leftover GPUI GUI first:

```bash
pkill -f '/workspace/oneasr-runtime/oneasr$' || true
```

## Surfaces

Settings / Hotwords / Stats / Guide / Welcome 均为**居中弹窗**（非右侧抽屉），spring 缩放淡入。

| Surface | Notes |
|---|---|
| Welcome modal | Got it / Open settings → persists `welcome_done` |
| Empty + drag-drop | Light mist dropzone |
| Task list | Start / Cancel / Remove / progress bar / stage / errors |
| Settings drawer | Language, chunk, outputs, script, beautify, paths, ASR size/quant, aligner, demucs, SenseVoice path, backend, sound; Unsaved/Save/Reset; model ready pills; open models folder (no multi-GB download) |
| Hotwords | Basic `hotword.json` editor (groups / top_k / threshold / default toggle) |
| Stats | Reads/writes `{app-root}/stats.jsonl`; KPI cards, 30d sparkline, 90d heatmap, language bars, session recent |
| Guide | Short real guide |
| Status bar | Models ready, ASR/aligner/demucs/SenseVoice, backend, running/queued counts |
| Toasts | Dialog failures, save/reset feedback |

## Smoke

```bash
npm run smoke:settings
npm run smoke:transcribe
DISPLAY=:4 node scripts/launch-smoke.mjs
```

## Known gaps vs GPUI

- No ModelScope in-app download (open folder + browse instead)
- No tray / close-to-tray
- Hotword injection needs CLI `--hotwords` (runtime CLI may ignore; bench uses `oneasr-cli-hotword`)
- Sounds toggle persisted, not played
- No ASS preview player
- CLI `--output` same-path self-copy can zero SRT; jobs omit `--output` when redundant

## Layout

```
apps/electron/src/main|preload|renderer|shared
apps/electron/scripts/smoke-*.mjs
```

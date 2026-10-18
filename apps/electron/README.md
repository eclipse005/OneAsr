# OneAsr Electron UI

Modern Electron + Vite + React + TypeScript shell that **reuses** the existing Rust pipeline via `/workspace/oneasr-runtime/oneasr-cli`. The GPUI crate under `crates/oneasr` is left in place.

> **DO NOT PUSH** this branch for 大宝 until explicitly asked. Local commits only.

## Branch

`feat/electron-ui`

## How to run (GUI)

```bash
cd /workspace/OneAsr/apps/electron
npm install
npm run build

# Headless / shared box display
export DISPLAY=:4
export ELECTRON_DISABLE_GPU=1
export ONEASR_APP_ROOT=/workspace/oneasr-runtime

npx electron .
# or:
node scripts/launch-smoke.mjs
```

Dev (HMR):

```bash
DISPLAY=:4 ELECTRON_DISABLE_GPU=1 ONEASR_APP_ROOT=/workspace/oneasr-runtime npm run dev
```

## What was rebuilt

| Surface | Status |
|---|---|
| Welcome / onboarding modal | Done |
| Main window empty state + drag-drop | Done |
| Task list / Add / Start all / Clear | Done |
| Settings drawer (all core sections) | Done |
| Stats panel | Done (session stats) |
| Guide panel (short real guide) | Done |
| Status bar models ready | Done |
| Frameless titlebar (Linux) | Done |
| 中文 + English UI | Done |
| Persist `settings.json` under app-root | Done |
| Spawn `oneasr-cli` for transcribe | Done |

## Layout

```
apps/electron/
  src/main/          # window, dialogs, settings I/O, CLI job queue
  src/preload/       # contextBridge IPC
  src/renderer/      # React UI
  src/shared/        # Settings / Task types (mirror oneasr-core field names)
  scripts/           # smoke-settings / smoke-transcribe / launch-smoke
  README.md
```

Settings fields mirror `crates/oneasr-core/src/settings.rs` JSON keys (`asr_model`, `ui_language`, `subtitle_length_preset`, `chunk_target_seconds`, `output_srt|txt|ass`, `text_script`, `subtitle_beautify`, `vocal_separation`, `backend`, `sound`, `welcome_done`, …).

## Smoke checklist

```bash
npm run build
npm run smoke:settings
npm run smoke:transcribe   # CPU SRT; logs wall time
DISPLAY=:4 node scripts/launch-smoke.mjs
```

## Known gaps vs GPUI

- No system tray / close-to-tray
- No hotword editor / SenseVoice UI
- No in-app model download
- Stats are in-memory for the Electron session (not GPUI persistent history DB)
- No ASS karaoke preview player
- Sounds toggle is persisted but not played yet
- CLI quirk: `--output` pointing at the same path the pipeline already wrote can truncate SRT to 0 bytes; Electron jobs omit `--output` when it matches `{data-root}/output/{stem}.*`
- Drag-drop relies on Electron `File.path` (works in packaged/Electron; not in plain browser)
- Older `oneasr-cli` builds may ignore `--asr`; settings.json path is the source of truth

## Non-goals (honoured)

- Did **not** rewrite oneasr-core / wgpu
- Did **not** delete GPUI code
- Did **not** push / open PR

# OneAsr Electron UI

Modern **light** Electron + Vite + React + TypeScript shell that drives the Rust
engine through `oneasr-cli`. The GPUI shell under `crates/oneasr` has been
removed on this branch; `master` keeps it as the GPUI line.

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
# 引擎侧只需要 CLI；从仓库根构建（或使用旁路 oneasr-runtime）
cargo build -p oneasr-core --bin oneasr-cli

cd apps/electron
npm install && npm run build
export DISPLAY=:4 ELECTRON_DISABLE_GPU=1
npm start
# 或显式指定 app root（一般不需要，见下）：
# ONEASR_APP_ROOT=/workspace/oneasr-runtime npm start
```

**app root 只有一处决策**：`src/main/settings.ts` 的 `resolveAppRoot()`
（`npm run dev` / `start` 不再预先写 `ONEASR_APP_ROOT`）。
它先看 `ONEASR_APP_ROOT`（**必须有 oneasr-cli 才采纳**，否则忽略并记日志），
再走 `chooseDevAppRoot()` —— 优先选**带 oneasr-cli** 的目录，
且 `hasCliBinary()` 把 `target/release`、`target/debug` 也算在内 —— 所以仓库根
只要 `cargo build` 过就能直接用，不必把二进制拷到根目录。
仓库里常有 `bin/ffmpeg` 却没有 CLI，此时会落到旁路 `oneasr-runtime`。

`resolveCli()` 会依次找 app root、`target/release`、`target/debug` 下的
`oneasr-cli[.exe]`。开发期数据目录跟 app root 走。

> 以前 `scripts/run-with-runtime.mjs` 另写了一份「只看根目录」的探测并把它
> 当 `ONEASR_APP_ROOT` 塞给主进程，遮掉了上面这份正确逻辑（表现为状态栏
> 一直「未找到引擎」）。那份重复实现已删：**不要再加第二处 root 探测**。

## Release packaging

```bash
# 一次性安装打包依赖
cd apps/electron && npm ci

# Windows（Inno Setup 6）→ setup.exe + portable.zip
pwsh -File scripts/pack-electron.ps1

# Linux → .deb + .tar.gz
./scripts/pack-electron.sh --os linux --arch x64

# macOS → unsigned .dmg（含安装说明与安装脚本）
./scripts/pack-electron.sh --os macos --arch arm64
```

产物在仓库根 `release/`。打包脚本从 `Cargo.toml` 读版本，并断言
`apps/electron/package.json` 与它一致；Electron 应用、`oneasr-cli` 和**校验过
sha256 的 ffmpeg** 一起进产物（`electron-builder.yml` 的 `extraResources`）。

运行时布局：

```text
OneAsr/
  OneAsr[.exe]                     Electron 可执行文件
  resources/runtime/oneasr-cli     CLI
  resources/runtime/bin/ffmpeg     媒体转换
  resources/icons/                 窗口/托盘图标
```

数据目录与安装目录分开：Windows per-user 安装、Linux tar.gz 解压目录可写时，
`settings.json` / `models/` / `output/` / 日志就在应用旁；Linux `.deb` 的
`/opt` 与 macOS `.app` 只读时自动落到平台 userData（见 `src/main/settings.ts`）。
`ONEASR_DATA_DIR` 可显式覆盖。

## Surfaces

Settings / Hotwords / Stats / Guide / Transcript matching 均为**居中弹窗**（非右侧抽屉），spring 缩放淡入。

| Surface | Notes |
|---|---|
| Empty + drag-drop | Light mist dropzone; media + matching transcripts can be dropped together |
| Task list | Start / Cancel / Remove / progress bar / stage / errors; per-row transcript chip |
| Transcript panel | Summary + speech-rate check + editor + smart split + re-read + remove + save & realign |
| Settings modal | Language, chunk, outputs, script, beautify, paths, ASR size/quant, aligner, demucs, SenseVoice path, backend, sound; Unsaved/Save/Reset; model ready pills; in-app download for missing catalog entries; open models folder |
| Hotwords | Basic `hotword.json` editor (groups / add / delete / default toggle); per-row group binding |
| Stats | Reads/writes `{data-root}/stats.jsonl`; KPI cards, 30d sparkline, 90d heatmap, language bars, session recent |
| Guide | Short real guide |
| Status bar | Models ready, ASR/aligner/demucs/SenseVoice, backend, running/queued counts, last message |
| Toasts | Dialog failures, save/reset feedback |

## Transcript matching

- Drop a media file with a same-stem `.txt` / `.srt` in one go, or drop a transcript onto an existing row. A single media + single transcript also pairs even when the names differ.
- One transcript line = one subtitle line. An `.srt`'s own timings are ignored and re-aligned with CTC; ASR and hotwords are skipped entirely.
- The row chip opens a panel with line/char counts, a speech-rate sanity check, the normalized text in an editor, smart sentence splitting, re-read from the source, and remove.
- Edits stay in the app's staged copy. The source transcript is **never written**; output uses `<stem>.aligned.srt` so a same-named transcript is not overwritten. The GUI passes `--no-timeline`: the measured-timeline sidecar is a CLI feature and is not produced by the app.
- After a completed alignment, editing and saving marks the row as needing a realign. The panel only saves; the row’s start button runs align or transcribe for that task.

## Smoke

```bash
# 这两个需要显式指定 app root（脚本不再猜路径，也不再硬编码任何本机路径）
ONEASR_APP_ROOT=/path/to/runtime npm run smoke:settings
ONEASR_APP_ROOT=/path/to/runtime ONEASR_SMOKE_INPUT=/path/to/audio.wav npm run smoke:transcribe
DISPLAY=:4 node scripts/launch-smoke.mjs   # 未设 ONEASR_APP_ROOT 就由主进程探测
```

## Known gaps vs the previous UI

- **完成反馈不完整**：提示音开关会持久化，但没有任何播放实现；批量结束也没有汇总或提醒音。
- **模型下载限制**：一次只能下一个；下载落到当前显示的模型路径（保存后该路径用于转写），没有速度显示；「打开模型目录」打开数据目录下的 `models`。
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

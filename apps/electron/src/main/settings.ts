import fs from 'fs'
import path from 'path'
import { writeFileAtomicSync } from './atomic'
import type { Settings } from '../shared/types'

/**
 * 壳里的 settings 层**只做三件事**：安装目录探测、oneasr-cli 落点、settings.json 的原始读写。
 *
 * 业务规则一律不在这里——这是架构约定（Electron 只是 UI 壳）：
 * - 模型就绪、路径解析、开跑门禁 → `oneasr-cli status --json`
 * - 可选模型清单与下载落点 → `oneasr-cli status --json` 的 catalog / `download`
 * 壳里不再出现目录名、后缀、候选顺序、就绪判据这类规则（以前这里有
 * hasModelFiles / resolveAsrDir / checkModels / modelsInventory 五处复刻，
 * 判据还比 Rust 松，导致启动门禁可能放行缺文件的目录）。
 *
 * TODO(架构收口·第三阶段)：settings 的默认值与规范化也搬进 Rust
 * （`settings get|set --json`），届时这里只剩 detectAppRoot / resolveCli / settingsPath。
 */

const CLI_NAMES =
  process.platform === 'win32' ? ['oneasr-cli.exe', 'oneasr-cli'] : ['oneasr-cli', 'oneasr-cli.exe']

const FFMPEG_MARKERS =
  process.platform === 'win32'
    ? ['bin/ffmpeg.exe', 'bin/ffmpeg']
    : ['bin/ffmpeg', 'bin/ffmpeg.exe']

function hasFfmpeg(dir: string): boolean {
  return FFMPEG_MARKERS.some((m) => fs.existsSync(path.join(dir, m)))
}

/** 该根下有没有可执行的 oneasr-cli（含 target/release|debug）。 */
export function hasCliBinary(dir: string): boolean {
  for (const n of CLI_NAMES) {
    if (
      fs.existsSync(path.join(dir, n)) ||
      fs.existsSync(path.join(dir, 'target', 'release', n)) ||
      fs.existsSync(path.join(dir, 'target', 'debug', n))
    ) {
      return true
    }
  }
  return false
}

/**
 * 从 startDir 往上找含 bin/ffmpeg 的目录（与 Rust resolve_app_root 同一约定）。
 * 开发期 out/main → 仓库根通常能命中。
 */
export function walkFfmpegRoots(startDir: string, maxLevels = 6): string[] {
  const found: string[] = []
  let dir = startDir
  for (let i = 0; i < maxLevels; i++) {
    if (hasFfmpeg(dir)) found.push(dir)
    const parent = path.dirname(dir)
    if (parent === dir) break
    dir = parent
  }
  return found
}

/**
 * 开发期选 app root：优先「带 CLI」的目录，避免仓库里只有 bin/ffmpeg 软链、
 * 却没有 oneasr-cli 时误把仓库根当运行时（随后 spawn ENOENT → 假「CLI 过旧」）。
 *
 * 候选顺序：walk 到的 ffmpeg 根 → 各根旁的 sibling `oneasr-runtime` → extraRoots。
 */
export function chooseDevAppRoot(
  walkedFfmpegRoots: string[],
  extraRoots: string[] = []
): string | undefined {
  const siblings = walkedFfmpegRoots.map((r) => path.join(path.dirname(r), 'oneasr-runtime'))
  const candidates = Array.from(new Set([...walkedFfmpegRoots, ...siblings, ...extraRoots]))
  const withCli = candidates.find((c) => hasCliBinary(c))
  if (withCli) return withCli
  return walkedFfmpegRoots[0]
}

/**
 * 安装目录 = 含 bin/ffmpeg[.exe] 的目录（与 Rust 侧 resolve_app_root 同一约定）。
 * 从本文件所在目录往上找，最多 6 层：开发期 out/main → 仓库根即可命中，
 * 省掉每次启动都要 export ONEASR_APP_ROOT。
 *
 * 若仓库根没有 CLI、旁路有 `oneasr-runtime`（本 box 布局），优先用后者。
 */
function detectAppRoot(): string | undefined {
  const walked = walkFfmpegRoots(__dirname)
  const extras: string[] = []
  if (fs.existsSync('/workspace/oneasr-runtime')) extras.push('/workspace/oneasr-runtime')
  return chooseDevAppRoot(walked, extras)
}

/**
 * 打包后的资源根：electron-builder 把 CLI / ffmpeg 放在 resources/runtime。
 * `process.defaultApp` 为 true 是开发期（electron .），此时仍从源码树探测。
 */
const PACKAGED = !process.defaultApp && Boolean(process.resourcesPath)
const INSTALL_ROOT = PACKAGED ? path.dirname(process.execPath) : undefined

/** 应用/工具目录：CLI 与 bin/ffmpeg 的落点（安装目录里的 resources/runtime）。 */
export const DEFAULT_APP_ROOT =
  process.env.ONEASR_APP_ROOT ||
  (PACKAGED
    ? path.join(process.resourcesPath!, 'runtime')
    : detectAppRoot() || '/workspace/oneasr-runtime')

let userDataDir: string | undefined
let cachedDataRoot: string | undefined

/** main 在 app ready 后调用一次：这是只读安装目录之外的兜底数据目录。 */
export function setUserDataDir(dir: string): void {
  userDataDir = dir
  cachedDataRoot = undefined
}

/** 目录可写吗？Windows 的 access(W_OK) 不可靠，直接试建一个临时文件。 */
function isWritableDir(dir: string): boolean {
  const probe = path.join(dir, `.oneasr-write-test-${process.pid}-${Date.now()}`)
  try {
    fs.writeFileSync(probe, '')
    fs.unlinkSync(probe)
    return true
  } catch {
    return false
  }
}

/**
 * 数据目录：settings.json / models / output / runs / hotword / stats / 日志。
 *
 * - 开发期就是仓库根（保持现有 npm run dev 行为）
 * - 打包后优先应用所在的安装目录（Windows per-user 安装、Linux tar.gz 解压目录）
 * - 只读安装位置（Linux /opt、macOS .app）或 macOS 一律用 userData
 *
 * 安装目录与数据目录分开，是 Linux .deb / macOS .app 能正常下载模型、保存设置的前提。
 */
export function dataRoot(): string {
  if (cachedDataRoot) return cachedDataRoot
  let root: string
  if (process.env.ONEASR_DATA_DIR) {
    root = process.env.ONEASR_DATA_DIR
  } else if (!PACKAGED) {
    root = DEFAULT_APP_ROOT
  } else if (process.platform !== 'darwin' && INSTALL_ROOT && isWritableDir(INSTALL_ROOT)) {
    root = INSTALL_ROOT
  } else {
    root = userDataDir || DEFAULT_APP_ROOT
  }
  cachedDataRoot = root
  return root
}

export function settingsPath(root = dataRoot()): string {
  return path.join(root, 'settings.json')
}

/**
 * settings.json 缺失时的占位值（形状与 Rust 的 `Settings` 对齐）。
 * 真正的默认值/规范化归 Rust：CLI 每次加载都会 normalize，并会把模型目录
 * 按 app/data root 重新解析——所以这里只需要给出「字段齐全的空壳」。
 */
export function defaultSettings(root = dataRoot()): Settings {
  const models = path.join(root, 'models')
  return {
    asr_model: 'Qwen3-ASR-0.6B-hf',
    asr_model_dir: path.join(models, 'Qwen3-ASR-0.6B-hf'),
    asr_dirs: {},
    aligner_model: 'omniASR-CTC-300M-v2-hf',
    aligner_model_dir: path.join(models, 'omniASR-CTC-300M-v2-hf'),
    aligner_dirs: {},
    backend: 'auto',
    max_new_tokens: 2048,
    language: 'zh',
    ui_language: 'system',
    subtitle_length_preset: 'standard',
    chunk_target_seconds: 60,
    output_dir: path.join(root, 'output'),
    save_next_to_source: true,
    output_srt: true,
    output_txt: false,
    output_ass: false,
    subtitle_beautify: false,
    text_script: 'original',
    vocal_separation: false,
    demucs_model_dir: path.join(models, 'htdemucs_ft'),
    sensevoice_model_dir: path.join(models, 'SenseVoiceSmall'),
    sound: true,
    close_to_tray: false
  }
}

/** 读 settings.json（只做 merge，不做规则；规范化归 Rust）。 */
export function loadSettings(root = dataRoot()): Settings {
  const base = defaultSettings(root)
  const p = settingsPath(root)
  if (!fs.existsSync(p)) return base
  try {
    const raw = JSON.parse(fs.readFileSync(p, 'utf8')) as Partial<Settings>
    return {
      ...base,
      ...raw,
      asr_dirs: { ...base.asr_dirs, ...(raw.asr_dirs || {}) },
      aligner_dirs: { ...base.aligner_dirs, ...(raw.aligner_dirs || {}) }
    }
  } catch {
    return base
  }
}

/** 写 settings.json：与磁盘上已有的字段合并（保住壳还不认识的字段）。 */
export function saveSettings(settings: Settings, root = dataRoot()): Settings {
  const p = settingsPath(root)
  let existing: Record<string, unknown> = {}
  if (fs.existsSync(p)) {
    try {
      existing = JSON.parse(fs.readFileSync(p, 'utf8'))
    } catch {
      existing = {}
    }
  }
  const out = { ...existing, ...settings }
  writeFileAtomicSync(p, JSON.stringify(out, null, 2) + '\n')
  return loadSettings(root)
}

/** oneasr-cli 落点：运行目录优先，其次开发期的 target/release|debug（免去手工拷）。 */
export function resolveCli(appRoot = DEFAULT_APP_ROOT): string {
  const candidates = [
    ...CLI_NAMES.map((n) => path.join(appRoot, n)),
    ...CLI_NAMES.map((n) => path.join(appRoot, 'target', 'release', n)),
    ...CLI_NAMES.map((n) => path.join(appRoot, 'target', 'debug', n))
  ]
  return candidates.find((p) => fs.existsSync(p)) ?? path.join(appRoot, CLI_NAMES[0]!)
}

/**
 * 把 GUI 的输出设置交给 CLI：最终目录由 Rust 的 `srt_target_dir` 决定
 * （源文件旁优先，源目录不可写时回退到 `--output-dir`）。壳不自己拼路径，
 * 否则 SRT 与 TXT/ASS 会落到两个地方。
 */
export function outputFlags(settings: Settings): string[] {
  const flags: string[] = []
  if (settings.save_next_to_source) flags.push('--save-next-to-source')
  if (settings.output_dir) flags.push('--output-dir', settings.output_dir)
  return flags
}

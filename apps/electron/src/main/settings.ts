import fs from 'fs'
import path from 'path'
import type { Settings } from '../shared/types'

/**
 * 壳里的 settings 层**只做三件事**：安装目录探测、oneasr-cli 落点、settings.json 的原始读写。
 *
 * 业务规则一律不在这里——这是架构约定（Electron 只是 UI 壳）：
 * - 模型就绪、路径解析、开跑门禁 → `oneasr-cli status --json`
 * - 可选模型清单与下载落点 → `oneasr-cli status --json` 的 catalog / `download`
 * 壳里不再出现目录名、阈值、候选顺序、就绪判据这类规则（以前这里有
 * hasModelFiles / resolveAsrDir / checkModels / modelsInventory 五处复刻，
 * 判据还比 Rust 松，导致启动门禁可能放行缺文件的目录）。
 *
 * TODO(架构收口·第三阶段)：settings 的默认值与规范化也搬进 Rust
 * （`settings get|set --json`），届时这里只剩 detectAppRoot / resolveCli / settingsPath。
 */

/**
 * 安装目录 = 含 bin/ffmpeg[.exe] 的目录（与 Rust 侧 resolve_app_root 同一约定）。
 * 从本文件所在目录往上找，最多 6 层：开发期 out/main → 仓库根即可命中，
 * 省掉每次启动都要 export ONEASR_APP_ROOT。
 */
function detectAppRoot(): string | undefined {
  const markers =
    process.platform === 'win32'
      ? ['bin/ffmpeg.exe', 'bin/ffmpeg']
      : ['bin/ffmpeg', 'bin/ffmpeg.exe']
  let dir = __dirname
  for (let i = 0; i < 6; i++) {
    if (markers.some((m) => fs.existsSync(path.join(dir, m)))) return dir
    const parent = path.dirname(dir)
    if (parent === dir) break
    dir = parent
  }
  return undefined
}

/** 环境变量优先（Linux 上运行目录与代码目录分离时仍然靠它），其次自动探测。 */
export const DEFAULT_APP_ROOT =
  process.env.ONEASR_APP_ROOT || detectAppRoot() || '/workspace/oneasr-runtime'

export function settingsPath(appRoot = DEFAULT_APP_ROOT): string {
  return path.join(appRoot, 'settings.json')
}

/**
 * settings.json 缺失时的占位值（形状与 Rust 的 `Settings` 对齐）。
 * 真正的默认值/规范化归 Rust：CLI 每次加载都会 normalize，并会把模型目录
 * 按 app/data root 重新解析——所以这里只需要给出「字段齐全的空壳」。
 */
export function defaultSettings(appRoot = DEFAULT_APP_ROOT): Settings {
  const models = path.join(appRoot, 'models')
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
    output_dir: path.join(appRoot, 'output'),
    save_next_to_source: true,
    output_srt: true,
    output_txt: false,
    output_ass: false,
    subtitle_beautify: false,
    text_script: 'simplified',
    vocal_separation: false,
    demucs_model_dir: path.join(models, 'htdemucs_ft'),
    sensevoice_model_dir: path.join(models, 'SenseVoiceSmall'),
    sound: true,
    close_to_tray: false,
    welcome_done: false
  }
}

/** 读 settings.json（只做 merge，不做规则；规范化归 Rust）。 */
export function loadSettings(appRoot = DEFAULT_APP_ROOT): Settings {
  const base = defaultSettings(appRoot)
  const p = settingsPath(appRoot)
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
export function saveSettings(settings: Settings, appRoot = DEFAULT_APP_ROOT): Settings {
  const p = settingsPath(appRoot)
  fs.mkdirSync(path.dirname(p), { recursive: true })
  let existing: Record<string, unknown> = {}
  if (fs.existsSync(p)) {
    try {
      existing = JSON.parse(fs.readFileSync(p, 'utf8'))
    } catch {
      existing = {}
    }
  }
  const out = { ...existing, ...settings }
  fs.writeFileSync(p, JSON.stringify(out, null, 2) + '\n', 'utf8')
  return loadSettings(appRoot)
}

/** oneasr-cli 落点：运行目录优先，其次开发期的 target/release|debug（免去手工拷）。 */
export function resolveCli(appRoot = DEFAULT_APP_ROOT): string {
  const names =
    process.platform === 'win32' ? ['oneasr-cli.exe', 'oneasr-cli'] : ['oneasr-cli', 'oneasr-cli.exe']
  const candidates = [
    ...names.map((n) => path.join(appRoot, n)),
    ...names.map((n) => path.join(appRoot, 'target', 'release', n)),
    ...names.map((n) => path.join(appRoot, 'target', 'debug', n))
  ]
  return candidates.find((p) => fs.existsSync(p)) ?? path.join(appRoot, 'oneasr-cli')
}

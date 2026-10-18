import fs from 'fs'
import path from 'path'
import type { ModelEntry, Settings } from '../shared/types'

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

const ASR_CATALOG = [
  'Qwen3-ASR-0.6B-hf',
  'Qwen3-ASR-0.6B-int8',
  'Qwen3-ASR-1.7B-hf',
  'Qwen3-ASR-1.7B-int8'
] as const

const ALIGNER_CATALOG = [
  'omniASR-CTC-300M-v2-hf',
  'Qwen3-ForcedAligner-0.6B-hf'
] as const

export function settingsPath(appRoot = DEFAULT_APP_ROOT): string {
  return path.join(appRoot, 'settings.json')
}

export function defaultSettings(appRoot = DEFAULT_APP_ROOT): Settings {
  const models = path.join(appRoot, 'models')
  const asrInt8 = path.join(models, 'Qwen3-ASR-0.6B-int8')
  const asrHf = path.join(models, 'Qwen3-ASR-0.6B-hf')
  const asrDir = fs.existsSync(asrInt8) ? asrInt8 : asrHf
  const asrModel = path.basename(asrDir)
  return {
    asr_model: asrModel,
    asr_model_dir: asrDir,
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

function hasModelFiles(dir: string): boolean {
  if (!dir || !fs.existsSync(dir)) return false
  const files = ['model.safetensors', 'model.safetensors.index.json', 'config.json']
  return files.some((f) => fs.existsSync(path.join(dir, f)))
}

export function resolveAsrDir(settings: Settings, appRoot = DEFAULT_APP_ROOT): string {
  if (hasModelFiles(settings.asr_model_dir)) return settings.asr_model_dir
  const models = path.join(appRoot, 'models')
  const id = settings.asr_model || ''
  const size = id.includes('1.7B') ? '1.7B' : '0.6B'
  const candidates = [
    path.join(models, `Qwen3-ASR-${size}-int8`),
    path.join(models, `Qwen3-ASR-${size}-hf`),
    ...ASR_CATALOG.map((n) => path.join(models, n))
  ]
  for (const c of candidates) {
    if (hasModelFiles(c)) return c
  }
  return settings.asr_model_dir
}

export function normalizeSettings(raw: Partial<Settings>, appRoot = DEFAULT_APP_ROOT): Settings {
  const base = defaultSettings(appRoot)
  const merged: Settings = {
    ...base,
    ...raw,
    asr_dirs: { ...base.asr_dirs, ...(raw.asr_dirs || {}) },
    aligner_dirs: { ...base.aligner_dirs, ...(raw.aligner_dirs || {}) }
  }
  if (!merged.output_srt && !merged.output_txt && !merged.output_ass) {
    merged.output_srt = true
  }
  merged.chunk_target_seconds = Math.min(180, Math.max(30, Number(merged.chunk_target_seconds) || 60))
  if (!['short', 'standard', 'loose'].includes(merged.subtitle_length_preset)) {
    merged.subtitle_length_preset = 'standard'
  }
  if (!['auto', 'gpu', 'cpu'].includes(merged.backend)) merged.backend = 'auto'
  if (!['system', 'zh', 'en'].includes(merged.ui_language)) merged.ui_language = 'system'
  if (!['simplified', 'traditional', 'original'].includes(merged.text_script)) {
    merged.text_script = 'simplified'
  }
  const resolved = resolveAsrDir(merged, appRoot)
  if (resolved !== merged.asr_model_dir && hasModelFiles(resolved)) {
    merged.asr_model_dir = resolved
    merged.asr_model = path.basename(resolved)
  }
  return merged
}

export function loadSettings(appRoot = DEFAULT_APP_ROOT): Settings {
  const p = settingsPath(appRoot)
  if (!fs.existsSync(p)) return defaultSettings(appRoot)
  try {
    const raw = JSON.parse(fs.readFileSync(p, 'utf8')) as Partial<Settings>
    return normalizeSettings(raw, appRoot)
  } catch {
    return defaultSettings(appRoot)
  }
}

export function saveSettings(settings: Settings, appRoot = DEFAULT_APP_ROOT): Settings {
  const normalized = normalizeSettings(settings, appRoot)
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
  const out = { ...existing, ...normalized }
  fs.writeFileSync(p, JSON.stringify(out, null, 2) + '\n', 'utf8')
  return normalized
}

export function checkModels(settings: Settings, appRoot = DEFAULT_APP_ROOT) {
  const asrPath = resolveAsrDir(settings, appRoot)
  const alignerPath = settings.aligner_model_dir
  const demucsPath = settings.demucs_model_dir
  const sensevoicePath =
    settings.sensevoice_model_dir || path.join(appRoot, 'models', 'SenseVoiceSmall')
  const asrReady = hasModelFiles(asrPath)
  const alignerReady =
    hasModelFiles(alignerPath) || fs.existsSync(path.join(alignerPath, 'config.json'))
  const demucsReady =
    fs.existsSync(path.join(demucsPath, 'htdemucs_ft_vocals.safetensors')) ||
    fs.existsSync(path.join(demucsPath, 'model.safetensors')) ||
    (fs.existsSync(demucsPath) && fs.readdirSync(demucsPath).length > 0)
  const sensevoiceReady =
    hasModelFiles(sensevoicePath) ||
    (fs.existsSync(sensevoicePath) && fs.readdirSync(sensevoicePath).length > 0)
  const needAligner = settings.output_srt || settings.output_ass
  const ready = asrReady && (!needAligner || alignerReady)
  return {
    asrReady,
    alignerReady,
    demucsReady,
    sensevoiceReady,
    asrPath,
    alignerPath,
    demucsPath,
    sensevoicePath,
    ready,
    asrLabel: path.basename(asrPath) || settings.asr_model,
    alignerLabel: path.basename(alignerPath) || settings.aligner_model,
    backend: settings.backend || 'auto'
  }
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

/**
 * 模型清单：目录名取自本文件已有的 catalog 常量（不新开真源），就绪判据与
 * checkModels 同源。渲染层只拿它来画「已装 / 未装 + 下载按钮」。
 */
export function modelsInventory(appRoot = DEFAULT_APP_ROOT): ModelEntry[] {
  const models = path.join(appRoot, 'models')
  const entries: ModelEntry[] = []
  const push = (id: string, kind: ModelEntry['kind'], dir: string, ready: boolean): void => {
    entries.push({ id, kind, installed: ready, dir })
  }
  for (const id of ASR_CATALOG) {
    const dir = path.join(models, id)
    push(id, 'asr', dir, hasModelFiles(dir))
  }
  for (const id of ALIGNER_CATALOG) {
    const dir = path.join(models, id)
    push(id, 'aligner', dir, hasModelFiles(dir) || fs.existsSync(path.join(dir, 'config.json')))
  }
  const demucs = path.join(models, 'htdemucs_ft')
  push(
    'htdemucs_ft',
    'demucs',
    demucs,
    fs.existsSync(demucs) && (fs.readdirSync(demucs).length > 0)
  )
  const sensevoice = path.join(models, 'SenseVoiceSmall')
  push(
    'SenseVoiceSmall',
    'sensevoice',
    sensevoice,
    fs.existsSync(sensevoice) && fs.readdirSync(sensevoice).length > 0
  )
  return entries
}

export { ASR_CATALOG, ALIGNER_CATALOG }

/** Mirrors crates/oneasr-core Settings JSON field names. */
export interface Settings {
  asr_model: string
  asr_model_dir: string
  asr_dirs: Record<string, string>
  aligner_model: string
  aligner_model_dir: string
  aligner_dirs: Record<string, string>
  backend: 'auto' | 'gpu' | 'cpu' | string
  max_new_tokens: number
  language: string
  ui_language: string
  subtitle_length_preset: string
  chunk_target_seconds: number
  output_dir: string
  save_next_to_source: boolean
  output_srt: boolean
  output_txt: boolean
  output_ass: boolean
  subtitle_beautify: boolean
  text_script: string
  vocal_separation: boolean
  demucs_model_dir: string
  sensevoice_model_dir?: string
  sound: boolean
  close_to_tray?: boolean
  welcome_done: boolean
}



export type TaskStatus = 'queued' | 'running' | 'done' | 'error' | 'cancelled'

/**
 * 启动结果：壳不再自己判断"能不能跑"，门禁问 CLI（`status --json` 的 can_start）。
 * nothing-to-start = UI 自己的判断（这行本来就不该起）；cli = 被 Rust 挡下，
 * reason 是它的双语文案，壳原样显示。
 */
export interface StartResult {
  ok: boolean
  code?: 'nothing-to-start' | 'cli'
  reason?: string
}

/** oneasr-cli status --json 的一个模型槽位。 */
export interface ModelSlot {
  id: string
  dir: string
  ready: boolean
  error: string | null
}

/** status --json 的 catalog：8 条可选模型，供 UI 画清单（目录名来自 Rust，壳不再硬编码）。 */
export interface CatalogEntry {
  id: string
  kind: 'asr' | 'aligner' | 'demucs' | 'sensevoice'
  dir: string
  ready: boolean
}

/** oneasr-cli status --json 的完整形状。 */
export interface StatusSnapshot {
  version: string
  app_root: string
  data_root: string
  models_root: string
  output_dir: string
  backend: string
  language: string
  ui_language: string
  models: {
    asr: ModelSlot
    aligner: ModelSlot
    demucs: ModelSlot
    sensevoice: ModelSlot
  }
  catalog: CatalogEntry[]
  can_start: { ok: boolean; reason: string | null }
}

/** oneasr-cli probe --json 的形状。 */
export interface ProbeResult {
  input: string
  size_bytes: number
  duration_sec: number | null
  kind: 'audio' | 'video'
}

/** 下载进度（对应 CLI stdout 的机器行）。 */
export interface DownloadState {
  model: string
  state: 'downloading' | 'verifying' | 'done' | 'error'
  doneBytes: number
  totalBytes: number
  pct: number
  speed?: number
  dir?: string
  message?: string
}

export interface TaskSnapshot {
  id: string
  input: string
  name: string
  status: TaskStatus
  progress: string
  stage?: string
  progressPct?: number
  outputPath?: string
  error?: string
  startedAt?: number
  finishedAt?: number
  hotwordGroupId?: string | null
  /** 每行的识别语言（加入时快照设置，可逐行改；运行中锁定） */
  language?: string
  /** 每行人声分离（加入时快照设置，可逐行改；运行中锁定） */
  vocalSeparation?: boolean
  /** 加入时的文件大小；时长后台探，探到前是 undefined */
  sizeBytes?: number
  durationSec?: number
  /** 排队位次，1 = 下一个跑；只有排队的行有 */
  queueRank?: number
  /** 当前阶段的稳定 key（CLI 事件流给；界面文案按它出） */
  stageKey?: string
  /** 失败时的原始 stderr（详情：给「展开」与「复制错误」用；摘要在 error 里） */
  errorDetail?: string
  /** 完成后由 CLI 的 timing 事件给出的阶段耗时（label 是 CLI 的语言，key 才是稳定的） */
  stageTimings?: { key?: string; stage: string; ms: number }[]
  /** CLI 报的总耗时（含模型加载） */
  totalMs?: number
  /** 媒体类型：由 `oneasr-cli probe` 判定（壳不自己按扩展名猜） */
  mediaKind?: 'audio' | 'video'
}

/** One finished-task row from stats.jsonl (oneasr-core compatible). */
export interface StatsRecord {
  v?: number
  day: string
  media_sec?: number
  process_ms: number
  lang: string
  sep?: boolean
  transcript?: boolean
  ok: boolean
  cues?: number
}

export interface DayStat {
  day: string
  mediaSec: number
  tasks: number
  errors: number
}

export interface StatsSnapshot {
  tasksOk: number
  tasksErr: number
  tasksTotal: number
  mediaSec: number
  processMs: number
  savedSec: number | null
  avgRealtime: number | null
  cues: number
  sepTasks: number
  langs: Array<{ lang: string; count: number }>
  perDay: DayStat[]
  longestMediaSec: number | null
  fastestSpeed: number | null
  firstDay: string | null
  recentSession: Array<{ name: string; status: TaskStatus; durationMs?: number; at: number }>
  lastOutput?: string
  lastDurationMs?: number
}

export interface HotwordGroup {
  id: string
  name: string
  words: string
}

export interface HotwordStore {
  groups: HotwordGroup[]
  default_group: string | null
  default_group_enabled: boolean
  top_k: number
  threshold: number
}

export interface AppInfo {
  appRoot: string
  cliPath: string
  display: string | undefined
  version: string
}

export type ToastKind = 'info' | 'success' | 'error' | 'warn'

export interface ToastPayload {
  id: string
  kind: ToastKind
  message: string
}

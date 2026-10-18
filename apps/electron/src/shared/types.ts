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

export interface ModelStatus {
  asrReady: boolean
  alignerReady: boolean
  demucsReady: boolean
  sensevoiceReady: boolean
  asrPath: string
  alignerPath: string
  demucsPath: string
  sensevoicePath: string
  ready: boolean
  asrLabel: string
  alignerLabel: string
  backend: string
}

export type TaskStatus = 'queued' | 'running' | 'done' | 'error' | 'cancelled'

/** 启动被挡下来的原因（渲染层翻成双语文案）。 */
export type StartBlockCode =
  | 'models' // ASR / 对齐权重不就绪
  | 'sep-weights' // 开了人声分离但没有 Demucs 权重
  | 'hotwords-empty' // 这一行绑的热词组是空的
  | 'nothing-to-start' // 没有可开始的任务

export interface StartResult {
  ok: boolean
  code?: StartBlockCode
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
  /** 完成后从 CLI 收尾输出解析出的阶段耗时 */
  stageTimings?: { stage: string; ms: number }[]
  /** CLI 报的总耗时（含模型加载） */
  totalMs?: number
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

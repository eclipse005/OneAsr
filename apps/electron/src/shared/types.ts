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
  asrPath: string
  alignerPath: string
  demucsPath: string
  ready: boolean
}

export type TaskStatus = 'queued' | 'running' | 'done' | 'error' | 'cancelled'

export interface TaskSnapshot {
  id: string
  input: string
  name: string
  status: TaskStatus
  progress: string
  outputPath?: string
  error?: string
  startedAt?: number
  finishedAt?: number
}

export interface StatsSnapshot {
  totalRuns: number
  successCount: number
  errorCount: number
  lastOutput?: string
  lastDurationMs?: number
  recent: Array<{ name: string; status: TaskStatus; durationMs?: number; at: number }>
}

export interface AppInfo {
  appRoot: string
  cliPath: string
  display: string | undefined
  version: string
}

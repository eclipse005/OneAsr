/**
 * CLI 事件流（`--events=json`）的解析：**纯函数**，从 jobs.ts 里抽出来是为了能单测。
 *
 * 契约见 `oneasr-cli --help`：stdout 每行一个 JSON 对象，形如
 *   {"event":"stage","key":"transcribing","label":"转写","index":6,"total":9,"pct":56,"chunk":2,"chunkTotal":5}
 *   {"event":"timing","label":"转写","ms":587}
 *   {"event":"total","ms":12828}
 *   {"event":"done","output":"D:\\out\\x.srt"}
 *   {"event":"error","message":"..."}
 * 壳不认识的行（人读日志、第三方输出）一律忽略，绝不猜。
 */
export type CliEventName = 'stage' | 'timing' | 'total' | 'done' | 'error'

export interface CliEvent {
  event: CliEventName
  key?: string
  label?: string
  index?: number
  total?: number
  pct?: number
  /** 当前分段（从 1 数）。只有 `chunkTotal > 1` 时 CLI 才带；与 index 无关。 */
  chunk?: number
  /** 这一阶段的分段总数。阶段计划的 total 不在这里。 */
  chunkTotal?: number
  ms?: number
  output?: string
  message?: string
}

const NAMES: readonly CliEventName[] = ['stage', 'timing', 'total', 'done', 'error']

function num(value: unknown): number | undefined {
  return typeof value === 'number' && Number.isFinite(value) ? value : undefined
}

/** 解析一行；不是本协议的事件就返回 undefined（不抛错——日志里什么都可能有）。 */
export function parseCliEvent(line: string): CliEvent | undefined {
  const trimmed = line.trim()
  if (!trimmed.startsWith('{')) return undefined
  let raw: unknown
  try {
    raw = JSON.parse(trimmed)
  } catch {
    return undefined
  }
  if (typeof raw !== 'object' || raw === null) return undefined
  const obj = raw as Record<string, unknown>
  const event = obj.event
  if (typeof event !== 'string' || !NAMES.includes(event as CliEventName)) return undefined
  return {
    event: event as CliEventName,
    key: typeof obj.key === 'string' ? obj.key : undefined,
    label: typeof obj.label === 'string' ? obj.label : undefined,
    index: num(obj.index),
    total: num(obj.total),
    pct: num(obj.pct),
    chunk: num(obj.chunk),
    chunkTotal: num(obj.chunkTotal),
    ms: num(obj.ms),
    output: typeof obj.output === 'string' ? obj.output : undefined,
    message: typeof obj.message === 'string' ? obj.message : undefined
  }
}

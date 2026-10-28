import type { TaskSnapshot } from './types'

/**
 * 取消后回到「刚加入列表」的空闲态。
 *
 * Electron 用 `status: 'queued'` + 无 queueRank 表示待处理（与 GPUI 的 Pending 同语义）；
 * 不再把取消写成粘性的 `cancelled` 终态——否则行上挂着「已取消」且 startOne 拒跑。
 * queueRank 由 JobManager.list() 按运行队列现算，这里不用清。
 */
export function restorePendingFields(t: TaskSnapshot): void {
  t.status = 'queued'
  t.progress = ''
  t.stage = undefined
  t.stageKey = undefined
  t.stageChunk = undefined
  t.stageChunkTotal = undefined
  t.progressPct = 0
  t.error = undefined
  t.errorDetail = undefined
  t.startedAt = undefined
  t.finishedAt = undefined
  t.outputPath = undefined
  t.stageTimings = undefined
  t.totalMs = undefined
}

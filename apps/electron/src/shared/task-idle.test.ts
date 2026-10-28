import { describe, expect, it } from 'vitest'
import { isBulkStartCandidate } from './start-policy'
import { restorePendingFields } from './task-idle'
import type { TaskSnapshot } from './types'

function task(partial: Partial<TaskSnapshot>): TaskSnapshot {
  return {
    id: 't1',
    input: '/a.mp4',
    name: 'a.mp4',
    status: 'queued',
    progress: '',
    ...partial
  }
}

describe('restorePendingFields', () => {
  it('排队中取消：回到待处理，可被全部开始 / 单行开始挑中', () => {
    const t = task({
      status: 'queued',
      progress: 'queued',
      queueRank: 1,
      stage: 'x',
      stageKey: 'asr',
      progressPct: 10,
      startedAt: 1,
      finishedAt: 2
    })
    restorePendingFields(t)
    expect(t.status).toBe('queued')
    expect(t.progress).toBe('')
    expect(t.stage).toBeUndefined()
    expect(t.stageKey).toBeUndefined()
    expect(t.progressPct).toBe(0)
    expect(t.startedAt).toBeUndefined()
    expect(t.finishedAt).toBeUndefined()
    // list() 只在 status=queued 且真在运行队列里才填 queueRank；字段本身无害
    expect(isBulkStartCandidate(t.status, false)).toBe(true)
  })

  it('运行中取消：清掉阶段进度，不再是 cancelled 终态', () => {
    const t = task({
      status: 'running',
      progress: 'running',
      stage: '打轴',
      stageKey: 'align',
      stageChunk: 2,
      stageChunkTotal: 5,
      progressPct: 40,
      startedAt: 100,
      finishedAt: 200,
      error: 'transient'
    })
    restorePendingFields(t)
    expect(t.status).toBe('queued')
    expect(t.stageChunk).toBeUndefined()
    expect(t.error).toBeUndefined()
    expect(isBulkStartCandidate(t.status, false)).toBe(true)
    expect(isBulkStartCandidate('cancelled', false)).toBe(false)
  })
})

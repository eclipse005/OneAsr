import { describe, expect, it } from 'vitest'
import { isBulkStartCandidate } from './start-policy'

describe('isBulkStartCandidate', () => {
  it('待处理和失败可以批量开始', () => {
    expect(isBulkStartCandidate('queued', false)).toBe(true)
    expect(isBulkStartCandidate('error', false)).toBe(true)
    // 失败行即使还挂着队列名次，也要能再开始；名次检查只约束待处理
    expect(isBulkStartCandidate('error', true)).toBe(true)
  })

  it('完成、取消、运行中、已经在排队的不进批量开始', () => {
    expect(isBulkStartCandidate('done', false)).toBe(false)
    expect(isBulkStartCandidate('cancelled', false)).toBe(false)
    expect(isBulkStartCandidate('running', false)).toBe(false)
    expect(isBulkStartCandidate('running', true)).toBe(false)
    expect(isBulkStartCandidate('queued', true)).toBe(false)
    expect(isBulkStartCandidate('done', true)).toBe(false)
  })
})

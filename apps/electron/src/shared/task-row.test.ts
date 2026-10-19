import { describe, expect, it } from 'vitest'
import { rowChipState, rowStartIcon, rowStartKind } from './task-row'

describe('rowChipState', () => {
  it('打轴时语言和热词都还在，热词禁用，语言仍可用', () => {
    expect(rowChipState(true)).toEqual({
      showLanguage: true,
      showHotwords: true,
      languageEnabled: true,
      hotwordsEnabled: false
    })
  })

  it('普通转写时语言和热词都能改', () => {
    expect(rowChipState(false)).toEqual({
      showLanguage: true,
      showHotwords: true,
      languageEnabled: true,
      hotwordsEnabled: true
    })
  })
})

describe('rowStartKind', () => {
  const base = {
    hasTranscript: false,
    status: 'queued' as const,
    hasOutput: false,
    needsRealign: false,
    timedOutputEnabled: true
  }

  it('开始键按任务自己的流程走，不另开一条打轴按钮', () => {
    expect(rowStartKind(base)).toBe('start')
    expect(rowStartKind({ ...base, hasTranscript: true })).toBe('align')
    expect(rowStartKind({ ...base, hasTranscript: true, needsRealign: true })).toBe('realign')
    expect(
      rowStartKind({ ...base, hasTranscript: true, status: 'done', hasOutput: true })
    ).toBe('realign')
    expect(rowStartKind({ ...base, hasTranscript: true, status: 'error' })).toBe('retry-align')
    expect(rowStartKind({ ...base, status: 'error' })).toBe('retry')
    expect(rowStartKind({ ...base, status: 'done', hasOutput: true })).toBe('retranscribe')
    expect(rowStartKind({ ...base, hasTranscript: true, timedOutputEnabled: false })).toBe(
      'blocked'
    )
  })
})

describe('rowStartIcon', () => {
  it('「再来一次」这一族用圆圈箭头（重新转录/重试/重试打轴/重新打轴）', () => {
    expect(rowStartIcon('retranscribe')).toBe('retry')
    expect(rowStartIcon('retry')).toBe('retry')
    expect(rowStartIcon('retry-align')).toBe('retry')
    expect(rowStartIcon('realign')).toBe('retry')
  })

  it('「从零开始」这一族用播放三角（开始/开始打轴/被禁用）', () => {
    expect(rowStartIcon('start')).toBe('play')
    expect(rowStartIcon('align')).toBe('play')
    expect(rowStartIcon('blocked')).toBe('play')
  })

  it('断骨锚点：rowStartIcon 一律返回 play 时，上面两条全红', () => {
    // 三角形是「播放」的意思，套在「重新转录」上会让人以为只是再播一遍
  })
})

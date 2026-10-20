import { describe, expect, it } from 'vitest'
import { hotwordGroupLabel, rowChipState, rowStartIcon, rowStartKind } from './task-row'

describe('rowChipState', () => {
  it('打轴时语言和热词都还在，热词禁用，语言仍可用', () => {
    expect(rowChipState(true, 'zh')).toEqual({
      showLanguage: true,
      showHotwords: true,
      languageEnabled: true,
      hotwordsEnabled: false,
      hotwordsDisabledReason: 'align'
    })
  })

  it('普通转写时中文和英文都能改热词', () => {
    for (const language of ['zh', 'en']) {
      expect(rowChipState(false, language)).toEqual({
        showLanguage: true,
        showHotwords: true,
        languageEnabled: true,
        hotwordsEnabled: true,
        hotwordsDisabledReason: null
      })
    }
  })

  it('其它语言热词禁用：这一栏固定为「无」，不能选组', () => {
    for (const language of ['yue', 'ja', 'ko', 'auto']) {
      const chips = rowChipState(false, language)
      expect(chips.hotwordsEnabled, language).toBe(false)
      expect(chips.hotwordsDisabledReason, language).toBe('language')
    }
  })

  it('打轴和不支持的语言同时成立时，按打轴解释', () => {
    expect(rowChipState(true, 'ja').hotwordsDisabledReason).toBe('align')
  })
})

describe('hotwordGroupLabel', () => {
  it('没起名的组用本地化占位名，不渲染空白胶囊', () => {
    expect(hotwordGroupLabel('', '未命名')).toBe('未命名')
    expect(hotwordGroupLabel('   ', '未命名')).toBe('未命名')
    expect(hotwordGroupLabel(undefined, 'Untitled')).toBe('Untitled')
  })

  it('起过名的组原样显示（两侧空白不算名字）', () => {
    expect(hotwordGroupLabel('ICT', '未命名')).toBe('ICT')
    expect(hotwordGroupLabel('  ICT  ', '未命名')).toBe('ICT')
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

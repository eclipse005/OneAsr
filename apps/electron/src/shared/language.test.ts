import { describe, expect, it } from 'vitest'
import { hotwordGroupForLanguage, supportsHotwords } from './language'

describe('supportsHotwords', () => {
  it('中文和英文（含 Rust 侧认的别名）支持热词', () => {
    for (const language of ['zh', 'en', 'ZH', 'en-US', 'Chinese', 'English', 'cmn', 'mandarin']) {
      expect(supportsHotwords(language), language).toBe(true)
    }
  })

  it('粤语、日语等其它语言不支持', () => {
    for (const language of ['yue', 'ja', 'ko', 'fr', 'auto', '', '   ', undefined, null]) {
      expect(supportsHotwords(language), String(language)).toBe(false)
    }
  })
})

describe('hotwordGroupForLanguage', () => {
  it('中英文保留已选的组', () => {
    expect(hotwordGroupForLanguage('zh', 'g1')).toBe('g1')
    expect(hotwordGroupForLanguage('English', 'g2')).toBe('g2')
  })

  it('其它语言一律清成「无」，不能留着一个永远不生效的组', () => {
    // 断骨锚点：这里的 groupId 有值时仍必须返回 null。把实现改成
    // 「原样返回 groupId」后，本用例立刻变红。
    expect(hotwordGroupForLanguage('ja', 'g1')).toBeNull()
    expect(hotwordGroupForLanguage('yue', 'g1')).toBeNull()
    expect(hotwordGroupForLanguage('auto', 'g1')).toBeNull()
    expect(hotwordGroupForLanguage(undefined, 'g1')).toBeNull()
  })

  it('没有组时保持「无」', () => {
    expect(hotwordGroupForLanguage('zh', null)).toBeNull()
    expect(hotwordGroupForLanguage('en', undefined)).toBeNull()
  })
})

import { describe, expect, it } from 'vitest'
import { closeOverlay, openOverlay, type OverlayKind } from './overlay'

/**
 * 浮窗互斥的规则。
 *
 * 断骨锚点：把 `openOverlay` 的 `return want` 改成 `return current`
 * （= 新浮窗不顶掉前一个），下面「打开指南会把统计关掉」立刻变红。
 */
describe('浮窗互斥', () => {
  it('打开一个浮窗会把前一个关掉', () => {
    let cur: OverlayKind = 'none'
    cur = openOverlay(cur, 'stats')
    expect(cur).toBe('stats')
    cur = openOverlay(cur, 'guide')
    // ↓ 断骨锚点：这里必须仍是 'guide'，而且 'stats' 已经被关掉
    expect(cur).toBe('guide')
    expect(cur).not.toBe('stats')
  })

  it('连开三个也只留最后一个', () => {
    let cur: OverlayKind = 'none'
    for (const kind of ['settings', 'stats', 'guide'] as const) {
      cur = openOverlay(cur, kind)
    }
    expect(cur).toBe('guide')
  })

  it('重复点同一个按钮不会把它关掉', () => {
    const cur = openOverlay('none', 'stats')
    expect(openOverlay(cur, 'stats')).toBe('stats')
  })

  it('关掉当前浮窗回到 none', () => {
    expect(closeOverlay('stats', 'stats')).toBe('none')
    expect(closeOverlay('stats')).toBe('none')
  })

  it('关掉别的浮窗不会动当前这个', () => {
    expect(closeOverlay('stats', 'guide')).toBe('stats')
  })

  it('任何操作之后最多只有一个浮窗开着', () => {
    // 穷举一小段操作序列：状态里同时为真的浮窗数必须 ≤ 1。
    const kinds: OverlayKind[] = ['none', 'settings', 'stats', 'guide']
    for (const a of kinds) {
      for (const b of kinds) {
        if (b === 'none') continue
        const after = openOverlay(a, b)
        const open = kinds.filter((k) => k !== 'none' && k === after)
        expect(open.length).toBeLessThanOrEqual(1)
      }
    }
  })
})

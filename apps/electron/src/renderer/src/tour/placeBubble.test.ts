import { describe, expect, it, beforeEach, afterEach } from 'vitest'
import { placeBubble } from './place'

function fakeRect(
  partial: { top: number; left: number; width: number; height: number }
): DOMRect {
  const { top, left, width, height } = partial
  return {
    top,
    left,
    width,
    height,
    bottom: top + height,
    right: left + width,
    x: left,
    y: top,
    toJSON() {
      return this
    }
  }
}

describe('placeBubble', () => {
  beforeEach(() => {
    Object.defineProperty(globalThis, 'window', {
      configurable: true,
      value: { innerWidth: 1280, innerHeight: 800 }
    })
  })
  afterEach(() => {
    // eslint-disable-next-line @typescript-eslint/no-dynamic-delete
    delete (globalThis as { window?: unknown }).window
  })

  it('状态栏在底部时气泡在上方且不覆盖目标', () => {
    const target = fakeRect({ top: 760, left: 24, width: 160, height: 28 })
    const pos = placeBubble(target, 'top', 320, 160)
    expect(pos.side).toBe('top')
    expect(pos.top + 160).toBeLessThanOrEqual(target.top - 4)
  })

  it('首选侧盖住目标时翻到对侧', () => {
    const target = fakeRect({ top: 200, left: 8, width: 120, height: 40 })
    const pos = placeBubble(target, 'left', 320, 160)
    expect(pos.side).not.toBe('left')
    const overlaps =
      pos.left < target.right + 4 &&
      pos.left + 320 > target.left - 4 &&
      pos.top < target.bottom + 4 &&
      pos.top + 160 > target.top - 4
    expect(overlaps).toBe(false)
  })

  it('auto 优先视口对侧（目标在下半 → top）', () => {
    const target = fakeRect({ top: 600, left: 500, width: 80, height: 32 })
    const pos = placeBubble(target, 'auto', 320, 140)
    expect(pos.side).toBe('top')
  })

  it('贴边时保留更大边距，减少盖住面板顶栏', () => {
    const target = fakeRect({ top: 80, left: 400, width: 480, height: 520 })
    const pos = placeBubble(target, 'left', 320, 180)
    expect(pos.left).toBeGreaterThanOrEqual(20)
    expect(pos.top).toBeGreaterThanOrEqual(20)
  })
})

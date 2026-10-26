import { describe, expect, it, vi } from 'vitest'
import {
  attachTourTargetClickListener,
  matchesTourTarget
} from './tourTargetClick'

function fakeTarget(matchSelector: string | null): EventTarget {
  return {
    closest: (sel: string) => (sel === matchSelector ? {} : null)
  } as unknown as EventTarget
}

describe('matchesTourTarget', () => {
  it('匹配 closest 命中的节点', () => {
    const hit = fakeTarget('[data-tour="settings-tab-hotwords"]')
    expect(matchesTourTarget(hit, '[data-tour="settings-tab-hotwords"]')).toBe(
      true
    )
    expect(matchesTourTarget(hit, '[data-tour="settings-tab-output"]')).toBe(
      false
    )
    expect(matchesTourTarget(null, '[data-tour="settings-tab-hotwords"]')).toBe(
      false
    )
    expect(
      matchesTourTarget({} as EventTarget, '[data-tour="settings-tab-hotwords"]')
    ).toBe(false)
  })
})

describe('attachTourTargetClickListener', () => {
  it('以 capture:true 注册；祖先冒泡 stopPropagation 场景仍 defer 推进', () => {
    const listeners: Array<{
      type: string
      fn: EventListener
      capture: boolean
    }> = []
    const root = {
      addEventListener: (
        type: string,
        listener: EventListenerOrEventListenerObject,
        options?: boolean | AddEventListenerOptions
      ) => {
        listeners.push({
          type,
          fn: listener as EventListener,
          capture: options === true || (typeof options === 'object' && !!options.capture)
        })
      },
      removeEventListener: (
        type: string,
        listener: EventListenerOrEventListenerObject,
        options?: boolean | EventListenerOptions
      ) => {
        const capture =
          options === true || (typeof options === 'object' && !!options.capture)
        const i = listeners.findIndex(
          (l) => l.type === type && l.fn === listener && l.capture === capture
        )
        if (i >= 0) listeners.splice(i, 1)
      }
    }

    const advances: number[] = []
    const pending: Array<() => void> = []
    const unsub = attachTourTargetClickListener({
      selector: '[data-tour="settings-tab-hotwords"]',
      onTargetClick: () => advances.push(1),
      root,
      defer: (cb) => {
        pending.push(cb)
        return () => {
          const i = pending.indexOf(cb)
          if (i >= 0) pending.splice(i, 1)
        }
      }
    })

    expect(listeners).toHaveLength(1)
    expect(listeners[0]!.type).toBe('click')
    expect(listeners[0]!.capture).toBe(true)

    // 模拟 capture 阶段：事件目标是热词标签（即使之后冒泡被 shell 拦住）
    listeners[0]!.fn(
      {
        target: fakeTarget('[data-tour="settings-tab-hotwords"]')
      } as unknown as Event
    )
    expect(advances).toEqual([])
    expect(pending).toHaveLength(1)
    pending[0]!()
    expect(advances).toEqual([1])

    // 点到别处不推进
    pending.length = 0
    listeners[0]!.fn({
      target: fakeTarget('[data-tour="settings-tab-output"]')
    } as unknown as Event)
    expect(pending).toHaveLength(0)
    expect(advances).toEqual([1])

    unsub()
    expect(listeners).toHaveLength(0)
  })

  it('卸载时取消尚未 defer 的 advance，避免双进', () => {
    const listeners: Array<{ fn: EventListener }> = []
    const root = {
      addEventListener: (
        _type: string,
        listener: EventListenerOrEventListenerObject
      ) => {
        listeners.push({ fn: listener as EventListener })
      },
      removeEventListener: () => {
        listeners.length = 0
      }
    }
    const onTargetClick = vi.fn()
    const pending: Array<() => void> = []
    const unsub = attachTourTargetClickListener({
      selector: '[data-tour="settings-tab-output"]',
      onTargetClick,
      root,
      defer: (cb) => {
        pending.push(cb)
        return () => {
          const i = pending.indexOf(cb)
          if (i >= 0) pending.splice(i, 1)
        }
      }
    })

    listeners[0]!.fn({
      target: fakeTarget('[data-tour="settings-tab-output"]')
    } as unknown as Event)
    expect(pending).toHaveLength(1)
    unsub()
    expect(pending).toHaveLength(0)
    expect(onTargetClick).not.toHaveBeenCalled()
  })

  it('热词与输出标签选择器均可命中（与 steps 一致）', () => {
    for (const id of ['settings-tab-hotwords', 'settings-tab-output'] as const) {
      const sel = `[data-tour="${id}"]`
      expect(matchesTourTarget(fakeTarget(sel), sel)).toBe(true)
    }
  })
})

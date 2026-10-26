import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import {
  markTourCompleted,
  markTourSkipped,
  readTourPrefs,
  shouldShowWelcome,
  TOUR_STORAGE_KEY,
  writeTourPrefs
} from './storage'

const mem = new Map<string, string>()

beforeEach(() => {
  mem.clear()
  Object.defineProperty(globalThis, 'localStorage', {
    configurable: true,
    value: {
      getItem: (k: string) => (mem.has(k) ? mem.get(k)! : null),
      setItem: (k: string, v: string) => {
        mem.set(k, v)
      },
      removeItem: (k: string) => {
        mem.delete(k)
      }
    }
  })
})

afterEach(() => {
  mem.clear()
})

describe('tour storage', () => {
  it('默认未完成未跳过 → 显示欢迎', () => {
    expect(readTourPrefs()).toEqual({ completed: false, skipped: false })
    expect(shouldShowWelcome()).toBe(true)
  })

  it('完成后不再欢迎', () => {
    markTourCompleted()
    expect(shouldShowWelcome()).toBe(false)
    expect(readTourPrefs().completed).toBe(true)
  })

  it('跳过后不再欢迎', () => {
    markTourSkipped()
    expect(shouldShowWelcome()).toBe(false)
    expect(readTourPrefs().skipped).toBe(true)
  })

  it('坏 JSON 当默认', () => {
    localStorage.setItem(TOUR_STORAGE_KEY, '{not-json')
    expect(readTourPrefs()).toEqual({ completed: false, skipped: false })
  })

  it('写入可读回', () => {
    writeTourPrefs({ completed: true, skipped: false })
    expect(JSON.parse(localStorage.getItem(TOUR_STORAGE_KEY)!)).toEqual({
      completed: true,
      skipped: false
    })
  })
})

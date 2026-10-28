import { describe, expect, it } from 'vitest'
import { bumpStartAttempt, isStartAttemptCurrent } from './start-attempt'

describe('start attempt epoch', () => {
  it('取消会 bump，门禁返回后原 attempt 作废', () => {
    const epochs = new Map<string, number>()
    const attempt = bumpStartAttempt(epochs, 'a')
    expect(isStartAttemptCurrent(epochs, 'a', attempt)).toBe(true)
    bumpStartAttempt(epochs, 'a') // cancel during gate
    expect(isStartAttemptCurrent(epochs, 'a', attempt)).toBe(false)
  })

  it('不同行互不影响', () => {
    const epochs = new Map<string, number>()
    const a = bumpStartAttempt(epochs, 'a')
    bumpStartAttempt(epochs, 'b')
    expect(isStartAttemptCurrent(epochs, 'a', a)).toBe(true)
  })
})

import { describe, expect, it } from 'vitest'
import { createLimit } from './limit'

const tick = (): Promise<void> => new Promise((resolve) => setTimeout(resolve, 0))

describe('createLimit', () => {
  it('最多同时跑 limit 个，其余按顺序排队', async () => {
    const run = createLimit(2)
    let active = 0
    let peak = 0
    const gates: Array<() => void> = []

    const jobs = [0, 1, 2, 3].map((i) =>
      run(async () => {
        active += 1
        peak = Math.max(peak, active)
        await new Promise<void>((resolve) => gates.push(resolve))
        active -= 1
        return i
      })
    )

    await tick()
    expect(peak).toBe(2)
    gates.shift()?.()
    await tick()
    expect(peak).toBe(2)
    gates.shift()?.()
    await tick()
    expect(peak).toBe(2)

    while (gates.length) {
      gates.shift()?.()
      await tick()
    }
    expect(await Promise.all(jobs)).toEqual([0, 1, 2, 3])
  })

  it('任务失败也会让出位置', async () => {
    const run = createLimit(1)
    await expect(run(async () => { throw new Error('boom') })).rejects.toThrow('boom')
    await expect(run(async () => 42)).resolves.toBe(42)
  })

  it('limit 非正整数直接拒绝（配置错误别静默变成无限并发）', () => {
    expect(() => createLimit(0)).toThrow()
    expect(() => createLimit(-1)).toThrow()
  })
})

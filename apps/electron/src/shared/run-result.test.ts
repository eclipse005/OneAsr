import { describe, expect, it } from 'vitest'
import { resolveRunOutcome } from './run-result'

describe('resolveRunOutcome', () => {
  it('退出码 0 且 CLI 报了产物路径：成功，路径原样用', () => {
    const r = resolveRunOutcome(0, 'D:\\out\\a.srt')
    expect(r).toEqual({ ok: true, outputPath: 'D:\\out\\a.srt' })
  })

  it('退出码非 0：失败，不管有没有产物路径', () => {
    expect(resolveRunOutcome(1, 'D:\\out\\a.srt')).toEqual({ ok: false, reason: 'failed' })
    expect(resolveRunOutcome(1)).toEqual({ ok: false, reason: 'failed' })
    // 被信号打断（退出码拿不到）也不算成功。
    expect(resolveRunOutcome(null)).toEqual({ ok: false, reason: 'failed' })
  })

  it('退出码 0 却没报产物路径：协议被破坏，报失败而不是假绿', () => {
    // 断骨锚点：把 resolveRunOutcome 里的 `!output` 判断去掉（直接返回成功），
    // 下面两条断言立刻变红 —— 这正是「壳去猜文件名」的老做法。
    expect(resolveRunOutcome(0)).toEqual({ ok: false, reason: 'no-output' })
    expect(resolveRunOutcome(0, '   ')).toEqual({ ok: false, reason: 'no-output' })
  })
})

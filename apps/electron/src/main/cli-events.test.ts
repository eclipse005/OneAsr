import { describe, expect, it } from 'vitest'
import { parseCliEvent } from './cli-events'

describe('parseCliEvent', () => {
  it('认得完整 stage 事件（含条件阶段的编号与百分比）', () => {
    const ev = parseCliEvent(
      '{"event":"stage","key":"transcribing","label":"转写","index":6,"total":9,"pct":56}'
    )
    expect(ev).toEqual({
      event: 'stage',
      key: 'transcribing',
      label: '转写',
      index: 6,
      total: 9,
      pct: 56,
      ms: undefined,
      output: undefined,
      message: undefined
    })
  })

  it('timing / total / done / error 都认', () => {
    expect(parseCliEvent('{"event":"timing","label":"打轴","ms":257}')?.ms).toBe(257)
    expect(parseCliEvent('{"event":"total","ms":12828}')?.ms).toBe(12828)
    expect(parseCliEvent('{"event":"done","output":"D:\\\\o\\\\x.srt"}')?.output).toBe(
      'D:\\o\\x.srt'
    )
    expect(parseCliEvent('{"event":"error","message":"ffmpeg failed"}')?.message).toBe(
      'ffmpeg failed'
    )
  })

  it('人读日志、空行、坏 JSON 一律忽略（不抛错——日志里什么都可能有）', () => {
    expect(parseCliEvent('[阶段] 转写')).toBeUndefined()
    expect(parseCliEvent('')).toBeUndefined()
    expect(parseCliEvent('{ not json')).toBeUndefined()
    expect(parseCliEvent('{"event":"unknown"}')).toBeUndefined()
    expect(parseCliEvent('[1,2,3]')).toBeUndefined()
  })

  it('字段类型不对就当没有（不把 "56" 当数字、不把 null 当路径）', () => {
    const ev = parseCliEvent('{"event":"stage","pct":"56","label":null,"index":null}')
    expect(ev?.pct).toBeUndefined()
    expect(ev?.label).toBeUndefined()
    expect(ev?.index).toBeUndefined()
  })
})

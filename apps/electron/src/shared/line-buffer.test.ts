import { describe, expect, it } from 'vitest'
import { LineBuffer } from './line-buffer'

describe('LineBuffer', () => {
  it('一行被拆成两个 chunk 也能拼回来', () => {
    const buf = new LineBuffer()
    const lines: string[] = []
    buf.push('{"event":"do', (l) => lines.push(l))
    expect(lines).toEqual([])
    buf.push('ne","output":"x.srt"}\n', (l) => lines.push(l))
    expect(lines).toEqual(['{"event":"done","output":"x.srt"}'])
  })

  it('支持 CRLF、多行，并忽略空行', () => {
    const buf = new LineBuffer()
    const lines: string[] = []
    buf.push('a\r\n\r\nb\nc', (l) => lines.push(l))
    expect(lines).toEqual(['a', 'b'])
    buf.push('\n', (l) => lines.push(l))
    expect(lines).toEqual(['a', 'b', 'c'])
  })

  it('flush 交出没有换行结尾的最后一行', () => {
    const buf = new LineBuffer()
    const lines: string[] = []
    buf.push('tail', (l) => lines.push(l))
    buf.flush((l) => lines.push(l))
    expect(lines).toEqual(['tail'])
    // 已经交出去的内容不能重复。
    buf.flush((l) => lines.push(l))
    expect(lines).toEqual(['tail'])
  })
})

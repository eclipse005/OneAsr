import { describe, expect, it } from 'vitest'
import { transcriptLineMarks, transcriptRows } from './transcript-lines'

describe('transcriptLineMarks', () => {
  it('每条字幕一个号，按出现顺序', () => {
    const marks = transcriptLineMarks('第一行。\n第二行。\n第三行。')
    expect(marks).toEqual([
      { row: 0, no: 1 },
      { row: 1, no: 2 },
      { row: 2, no: 3 }
    ])
  })

  it('空行不是字幕：跳过但保留它占的那一行', () => {
    const marks = transcriptLineMarks('第一行。\n\n第二行。')
    // 中间那行是空的 → 不是字幕；第二行是第 2 条字幕
    expect(marks).toEqual([
      { row: 0, no: 1 },
      { row: 2, no: 2 }
    ])
  })

  it('只有空白的行也不算字幕（与 core 的 text_stats 同口径）', () => {
    expect(transcriptLineMarks('第一行。\n   \n\t\n第二行。')).toEqual([
      { row: 0, no: 1 },
      { row: 3, no: 2 }
    ])
  })

  it('空稿没有号', () => {
    expect(transcriptLineMarks('')).toEqual([])
    expect(transcriptLineMarks('\n\n')).toEqual([])
  })

  it('物理行保留空行：编辑器里它们确实占一行', () => {
    expect(transcriptRows('a\n\nb')).toEqual(['a', '', 'b'])
    // 末尾有换行时，最后一段是空的（编辑器里那也是一个空行）
    expect(transcriptRows('a\n')).toEqual(['a', ''])
  })

  it('号数与 core 的行数口径一致：非空行数', () => {
    const text = '一。\n\n二。\n三。\n\n\n四。'
    const marks = transcriptLineMarks(text)
    const nonEmpty = transcriptRows(text).filter((r) => r.trim()).length
    expect(marks.length).toBe(nonEmpty)
    expect(marks[marks.length - 1].no).toBe(nonEmpty)
  })
})

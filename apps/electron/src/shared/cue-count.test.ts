import { describe, expect, it } from 'vitest'
import { countCues } from './cue-count'

describe('countCues', () => {
  it('SRT 数箭头行', () => {
    const srt = '1\n00:00:00,000 --> 00:00:01,000\nhello\n\n2\n00:00:01,000 --> 00:00:02,000\nworld\n'
    expect(countCues('a.srt', srt)).toBe(2)
  })

  it('ASS 数 Dialogue 行，不把样式/注释算进去', () => {
    const ass = '[Events]\nFormat: Layer, Start, End, Style, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,hi\nComment: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,x\nDialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,there\n'
    expect(countCues('a.ass', ass)).toBe(2)
  })

  it('TXT 数非空行', () => {
    expect(countCues('a.txt', 'one\n\n two \n')).toBe(2)
  })
})

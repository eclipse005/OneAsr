import { describe, expect, it } from 'vitest'
import {
  isCliUnreachable,
  statusCapabilityFromOutput,
  transcriptCapabilityFromOutput
} from './cli'

describe('CLI capability classifiers', () => {
  it('ENOENT / spawn 失败算不可达，不算过旧', () => {
    expect(isCliUnreachable(-1, 'Error: spawn /no/oneasr-cli ENOENT')).toBe(true)
    expect(isCliUnreachable(0, '')).toBe(false)
    expect(isCliUnreachable(2, 'unknown command: status')).toBe(false)
  })

  it('transcript inspect 缺 --input 算有能力', () => {
    expect(transcriptCapabilityFromOutput('', 'missing required --input')).toBe(true)
  })

  it('help 里的 transcript_exts / --transcript 不算有能力', () => {
    const help = `
      transcript_exts: ["txt", "srt"]
      --transcript             attach a transcript
      transcript 选项：
    `
    expect(transcriptCapabilityFromOutput(help, '')).toBe(false)
  })

  it('help 用法行写 transcript inspect 算有能力', () => {
    expect(
      transcriptCapabilityFromOutput(
        'oneasr-cli transcript inspect --input <transcript> [--json]',
        ''
      )
    ).toBe(true)
  })

  it('unknown command: transcript 算无能力', () => {
    expect(transcriptCapabilityFromOutput('', 'unknown command: transcript')).toBe(false)
  })

  it('status --json 成功或 JSON 开头算有能力', () => {
    expect(statusCapabilityFromOutput(0, '{"can_start":{"ok":true}}', '')).toBe(true)
    expect(statusCapabilityFromOutput(1, '{"can_start":{"ok":false}}', '')).toBe(true)
    expect(statusCapabilityFromOutput(2, '', 'unknown command: status')).toBe(false)
    expect(statusCapabilityFromOutput(-1, '', 'Error: spawn ENOENT')).toBe(false)
  })
})

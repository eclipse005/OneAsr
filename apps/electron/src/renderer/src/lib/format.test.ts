import { describe, expect, it } from 'vitest'
import { dirName, formatSize, gib } from './format'

describe('formatSize', () => {
  it('未探测到（undefined / 0 / 负数）时不给数字', () => {
    expect(formatSize(undefined)).toBeUndefined()
    expect(formatSize(0)).toBeUndefined()
    expect(formatSize(-5)).toBeUndefined()
    expect(formatSize(Number.NaN)).toBeUndefined()
  })

  it('不到 1 KB 也显示 1 KB（别出现 0 KB 的假信息）', () => {
    expect(formatSize(1)).toBe('1 KB')
    expect(formatSize(512)).toBe('1 KB')
  })

  it('按 GB / MB / KB 自适应并保留约定小数位', () => {
    expect(formatSize(403 * 1024)).toBe('403 KB')
    expect(formatSize(1024 ** 2 * 1.5)).toBe('1.5 MB')
    expect(formatSize(1024 ** 3 * 2)).toBe('2.00 GB')
  })
})

describe('gib', () => {
  it('0 / 缺省都回 0 GB，不吐 NaN', () => {
    expect(gib(undefined)).toBe('0 GB')
    expect(gib(0)).toBe('0 GB')
  })

  it('两位小数', () => {
    expect(gib(937031977)).toBe('0.87 GB')
  })
})

describe('dirName', () => {
  it('Windows 与 POSIX 两种分隔符都取末段', () => {
    expect(dirName('D:\\OneAsr\\models\\Qwen3-ASR-0.6B-hf')).toBe('Qwen3-ASR-0.6B-hf')
    expect(dirName('/workspace/oneasr-runtime/models/htdemucs_ft')).toBe('htdemucs_ft')
  })

  it('缺省给空串，不抛', () => {
    expect(dirName(undefined)).toBe('')
    expect(dirName('')).toBe('')
  })
})

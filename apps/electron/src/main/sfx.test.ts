import { describe, expect, it, beforeEach } from 'vitest'
import fs from 'fs'
import path from 'path'
import {
  _resetSfxForTests,
  play,
  playClick,
  setSoundEnabled,
  wavDurationMs,
  bindSfxWindow
} from './sfx'

/** 最小 16-bit mono PCM WAV：`frames` 帧、`rate` Hz。 */
function wav(frames: number, rate: number): Buffer {
  const blockAlign = 2
  const byteRate = rate * blockAlign
  const dataLen = frames * blockAlign
  const buf = Buffer.alloc(44 + dataLen)
  buf.write('RIFF', 0)
  buf.writeUInt32LE(36 + dataLen, 4)
  buf.write('WAVE', 8)
  buf.write('fmt ', 12)
  buf.writeUInt32LE(16, 16)
  buf.writeUInt16LE(1, 20) // PCM
  buf.writeUInt16LE(1, 22) // mono
  buf.writeUInt32LE(rate, 24)
  buf.writeUInt32LE(byteRate, 28)
  buf.writeUInt16LE(blockAlign, 32)
  buf.writeUInt16LE(16, 34)
  buf.write('data', 36)
  buf.writeUInt32LE(dataLen, 40)
  return buf
}

describe('wavDurationMs', () => {
  it('读出合成 WAV 的时长', () => {
    // 44100 Hz · 4410 帧 = 100 ms
    expect(wavDurationMs(wav(4410, 44100))).toBe(100)
  })

  it('非 WAVE / 太短 → 0', () => {
    expect(wavDurationMs(Buffer.from('not a wav'))).toBe(0)
    expect(wavDurationMs(Buffer.alloc(8))).toBe(0)
  })

  it('仓库 reminder.wav 可读出约 3.7 s（字节不得被改）', () => {
    const p = path.resolve(__dirname, '../../../../assets/sounds/reminder.wav')
    const bytes = fs.readFileSync(p)
    expect(bytes.length).toBe(661294)
    const ms = wavDurationMs(bytes)
    expect(ms).toBeGreaterThan(3700)
    expect(ms).toBeLessThan(3800)
  })
})

describe('play mute-until', () => {
  beforeEach(() => _resetSfxForTests())

  it('sound 关时不发 IPC', () => {
    const sent: string[] = []
    bindSfxWindow(() => {
      return {
        isDestroyed: () => false,
        webContents: { send: (_ch: string, kind: string) => sent.push(kind) }
      } as never
    })
    setSoundEnabled(false)
    playClick()
    play('reminder')
    expect(sent).toEqual([])
  })

  it('reminder 占通道期间 click 被吞', () => {
    const sent: string[] = []
    bindSfxWindow(() => {
      return {
        isDestroyed: () => false,
        webContents: { send: (_ch: string, kind: string) => sent.push(kind) }
      } as never
    })
    // 没有真实 WAV 时 duration=0，仍有 +150ms 尾；直接用 play reminder 再立刻 click
    play('reminder')
    playClick()
    expect(sent).toEqual(['reminder'])
  })
})

import fs from 'fs'
import os from 'os'
import path from 'path'
import { describe, expect, it } from 'vitest'
import { writeFileAtomicSync } from './atomic'

describe('writeFileAtomicSync', () => {
  it('覆盖已有文件，且不留下临时文件', () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'oneasr-atomic-'))
    const file = path.join(dir, 'settings.json')
    try {
      writeFileAtomicSync(file, '{"v":1}')
      writeFileAtomicSync(file, '{"v":2}')
      expect(fs.readFileSync(file, 'utf8')).toBe('{"v":2}')
      expect(fs.readdirSync(dir)).toEqual(['settings.json'])
    } finally {
      fs.rmSync(dir, { recursive: true, force: true })
    }
  })

  it('目录不存在时会建出来', () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'oneasr-atomic-nested-'))
    const file = path.join(dir, 'a', 'b.json')
    try {
      writeFileAtomicSync(file, '{}')
      expect(fs.readFileSync(file, 'utf8')).toBe('{}')
    } finally {
      fs.rmSync(dir, { recursive: true, force: true })
    }
  })
})

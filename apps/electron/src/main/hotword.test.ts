import fs from 'fs'
import os from 'os'
import path from 'path'
import { describe, expect, it } from 'vitest'
import { hotwordPath, loadHotwords, saveHotwords } from './hotword'

describe('hotword store', () => {
  it('读取缺失的 hotword.json 只返回默认库，不在读路径上落盘', () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'oneasr-hw-'))
    try {
      const store = loadHotwords(dir)
      expect(store.groups.length).toBe(1)
      expect(fs.existsSync(hotwordPath(dir))).toBe(false)
    } finally {
      fs.rmSync(dir, { recursive: true, force: true })
    }
  })

  it('保存会写盘并可再次读回（原子写不依赖目标已存在）', () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'oneasr-hw-save-'))
    try {
      const store = loadHotwords(dir)
      store.groups[0].words = '甲词\n乙词'
      saveHotwords(store, dir)
      expect(loadHotwords(dir).groups[0].words).toBe('甲词\n乙词')
    } finally {
      fs.rmSync(dir, { recursive: true, force: true })
    }
  })
})

import fs from 'fs'
import os from 'os'
import path from 'path'
import { describe, expect, it } from 'vitest'
import { hotwordPath, loadHotwords, newTaskGroup, saveHotwords } from './hotword'

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

  it('自动启用只绑有词的默认组：空组当作「无热词」', () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'oneasr-hw-default-'))
    try {
      const store = loadHotwords(dir)
      store.default_group_enabled = true
      store.default_group = 'default'

      // 组里只有空白 / 分隔符时，不能把新任务绑到一个没有词的组上。
      expect(newTaskGroup(store)).toBeNull()
      store.groups[0].words = '  , \n'
      expect(newTaskGroup(store)).toBeNull()

      store.groups[0].words = '甲词, 乙词'
      expect(newTaskGroup(store)).toBe('default')

      // 关掉开关 / 默认组不存在：都不绑。
      store.default_group_enabled = false
      expect(newTaskGroup(store)).toBeNull()
      store.default_group_enabled = true
      store.default_group = 'gone'
      expect(newTaskGroup(store)).toBeNull()
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

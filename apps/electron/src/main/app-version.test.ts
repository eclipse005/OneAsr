import fs from 'fs'
import os from 'os'
import path from 'path'
import { afterEach, describe, expect, it } from 'vitest'
import { readAppVersion, versionMismatchNote } from './log'

const temps: string[] = []

function tmpDir(name: string): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), `oneasr-${name}-`))
  temps.push(dir)
  return dir
}

afterEach(() => {
  for (const dir of temps.splice(0)) {
    fs.rmSync(dir, { recursive: true, force: true })
  }
})

/**
 * 为什么要单独测这个：`app.getVersion()` 在开发期返回的是 **Electron 的版本**
 * （44.x），把它当应用版本去和引擎版本比对，会凭空造出一条「版本不一致」警告。
 * 第一版实现就踩了这个坑（日志里出现 `cli=2.0.0 app=44.9.0`），所以：
 * **读不到就返回 null，绝不回退成别的版本号。**
 */
describe('readAppVersion', () => {
  it('读到 package.json 的 version', () => {
    const dir = tmpDir('pkg')
    const file = path.join(dir, 'package.json')
    fs.writeFileSync(file, JSON.stringify({ version: '2.0.0' }))
    expect(readAppVersion([file])).toBe('2.0.0')
  })

  it('按候选顺序取第一个成功的（out/main → ../../package.json）', () => {
    const root = tmpDir('layout')
    const outMain = path.join(root, 'out', 'main')
    fs.mkdirSync(outMain, { recursive: true })
    fs.writeFileSync(path.join(root, 'package.json'), JSON.stringify({ version: '9.9.9' }))

    const missing = path.join(outMain, 'package.json')
    const real = path.resolve(outMain, '..', '..', 'package.json')
    expect(readAppVersion([missing, real])).toBe('9.9.9')
  })

  it('都读不到时返回 null，不编造版本号', () => {
    const dir = tmpDir('nothing')
    expect(readAppVersion([path.join(dir, 'nope.json')])).toBeNull()
    expect(readAppVersion([])).toBeNull()
  })

  it('package.json 损坏或没有 version 时返回 null', () => {
    const dir = tmpDir('broken')
    const broken = path.join(dir, 'broken.json')
    fs.writeFileSync(broken, '{ not json')
    const noVersion = path.join(dir, 'noversion.json')
    fs.writeFileSync(noVersion, JSON.stringify({ name: 'x' }))
    expect(readAppVersion([broken])).toBeNull()
    expect(readAppVersion([noVersion])).toBeNull()
  })
})

/**
 * 比对的**边界**比比对本身更重要：界面版本读不到时必须闭嘴。
 * 第一版实现回退到了 `app.getVersion()`（Electron 的 44.x），在日志里凭空造出
 * `cli=2.0.0 app=44.9.0` 这条假警告——刚删掉一个误报就又造一个。
 */
describe('versionMismatchNote', () => {
  it('版本一致时不报警', () => {
    expect(versionMismatchNote('2.0.0', '2.0.0')).toBeNull()
  })

  it('版本不同时给可搜的 WARN', () => {
    const note = versionMismatchNote('2.0.0', '2.1.0')
    expect(note).toContain('cli=2.0.0')
    expect(note).toContain('app=2.1.0')
  })

  it('界面版本读不到（null）时不报警 —— 宁可不比，不造假警告', () => {
    expect(versionMismatchNote('2.0.0', null)).toBeNull()
  })

  it('引擎没报版本时也不报警', () => {
    expect(versionMismatchNote(undefined, '2.0.0')).toBeNull()
    expect(versionMismatchNote('', '2.0.0')).toBeNull()
  })
})

import fs from 'fs'
import os from 'os'
import path from 'path'
import { afterEach, describe, expect, it } from 'vitest'
import { chooseDevAppRoot, hasCliBinary } from './settings'

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

describe('chooseDevAppRoot', () => {
  it('仓库只有 ffmpeg、旁路 oneasr-runtime 有 CLI 时选 runtime', () => {
    const base = tmpDir('layout')
    const repo = path.join(base, 'OneAsr')
    const runtime = path.join(base, 'oneasr-runtime')
    fs.mkdirSync(path.join(repo, 'bin'), { recursive: true })
    fs.writeFileSync(path.join(repo, 'bin', 'ffmpeg'), '')
    fs.mkdirSync(runtime, { recursive: true })
    fs.writeFileSync(path.join(runtime, 'oneasr-cli'), '')
    fs.chmodSync(path.join(runtime, 'oneasr-cli'), 0o755)

    const picked = chooseDevAppRoot([repo])
    expect(picked).toBe(runtime)
    expect(hasCliBinary(runtime)).toBe(true)
    expect(hasCliBinary(repo)).toBe(false)
  })

  it('仓库本身有 CLI 时仍选仓库', () => {
    const repo = tmpDir('repo-cli')
    fs.mkdirSync(path.join(repo, 'bin'), { recursive: true })
    fs.writeFileSync(path.join(repo, 'bin', 'ffmpeg'), '')
    fs.writeFileSync(path.join(repo, 'oneasr-cli'), '')
    expect(chooseDevAppRoot([repo])).toBe(repo)
  })

  it('都没有 CLI 时回退到第一个 ffmpeg 根', () => {
    const repo = tmpDir('ffmpeg-only')
    fs.mkdirSync(path.join(repo, 'bin'), { recursive: true })
    fs.writeFileSync(path.join(repo, 'bin', 'ffmpeg'), '')
    expect(chooseDevAppRoot([repo], [])).toBe(repo)
  })

  it('extraRoots 可补上 /workspace/oneasr-runtime 一类路径', () => {
    const repo = tmpDir('repo')
    const runtime = tmpDir('runtime-extra')
    fs.mkdirSync(path.join(repo, 'bin'), { recursive: true })
    fs.writeFileSync(path.join(repo, 'bin', 'ffmpeg'), '')
    fs.writeFileSync(path.join(runtime, 'oneasr-cli'), '')
    expect(chooseDevAppRoot([repo], [runtime])).toBe(runtime)
  })
})

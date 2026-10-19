import fs from 'fs'
import os from 'os'
import path from 'path'
import { afterEach, describe, expect, it } from 'vitest'
import {
  clearDiagnosticLogs,
  diagnosticLogInfo,
  diagnosticLogPaths,
  diagnosticLogToOpen
} from './log-files'

const dirs: string[] = []

function scratch(): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'oneasr-log-'))
  dirs.push(dir)
  return dir
}

afterEach(() => {
  for (const dir of dirs.splice(0)) {
    fs.rmSync(dir, { recursive: true, force: true })
  }
})

describe('诊断日志文件', () => {
  it('大小是当前文件和 .old 之和，不算任务日志', () => {
    const dir = scratch()
    const [current, old] = diagnosticLogPaths(dir)
    fs.writeFileSync(current, 'a'.repeat(100))
    fs.writeFileSync(old, 'b'.repeat(50))
    fs.writeFileSync(path.join(dir, 'oneasr-task-x.log'), 'c'.repeat(999))
    expect(path.basename(current)).toBe('oneasr-error.log')
    expect(path.basename(old)).toBe('oneasr-error.log.old')
    expect(diagnosticLogInfo(dir)).toEqual({ bytes: 150, currentExists: true })
    expect(diagnosticLogToOpen(dir)).toBe(current)
  })

  it('只有轮转文件时也计入大小，打开指向它', () => {
    const dir = scratch()
    const [, old] = diagnosticLogPaths(dir)
    fs.writeFileSync(old, 'x'.repeat(20))
    expect(diagnosticLogInfo(dir)).toEqual({ bytes: 20, currentExists: false })
    expect(diagnosticLogToOpen(dir)).toBe(old)
  })

  it('清理删掉当前和 .old，留下任务日志', () => {
    const dir = scratch()
    const [current, old] = diagnosticLogPaths(dir)
    const task = path.join(dir, 'oneasr-task-x.log')
    fs.writeFileSync(current, 'hello')
    fs.writeFileSync(old, 'world')
    fs.writeFileSync(task, 'keep')
    expect(clearDiagnosticLogs(dir)).toEqual({ bytes: 0, currentExists: false })
    expect(fs.existsSync(current)).toBe(false)
    expect(fs.existsSync(old)).toBe(false)
    expect(fs.readFileSync(task, 'utf8')).toBe('keep')
    expect(diagnosticLogToOpen(dir)).toBeUndefined()
  })

  it('没有日志时大小是 0，清理也不报错', () => {
    const dir = scratch()
    expect(diagnosticLogInfo(dir)).toEqual({ bytes: 0, currentExists: false })
    expect(clearDiagnosticLogs(dir)).toEqual({ bytes: 0, currentExists: false })
  })
})

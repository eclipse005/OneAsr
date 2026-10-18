import { spawn } from 'child_process'
import { describe, expect, it } from 'vitest'
import { killProcessTree } from './kill-tree'

const sleep = (ms: number): Promise<void> => new Promise((r) => setTimeout(r, ms))

/** 进程还活着吗（signal 0 只做权限/存在性检查，不发信号）。 */
function alive(pid: number | undefined): boolean {
  if (!pid) return false
  try {
    process.kill(pid, 0)
    return true
  } catch {
    return false
  }
}

/**
 * 起一个"假 CLI"：它自己再 spawn 一个长命后代（模拟 CLI 带出的 ffmpeg），
 * 并把后代 pid 打在 stdout 第一行。
 */
function spawnFakeCli(): Promise<{ child: ReturnType<typeof spawn>; grandPid: number }> {
  const script = [
    "const { spawn } = require('child_process')",
    "const g = spawn(process.execPath, ['-e', 'setTimeout(() => {}, 60000)'], { stdio: 'ignore' })",
    'process.stdout.write(String(g.pid) + "\\n")',
    'setTimeout(() => {}, 60000)'
  ].join(';')
  const child = spawn(process.execPath, ['-e', script], {
    stdio: ['ignore', 'pipe', 'ignore'],
    // 与壳里的转写 spawn 保持一致：POSIX 建独立进程组（往 -pid 发信号才能连后代一起收）
    detached: process.platform !== 'win32'
  })
  return new Promise((resolve, reject) => {
    let buf = ''
    const timer = setTimeout(() => reject(new Error('假 CLI 没报出后代 pid')), 10000)
    child.stdout?.on('data', (d) => {
      buf += d.toString()
      const line = buf.split(/\r?\n/)[0]
      if (line) {
        clearTimeout(timer)
        resolve({ child, grandPid: Number(line) })
      }
    })
    child.on('error', reject)
  })
}

describe('killProcessTree', () => {
  it(
    '取消时连后代一起收：假 CLI 自己 spawn 的长命子进程也必须在取消后消失',
    async () => {
      const { child, grandPid } = await spawnFakeCli()
      await sleep(300)
      expect(alive(child.pid)).toBe(true)
      expect(alive(grandPid)).toBe(true)

      killProcessTree(child, 'SIGTERM')
      // Windows 的 taskkill 是另起的进程，给它一点时间；POSIX 是同步发信号
      await sleep(1500)

      expect(alive(child.pid)).toBe(false)
      expect(alive(grandPid)).toBe(false)
    },
    20000
  )

  it('对已经死掉的 child 调用不抛异常（取消与关闭竞争时的常见情形）', async () => {
    const child = spawn(process.execPath, ['-e', '0'], { stdio: 'ignore' })
    await sleep(400)
    expect(() => killProcessTree(child, 'SIGKILL')).not.toThrow()
  })
})

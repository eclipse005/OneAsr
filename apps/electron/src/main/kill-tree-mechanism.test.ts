import { describe, expect, it, vi } from 'vitest'

/**
 * 机制层测试：断言**发出的是什么命令**，而不是靠"后代死了没"。
 *
 * 为什么不靠后者：本机（Windows）实测里，无论是 Node 假 CLI 带出的 Node 后代、
 * cmd/ping 后代，还是真 oneasr-cli 带出的真 ffmpeg，父进程被 TerminateProcess 之后
 * 后代都会跟着消失（这个环境另有回收机制），所以"后代是否存活"根本区分不出
 * "有没有树杀"——那种测试是假绿。真正能钉住的是：win32 上**在 SIGTERM 阶段**
 * 就必须发 taskkill /T /F（旧实现只发 child.kill，等到 1.5 秒后的 SIGKILL 才树杀，
 * 而那时树根已经没了）。
 */

const fakeChild = (pid: number): { pid: number; kill: ReturnType<typeof vi.fn> } => ({
  pid,
  kill: vi.fn(() => true)
})

describe('killProcessTree 的机制（平台分支）', () => {
  it.skipIf(process.platform !== 'win32')(
    'win32：SIGTERM 阶段就要 taskkill /T /F（不能等 SIGKILL 才树杀）',
    async () => {
      const spawnMock = vi.fn(() => ({ unref: vi.fn() }))
      vi.doMock('child_process', () => ({ spawn: spawnMock }))
      vi.resetModules()
      const { killProcessTree } = await import('./kill-tree')

      const child = fakeChild(4242)
      killProcessTree(child as never, 'SIGTERM')

      expect(spawnMock).toHaveBeenCalledTimes(1)
      const [cmd, args, opts] = spawnMock.mock.calls[0] as unknown as [string, string[], { windowsHide?: boolean }]
      expect(cmd).toBe('taskkill')
      expect(args).toEqual(['/PID', '4242', '/T', '/F'])
      expect(opts?.windowsHide).toBe(true)
      // 直接子进程也要打，兜住 taskkill 不可用的场景
      expect(child.kill).toHaveBeenCalledWith('SIGTERM')

      vi.doUnmock('child_process')
      vi.resetModules()
    }
  )

  it.skipIf(process.platform === 'win32')(
    'POSIX：往进程组发信号（-pid），再补一次直接子进程',
    async () => {
      const killSpy = vi.spyOn(process, 'kill').mockImplementation(() => true)
      const { killProcessTree } = await import('./kill-tree')

      const child = fakeChild(5150)
      killProcessTree(child as never, 'SIGTERM')

      const groupCall = killSpy.mock.calls.find((c) => c[0] === -5150)
      expect(groupCall?.[1]).toBe('SIGTERM')
      expect(child.kill).toHaveBeenCalledWith('SIGTERM')
      killSpy.mockRestore()
    }
  )
})

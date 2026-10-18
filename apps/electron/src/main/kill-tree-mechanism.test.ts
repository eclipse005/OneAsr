import { EventEmitter } from 'events'
import { describe, expect, it, vi } from 'vitest'

/**
 * 机制层测试：断言**发出/等待的是什么**，而不是靠"后代死了没"。
 *
 * 为什么不靠后者：本机（Windows）实测里，无论 Node 假 CLI 带出的 Node 后代、
 * cmd/ping 后代，还是真 oneasr-cli 带出的真 ffmpeg，父进程被 TerminateProcess 之后
 * 后代都会跟着消失（这个环境另有回收机制），所以"后代是否存活"区分不出有没有树杀。
 * 真要验实际效果，得找一台干净 Windows VM 跑；单测这层只钉机制。
 *
 * 这里钉的两条（旧实现两条都会红）：
 * 1. win32 上 SIGTERM 阶段就必须发 taskkill /T /F（不能等 1.5s 后的 SIGKILL）；
 * 2. **taskkill 结束前不许调 child.kill** —— spawn 只保证进程被创建，不保证它已经
 *    枚举完目标树；抢先 TerminateProcess 掉树根，taskkill 就找不到这棵树了。
 *    只有 taskkill 明确失败（error / close 非 0）才允许回退。
 */

/** taskkill 的替身：只实现我们用到的 EventEmitter + unref。 */
class FakeKiller extends EventEmitter {
  unref(): this {
    return this
  }
}

function fakeChild(pid: number) {
  return { pid, kill: vi.fn(() => true) }
}

async function loadWithSpawn(spawnImpl: unknown): Promise<typeof import('./kill-tree')> {
  vi.doMock('child_process', () => ({ spawn: spawnImpl }))
  vi.resetModules()
  return import('./kill-tree')
}

describe('killProcessTree 的机制（平台分支）', () => {
  it.skipIf(process.platform !== 'win32')(
    'win32：发 taskkill /T /F，taskkill 成功收尾前绝不 child.kill',
    async () => {
      const killer = new FakeKiller()
      const spawnMock = vi.fn(() => killer)
      const { killProcessTree } = await loadWithSpawn(spawnMock)

      const child = fakeChild(4242)
      killProcessTree(child as never, 'SIGTERM')

      expect(spawnMock).toHaveBeenCalledTimes(1)
      const [cmd, args, opts] = spawnMock.mock.calls[0] as unknown as [
        string,
        string[],
        { windowsHide?: boolean; stdio?: string }
      ]
      expect(cmd).toBe('taskkill')
      expect(args).toEqual(['/PID', '4242', '/T', '/F'])
      expect(opts?.windowsHide).toBe(true)
      expect(opts?.stdio).toBe('ignore')
      // 关键：还没等 taskkill 结束，不能碰树根
      expect(child.kill).not.toHaveBeenCalled()

      killer.emit('close', 0)
      // 成功收尾 → 不回退
      expect(child.kill).not.toHaveBeenCalled()

      vi.doUnmock('child_process')
      vi.resetModules()
    }
  )

  it.skipIf(process.platform !== 'win32')(
    'win32：taskkill 失败（close 非 0 或 error）才回退 child.kill',
    async () => {
      // close 非 0
      const killer1 = new FakeKiller()
      const mod1 = await loadWithSpawn(vi.fn(() => killer1))
      const child1 = fakeChild(100)
      mod1.killProcessTree(child1 as never, 'SIGTERM')
      killer1.emit('close', 1)
      expect(child1.kill).toHaveBeenCalledWith('SIGTERM')

      // error（taskkill 不存在/权限不足）
      const killer2 = new FakeKiller()
      const mod2 = await loadWithSpawn(vi.fn(() => killer2))
      const child2 = fakeChild(200)
      mod2.killProcessTree(child2 as never, 'SIGKILL')
      killer2.emit('error', new Error('ENOENT'))
      expect(child2.kill).toHaveBeenCalledWith('SIGKILL')

      vi.doUnmock('child_process')
      vi.resetModules()
    }
  )

  it.skipIf(process.platform !== 'win32')(
    'win32：spawn 自己抛错时回退（并且不留下未处理的 error）',
    async () => {
      const spawnMock = vi.fn(() => {
        throw new Error('spawn EPERM')
      })
      const { killProcessTree } = await loadWithSpawn(spawnMock)
      const child = fakeChild(300)
      expect(() => killProcessTree(child as never, 'SIGTERM')).not.toThrow()
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

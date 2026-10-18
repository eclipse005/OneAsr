import { ChildProcess, spawn } from 'child_process'

export type KillSignal = 'SIGTERM' | 'SIGKILL'

/**
 * 杀掉一个 child 及其**整棵进程树**（转写会带出 ffmpeg 等后代）。
 *
 * Windows：**只发 `taskkill /PID <pid> /T /F`，成功就不碰 `child.kill`。**
 *
 * 两个都踩过的坑，别再走回去：
 * 1. 顺序反了等于没有树杀 —— Windows 上没有可捕获的 SIGTERM，`child.kill()` 就是
 *    TerminateProcess：树根一死，`taskkill /T` 再也找不到这棵树。
 * 2. **紧跟着 `child.kill` 也是竞态** —— `spawn` 只保证 taskkill 进程被创建，
 *    不保证它已经枚举完目标树；抢先把树根拆掉，taskkill 一样找不到树（窗口更小而已）。
 *    所以正常路径要把 `child.kill` 交给 taskkill 的收尾事件去决定：
 *    `error`（ENOENT/权限）或 `close` 非 0 才回退直接子进程。
 *
 * POSIX：spawn 时用 detached 建了独立进程组（见 jobs.ts 的转写 spawn），
 * 往 -pid 发信号才能连后代一起收；进程组没了再补一次直接子进程。
 */
export function killProcessTree(child: ChildProcess, signal: KillSignal): void {
  const pid = child.pid

  if (process.platform === 'win32') {
    const fallback = (): void => {
      try {
        child.kill(signal)
      } catch {
        /* 进程可能已经没了 */
      }
    }
    if (!pid) {
      fallback()
      return
    }
    let killer: ChildProcess
    try {
      killer = spawn('taskkill', ['/PID', String(pid), '/T', '/F'], {
        // stdio: 'ignore' —— taskkill 的输出没人看；不挂 error 的话异步 spawn 失败
        // 会变成未处理的 'error' 事件（进程级未捕获异常）
        stdio: 'ignore',
        windowsHide: true
      })
    } catch {
      fallback()
      return
    }
    killer.on('error', fallback)
    killer.on('close', (code) => {
      if (code !== 0) fallback()
    })
    // 不阻塞退出流程；进程本身继续跑（Windows 上父进程退出不会带走它）
    killer.unref()
    return
  }

  if (pid) {
    try {
      process.kill(-pid, signal)
    } catch {
      /* 进程组可能已经没了，下面再试直接子进程 */
    }
  }
  try {
    child.kill(signal)
  } catch {
    /* ignore */
  }
}

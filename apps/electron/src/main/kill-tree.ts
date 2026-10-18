import { ChildProcess, spawn } from 'child_process'

export type KillSignal = 'SIGTERM' | 'SIGKILL'

/**
 * 杀掉一个 child 及其**整棵进程树**（转写会带出 ffmpeg 等后代）。
 *
 * **Windows 的顺序是关键：先 taskkill /T /F，再 child.kill()。**
 * Windows 上没有可捕获的 SIGTERM —— `child.kill()` 走的是 TerminateProcess，CLI 立刻死；
 * 一旦树根没了，`taskkill /T` 就再也找不到这棵树，脱离的 ffmpeg 会变成孤儿。
 * 所以"先温柔地 kill 一下、1.5 秒后再树杀"在 Windows 上是错的顺序：
 * 常见取消路径根本走不到 taskkill（外部 review 指出的缺口）。
 *
 * POSIX：spawn 时用 detached 建了独立进程组（见 jobs.ts 的转写 spawn），
 * 往 -pid 发信号才能连后代一起收；进程组没了再补一次直接子进程。
 */
export function killProcessTree(child: ChildProcess, signal: KillSignal): void {
  const pid = child.pid
  if (process.platform === 'win32') {
    if (pid) {
      try {
        // 不走 shell（避免 quoting/转义问题）；windowsHide 防止闪一个控制台窗口。
        // unref：不阻塞退出流程。
        spawn('taskkill', ['/PID', String(pid), '/T', '/F'], { windowsHide: true }).unref()
      } catch {
        /* taskkill 不可用时退回只杀直接子进程 */
      }
    }
    try {
      child.kill(signal)
    } catch {
      /* 可能已经被 taskkill 杀掉了 */
    }
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

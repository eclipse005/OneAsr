/**
 * 小并发闸门：同一时刻最多跑 `limit` 个任务，其余排队。
 *
 * 批量拖入文件时每个探针都要 spawn 一次 CLI（CLI 再 spawn ffmpeg），
 * 不设上限会把机器瞬间打满。core 里的探针池是**进程内**的，管不到
 * 壳这一侧的多个 CLI 进程。
 */
export type Limit = <T>(fn: () => Promise<T>) => Promise<T>

export function createLimit(limit: number): Limit {
  if (!Number.isInteger(limit) || limit < 1) {
    throw new Error(`limit must be a positive integer, got ${limit}`)
  }
  let active = 0
  const waiters: Array<() => void> = []

  return async function run<T>(fn: () => Promise<T>): Promise<T> {
    if (active >= limit) {
      await new Promise<void>((resolve) => waiters.push(resolve))
    }
    active += 1
    try {
      return await fn()
    } finally {
      active -= 1
      // 让出一个位置后唤醒最早等待者；它会在自己的 finally 里继续传下去。
      waiters.shift()?.()
    }
  }
}

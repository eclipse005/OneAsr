import type { TaskStatus } from './types'

/**
 * 「全部开始」只带走还没跑的和失败的。
 * 已完成留在原地；已经在运行队列里的不要再入队一次。
 * 取消过的行会被 restorePending 成待处理（queued + 不在运行队列），因而可以再次被带走。
 * 重跑某一行用那一行自己的动作。
 */
export function isBulkStartCandidate(status: TaskStatus, inRunQueue: boolean): boolean {
  if (status === 'error') return true
  return status === 'queued' && !inRunQueue
}

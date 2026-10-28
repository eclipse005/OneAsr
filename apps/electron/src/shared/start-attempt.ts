/**
 * 启动门禁是异步的：等待期间用户可能点了取消。
 * 用代数号作废那一次 startOne/startAll，而不是把行钉成 cancelled 终态。
 */
export function bumpStartAttempt(epochs: Map<string, number>, id: string): number {
  const next = (epochs.get(id) ?? 0) + 1
  epochs.set(id, next)
  return next
}

export function isStartAttemptCurrent(
  epochs: Map<string, number>,
  id: string,
  attempt: number
): boolean {
  return (epochs.get(id) ?? 0) === attempt
}

/**
 * 一次运行算不算成功、产物在哪。
 *
 * 产物路径的**唯一**出处是 CLI 的 `done` 事件：文件名由 core 决定
 * （普通转写是 `<stem>.srt`，文稿匹配是 `<stem>.aligned.srt`），壳不自己拼。
 *
 * 为什么不能拼：拼了等于把 core 的命名规则抄一份到壳里。规则一变（文稿
 * 匹配加 `.aligned` 就是现成的例子），壳就会静默去找一个不存在的文件——
 * 更糟的是，它会找到**上一轮遗留的同名文件**，把那次的结果当成本次的产物
 * 报给用户。抄规则这件事，规则一变就错，错了还是静默的。
 *
 * 退出码 0 却没有产物路径 = 协议被破坏（CLI 成功时必发 `done`）。这时宁可
 * 报失败：一份说不出名字的产物没法打开、没法核对，标成成功就是假绿。
 */

/** 运行结果：要么有产物路径，要么说清为什么不算成功。 */
export type RunOutcome =
  | { ok: true; outputPath: string }
  | { ok: false; reason: 'failed' | 'no-output' }

/**
 * @param exitCode 子进程的退出码（`null` = 被信号打断）
 * @param reportedOutput CLI 的 `done` 事件报出来的产物路径
 */
export function resolveRunOutcome(
  exitCode: number | null,
  reportedOutput?: string
): RunOutcome {
  if (exitCode !== 0) return { ok: false, reason: 'failed' }
  const output = reportedOutput?.trim()
  if (!output) return { ok: false, reason: 'no-output' }
  return { ok: true, outputPath: output }
}

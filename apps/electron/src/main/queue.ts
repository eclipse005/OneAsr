/**
 * 排队 / 取消 / 泵出的**纯状态机**：只吃事件、只吐命令，不 spawn、不 kill；时钟可注入。
 *
 * 为什么要有它：jobs.ts 旧实现里 running / runningId / busy / queue 是四个字段，
 * cancel 会先把它们清空再 pump 下一条，而旧 child 的 close 回调与 1.5s 的 SIGKILL 定时器
 * 读的又是"当时"的 this.running。于是下一条可能无法取消、第三条可能与第二条并发、
 * 旧定时器可能误杀新 child。把"谁是当前 run"收进这一个类之后，外面只执行命令，
 * 不再自己维护运行态。
 *
 * 输入事件：
 * - enqueue(task)       入队；空闲时立刻给出 spawn
 * - cancel(id)          排队中 = 撤下；运行中 = 只标记取消意图并给出 SIGTERM
 * - childClosed(runId)  **唯一**会启动下一条的入口；runId 对不上就整条忽略
 * - tick()              取消宽限期到点，给当前那个仍处于取消中的 run 发 SIGKILL
 *
 * 输出命令（由 jobs.ts 执行）：
 * - { type: 'spawn', runId, task }
 * - { type: 'kill', runId, taskId, signal }
 *
 * 不变量：
 * 1. 同一时刻最多一个 current run；下一条只在该 run 的 close 之后启动。
 * 2. 每次运行有唯一的 runId；晚到的旧 close / 旧定时器不能碰当前 run。
 * 3. view() 是运行态的唯一真源（jobs.ts 不再保存 running/runningId/busy）。
 */

export type RunId = number
export type KillSignal = 'SIGTERM' | 'SIGKILL'

export interface QueueItem {
  id: string
}

export interface SpawnCommand<T extends QueueItem> {
  type: 'spawn'
  runId: RunId
  task: T
}

export interface KillCommand {
  type: 'kill'
  runId: RunId
  taskId: string
  signal: KillSignal
}

export type QueueCommand<T extends QueueItem> = SpawnCommand<T> | KillCommand

/** 外部能读到的运行态。runId/taskId 为 null 时 busy 必为 false。 */
export interface QueueView {
  busy: boolean
  runId: RunId | null
  taskId: string | null
  cancelling: boolean
}

export interface CancelResult<T extends QueueItem> {
  /** running = 这条正在跑（已发/已发过 SIGTERM）；queued = 从队列撤下；absent = 没找到 */
  outcome: 'queued' | 'running' | 'absent'
  /** outcome === 'running' 时，被取消的那次 run */
  runId?: RunId
  commands: QueueCommand<T>[]
}

export interface JobQueueOptions {
  now?: () => number
  /** SIGTERM 到 SIGKILL 的宽限期；jobs.ts 的定时器也读同一个值 */
  killGraceMs?: number
}

export const KILL_GRACE_MS = 1500

interface CurrentRun<T extends QueueItem> {
  runId: RunId
  task: T
  /** 取消意图：只影响这个 run，不会把它从 current 里摘掉 */
  cancelled: boolean
  /** 宽限期截止时刻；cancelled 为 true 时才有 */
  killDeadline: number | null
  /** SIGKILL 只发一次 */
  escalated: boolean
}

export class JobQueue<T extends QueueItem> {
  private queue: T[] = []
  private current: CurrentRun<T> | null = null
  private nextRunId = 0
  private readonly now: () => number
  readonly killGraceMs: number

  constructor(options: JobQueueOptions = {}) {
    this.now = options.now ?? Date.now
    this.killGraceMs = options.killGraceMs ?? KILL_GRACE_MS
  }

  /**
   * 入队。已在队列里不重复入队；当前 run 正在取消时允许同一行重新排队
   * （旧 startOne 在 cancel 之后可以再起同一行，这里是同一语义）。
   */
  enqueue(task: T): QueueCommand<T>[] {
    const alreadyQueued = this.queue.some((t) => t.id === task.id)
    const sameAsCurrent = this.current?.task.id === task.id && !this.current.cancelled
    if (alreadyQueued || sameAsCurrent) return []
    this.queue.push(task)
    if (this.current) return []
    return this.startNext()
  }

  /**
   * 取消：排队中的行撤下来；运行中的行**只标记意图**并发 SIGTERM。
   * 注意这里绝不会 spawn 下一条——那要等 childClosed(runId)。
   */
  cancel(id: string, now = this.now()): CancelResult<T> {
    const before = this.queue.length
    this.queue = this.queue.filter((t) => t.id !== id)
    const removedQueued = this.queue.length !== before
    const run = this.current

    if (run?.task.id === id) {
      if (run.cancelled) {
        // 重复取消是幂等的：不再发第二个 SIGTERM，也不重置宽限期
        return { outcome: 'running', runId: run.runId, commands: [] }
      }
      run.cancelled = true
      run.killDeadline = now + this.killGraceMs
      return {
        outcome: 'running',
        runId: run.runId,
        commands: [
          { type: 'kill', runId: run.runId, taskId: run.task.id, signal: 'SIGTERM' }
        ]
      }
    }

    return { outcome: removedQueued ? 'queued' : 'absent', commands: [] }
  }

  /**
   * 一次 child 关闭。runId 必须与 current 一致；旧 run 晚到的 close 直接忽略，
   * 既不清当前运行态、也不启动下一条。只有这里会启动下一条。
   */
  childClosed(runId: RunId): QueueCommand<T>[] {
    if (!this.current || this.current.runId !== runId) return []
    this.current = null
    return this.startNext()
  }

  /** 取消宽限期到点：只对"仍是 current 且仍在取消中"的那个 run 升级 SIGKILL。 */
  tick(now = this.now()): QueueCommand<T>[] {
    const run = this.current
    if (!run || !run.cancelled || run.escalated) return []
    if (run.killDeadline == null || now < run.killDeadline) return []
    run.escalated = true
    return [
      { type: 'kill', runId: run.runId, taskId: run.task.id, signal: 'SIGKILL' }
    ]
  }

  /** 运行态唯一真源。 */
  view(): QueueView {
    const run = this.current
    return {
      busy: run !== null,
      runId: run?.runId ?? null,
      taskId: run?.task.id ?? null,
      cancelling: run?.cancelled ?? false
    }
  }

  isRunning(id: string): boolean {
    return this.current?.task.id === id
  }

  isCurrentRun(runId: RunId): boolean {
    return this.current?.runId === runId
  }

  /**
   * kill 命令的唯一裁决点：只有仍打在 current run 上的命令才放行。
   * 旧 run 的晚到定时器 / 晚到命令（runId 对不上）返回 null，调用方不得执行。
   */
  resolveKill(cmd: KillCommand): KillCommand | null {
    return this.current?.runId === cmd.runId ? cmd : null
  }

  /** 排队位次，1 = 下一个跑；不在队列里返回 undefined。 */
  queueRank(id: string): number | undefined {
    const i = this.queue.findIndex((t) => t.id === id)
    return i < 0 ? undefined : i + 1
  }

  /** 清空排队区，**保留当前 run**（对应 jobs.clear() 的语义）。 */
  clearQueued(): T[] {
    const removed = this.queue
    this.queue = []
    return removed
  }

  removeQueued(id: string): boolean {
    const before = this.queue.length
    this.queue = this.queue.filter((t) => t.id !== id)
    return this.queue.length !== before
  }

  /** 退出：丢掉排队区与当前 run（child 由 jobs.ts 自己收）。 */
  shutdown(): { runId: RunId; task: T } | null {
    const run = this.current
    this.current = null
    this.queue = []
    return run ? { runId: run.runId, task: run.task } : null
  }

  private startNext(): QueueCommand<T>[] {
    const task = this.queue.shift()
    if (!task) return []
    this.nextRunId += 1
    this.current = {
      runId: this.nextRunId,
      task,
      cancelled: false,
      killDeadline: null,
      escalated: false
    }
    return [{ type: 'spawn', runId: this.nextRunId, task }]
  }
}

import { describe, expect, it } from 'vitest'
import { JobQueue, type KillCommand, type KillSignal, type QueueCommand } from './queue'

interface FakeTask {
  id: string
}

interface FakeChild {
  runId: number
  taskId: string
  kills: KillSignal[]
  closed: boolean
}

/**
 * 假执行器：只做 jobs.ts 在真实代码里做的那两件事——按命令 spawn、按命令 kill。
 * 子进程不会自己退出：测试显式调用 close(runId)，这样"启动下一条"的时机才可断言。
 * 时钟是手动的：advance() 之后 tick() 才会触发 SIGKILL 升级。
 */
class Harness {
  readonly clock = { now: 1_000 }
  readonly queue: JobQueue<FakeTask>
  readonly spawned: string[] = []
  readonly children = new Map<number, FakeChild>()

  constructor() {
    this.queue = new JobQueue<FakeTask>({ now: () => this.clock.now })
  }

  enqueue(id: string): void {
    this.exec(this.queue.enqueue({ id }))
  }

  cancel(id: string): void {
    this.exec(this.queue.cancel(id).commands)
  }

  /** 子进程真的退出了：标记死亡，再把 close 喂回状态机。 */
  close(runId: number): void {
    const child = this.children.get(runId)
    if (!child) throw new Error('no such child: ' + runId)
    child.closed = true
    this.exec(this.queue.childClosed(runId))
  }

  /** 同一个 close 再来一次（旧回调晚到）：状态机必须完全忽略。 */
  replayClose(runId: number): void {
    this.exec(this.queue.childClosed(runId))
  }

  advance(ms: number): void {
    this.clock.now += ms
  }

  tick(): void {
    this.exec(this.queue.tick())
  }

  exec(commands: QueueCommand<FakeTask>[]): void {
    for (const cmd of commands) {
      if (cmd.type === 'spawn') {
        if (this.children.has(cmd.runId)) throw new Error('run id reused: ' + cmd.runId)
        this.children.set(cmd.runId, {
          runId: cmd.runId,
          taskId: cmd.task.id,
          kills: [],
          closed: false
        })
        this.spawned.push(cmd.task.id)
        continue
      }
      // 和 jobs.ts 一样：kill 先过状态机裁决，runId 对不上就整条丢掉
      const allowed = this.queue.resolveKill(cmd)
      if (!allowed) continue
      const child = this.children.get(allowed.runId)
      if (!child) throw new Error('kill for unknown run: ' + allowed.runId)
      if (child.closed) throw new Error('kill for closed run: ' + allowed.runId)
      child.kills.push(allowed.signal)
    }
  }

  kills(taskId: string): KillSignal[] {
    let found: KillSignal[] = []
    this.children.forEach((child) => {
      if (child.taskId === taskId) found = [...child.kills]
    })
    return found
  }

  /** 任一时刻仍在跑的 child 数（收到 SIGTERM 但还没 close 也算在跑）。 */
  concurrent(): number {
    let n = 0
    this.children.forEach((child) => {
      if (!child.closed) n += 1
    })
    return n
  }
}

describe('JobQueue（排队 / 取消 / 泵出的纯状态机）', () => {
  it('1. 取消 A 后，B 只在 A 的 close 之后才启动', () => {
    const h = new Harness()
    h.enqueue('A')
    h.enqueue('B')
    expect(h.spawned).toEqual(['A'])

    h.cancel('A')
    expect(h.kills('A')).toEqual(['SIGTERM'])
    expect(h.spawned).toEqual(['A'])

    h.close(1)
    expect(h.spawned).toEqual(['A', 'B'])
  })

  it('2. B 启动后仍能被取消', () => {
    const h = new Harness()
    h.enqueue('A')
    h.enqueue('B')
    h.cancel('A')
    h.close(1)

    expect(h.spawned).toEqual(['A', 'B'])
    h.cancel('B')
    expect(h.kills('B')).toEqual(['SIGTERM'])
  })

  it('3. C 不得与 B 并发（任一时刻最多一个在跑）', () => {
    const h = new Harness()
    h.enqueue('A')
    h.enqueue('B')
    h.enqueue('C')
    h.cancel('A')
    h.close(1)
    expect(h.spawned).toEqual(['A', 'B'])

    h.cancel('B')
    expect(h.spawned).toEqual(['A', 'B'])
    expect(h.concurrent()).toBe(1)

    h.close(2)
    expect(h.spawned).toEqual(['A', 'B', 'C'])
    expect(h.concurrent()).toBe(1)
  })

  it('4. 旧进程的 SIGKILL 定时器不能误杀当前 child', () => {
    const h = new Harness()
    h.enqueue('A')
    h.enqueue('B')
    h.cancel('A')

    h.advance(1_500)
    h.tick()

    expect(h.kills('B')).toEqual([])
    expect(h.kills('A')).toEqual(['SIGTERM', 'SIGKILL'])
  })

  it('4a. A 已 close、B 已启动后，A 的 1.5s 定时器到点不能落到 B 上', () => {
    const h = new Harness()
    h.enqueue('A')
    h.enqueue('B')
    h.cancel('A')
    // A 真的退出，B 才启动；此时 A 的宽限定时器还挂着
    h.close(1)
    expect(h.spawned).toEqual(['A', 'B'])

    h.advance(1_500)
    h.tick()

    expect(h.kills('B')).toEqual([])
    expect(h.kills('A')).toEqual(['SIGTERM'])
  })

  it('4b. B 启动后，属于旧 run（A）的 kill 命令必须被忽略', () => {
    const h = new Harness()
    h.enqueue('A')
    h.enqueue('B')
    h.cancel('A')
    h.close(1)
    expect(h.spawned).toEqual(['A', 'B'])
    // 前提：状态机确实认 B 是当前 run（旧逻辑在这里会丢）
    expect(h.queue.view()).toEqual({
      busy: true,
      runId: 2,
      taskId: 'B',
      cancelling: false
    })

    const stale: KillCommand = { type: 'kill', runId: 1, taskId: 'A', signal: 'SIGKILL' }
    // 状态机裁决：旧 runId 不匹配当前 run，返回 null（调用方不得下发给任何 child）
    expect(h.queue.resolveKill(stale)).toBeNull()
    h.exec([stale])
    expect(h.kills('B')).toEqual([])
    expect(h.spawned).toEqual(['A', 'B'])

    const live: KillCommand = { type: 'kill', runId: 2, taskId: 'B', signal: 'SIGTERM' }
    expect(h.queue.resolveKill(live)).toEqual(live)
  })

  it('5. running/runningId/busy 只有一个真源，外部读到的始终是当前运行', () => {
    const h = new Harness()
    h.enqueue('A')
    h.enqueue('B')
    expect(h.queue.view()).toEqual({
      busy: true,
      runId: 1,
      taskId: 'A',
      cancelling: false
    })

    h.cancel('A')
    expect(h.queue.view()).toEqual({
      busy: true,
      runId: 1,
      taskId: 'A',
      cancelling: true
    })

    h.close(1)
    expect(h.queue.view()).toEqual({
      busy: true,
      runId: 2,
      taskId: 'B',
      cancelling: false
    })
    expect(h.queue.isRunning('B')).toBe(true)
    expect(h.queue.isCurrentRun(2)).toBe(true)

    h.replayClose(1)
    expect(h.queue.view()).toEqual({
      busy: true,
      runId: 2,
      taskId: 'B',
      cancelling: false
    })

    h.cancel('B')
    h.close(2)
    expect(h.queue.view()).toEqual({
      busy: false,
      runId: null,
      taskId: null,
      cancelling: false
    })
  })

  it('额外：排队位次与 clear() 保留运行中的那条', () => {
    const h = new Harness()
    h.enqueue('A')
    h.enqueue('B')
    h.enqueue('C')
    expect(h.queue.queueRank('B')).toBe(1)
    expect(h.queue.queueRank('C')).toBe(2)
    expect(h.queue.queueRank('A')).toBeUndefined()

    const removed = h.queue.clearQueued().map((t) => t.id)
    expect(removed).toEqual(['B', 'C'])
    expect(h.queue.queueRank('B')).toBeUndefined()
    expect(h.queue.isRunning('A')).toBe(true)
    expect(h.queue.view().busy).toBe(true)
  })

  it('isActive：当前 run 与已入队都算，撤下/待处理不算', () => {
    const h = new Harness()
    h.enqueue('A')
    h.enqueue('B')
    expect(h.queue.isActive('A')).toBe(true)
    expect(h.queue.isActive('B')).toBe(true)
    expect(h.queue.isActive('C')).toBe(false)
    // B 还在排队区，撤下来就不算 active 了。
    h.cancel('B')
    expect(h.queue.isActive('B')).toBe(false)
    expect(h.queue.isActive('A')).toBe(true)
  })

  it('isCancelling：只认当前 run，且取消后重开同一行也不影响旧 run', () => {
    const h = new Harness()
    h.enqueue('A')
    h.enqueue('B')
    h.cancel('A')
    expect(h.queue.isCancelling(1)).toBe(true)
    expect(h.queue.isCancelling(2)).toBe(false)
    // 取消后马上重开 A：A 进队列（B 仍在前面），但旧 run 仍然是取消态。
    h.enqueue('A')
    expect(h.queue.isCancelling(1)).toBe(true)
    expect(h.queue.queueRank('A')).toBe(2)
  })
})

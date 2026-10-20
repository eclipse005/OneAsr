import { ChildProcess, spawn } from 'child_process'
import { EventEmitter } from 'events'
import type { DownloadState } from '../shared/types'
import { DEFAULT_APP_ROOT, dataRoot, resolveCli } from './settings'
import { cliEnv } from './cli'
import { LineBuffer } from '../shared/line-buffer'
import { logError } from './log'

/**
 * 模型下载：spawn `oneasr-cli download --model <id>`，解析它写在 **stdout** 的机器行。
 *
 * 为什么走 CLI 而不是在 Node 里自己拉：Range 续传、重试退避、revision 校验、
 * 逐文件 SHA-256 全在 oneasr-core 里（旧版界面用了很久），没有理由再写第二份。
 *
 * 契约（见 `oneasr-cli --help` 的 download 段落）：
 *   [download] state=<downloading|verifying|done|error> model=<id> done=<n> total=<n> pct=<n> [speed=<n>] [dir="<path>"]
 * 人类可读的双语行在 stderr，不参与解析。
 *
 * 不同模型可以同时下载：每个模型一条 CLI 子进程、一份状态；同一个模型重复点仍被挡住。
 * 取消 = kill 该模型的子进程；下载器写的是临时文件 + Range 续传，下次重跑会接着下。
 */
export class DownloadManager extends EventEmitter {
  private readonly children = new Map<string, ChildProcess>()
  private readonly current = new Map<string, DownloadState>()

  /** 本次运行里各模型的最新状态（含已完成/失败，供重开设置时回显）。 */
  state(): DownloadState[] {
    return Array.from(this.current.values())
  }

  isBusy(): boolean {
    return this.children.size > 0
  }

  start(model: string, targetDir?: string): { ok: boolean; code?: 'busy' | 'spawn' } {
    if (this.children.has(model)) return { ok: false, code: 'busy' }
    const cli = resolveCli()
    const args = [
      'download',
      '--model',
      model,
      '--app-root',
      DEFAULT_APP_ROOT,
      '--data-root',
      dataRoot()
    ]
    if (targetDir) args.push('--model-dir', targetDir)
    const initial: DownloadState = {
      model,
      state: 'verifying',
      doneBytes: 0,
      totalBytes: 0,
      pct: 0,
      dir: undefined
    }
    this.current.set(model, initial)
    this.emit('updated', initial)

    let child: ChildProcess
    try {
      child = spawn(cli, args, {
        cwd: DEFAULT_APP_ROOT,
        env: cliEnv(dataRoot()),
        windowsHide: true
      })
    } catch (error) {
      logError(`download spawn failed: ${cli}`, error)
      const failed = { ...initial, state: 'error' as const, message: String(error) }
      this.current.set(model, failed)
      this.emit('updated', failed)
      return { ok: false, code: 'spawn' }
    }
    this.children.set(model, child)

    const stdoutLines = new LineBuffer()
    let stderr = ''
    const onStdoutLine = (line: string): void => {
      const parsed = parseEvent(line)
      if (!parsed) return
      const next = { ...parsed, dir: parsed.dir ?? this.current.get(model)?.dir }
      this.current.set(model, next)
      this.emit('updated', next)
    }
    child.stdout?.on('data', (b) => stdoutLines.push(b.toString('utf8'), onStdoutLine))
    child.stderr?.on('data', (b) => {
      stderr += b.toString('utf8')
    })

    child.on('close', (code) => {
      stdoutLines.flush(onStdoutLine)
      this.children.delete(model)
      const prev = this.current.get(model)
      if (!prev || prev.state === 'done' || prev.state === 'error') return
      // 没收到终态就退出了：按退出码判定（0 = 已存在/完成，其它 = 失败/被取消）
      const next: DownloadState = {
        ...prev,
        state: code === 0 ? 'done' : 'error',
        pct: code === 0 ? 100 : prev.pct,
        message:
          code === 0
            ? undefined
            : prev.message ?? (stderr.trim().slice(-300) || `exit ${code}`)
      }
      this.current.set(model, next)
      this.emit('updated', next)
    })
    child.on('error', (error) => {
      this.children.delete(model)
      logError('download child error', error)
      const prev = this.current.get(model)
      if (!prev || prev.state === 'done' || prev.state === 'error') return
      const next = { ...prev, state: 'error' as const, message: String(error) }
      this.current.set(model, next)
      this.emit('updated', next)
    })
    return { ok: true }
  }

  cancel(model: string): void {
    const child = this.children.get(model)
    if (!child) return
    try {
      child.kill('SIGTERM')
    } catch {
      /* ignore */
    }
    const prev = this.current.get(model)
    if (!prev) return
    const next = { ...prev, state: 'error' as const, message: 'cancelled' }
    this.current.set(model, next)
    this.emit('updated', next)
  }

  /** 退出前收尾，别留孤儿。 */
  shutdown(): void {
    for (const child of Array.from(this.children.values())) {
      try {
        child.kill('SIGTERM')
        child.kill('SIGKILL')
      } catch {
        /* ignore */
      }
    }
    this.children.clear()
  }
}

/** 解析 CLI 的机器行；不是那条就返回 null。 */
function parseEvent(line: string): DownloadState | null {
  if (!line.startsWith('[download]')) return null
  const kv = new Map<string, string>()
  const re = /([a-z_]+)=(?:"([^"]*)"|(\S+))/g
  let m: RegExpExecArray | null
  while ((m = re.exec(line)) !== null) kv.set(m[1], m[2] ?? m[3] ?? '')
  const state = kv.get('state')
  if (state !== 'downloading' && state !== 'verifying' && state !== 'done' && state !== 'error') {
    return null
  }
  const done = Number(kv.get('done') ?? '0')
  const total = Number(kv.get('total') ?? '0')
  const pct = kv.get('pct') != null ? Number(kv.get('pct')) : total > 0 ? (done / total) * 100 : 0
  const speed = kv.has('speed') ? Number(kv.get('speed')) : undefined
  return {
    model: kv.get('model') ?? '',
    state,
    doneBytes: Number.isFinite(done) ? done : 0,
    totalBytes: Number.isFinite(total) ? total : 0,
    pct: Number.isFinite(pct) ? Math.max(0, Math.min(100, pct)) : 0,
    speed,
    dir: kv.get('dir'),
    message: kv.get('message')
  }
}

import { ChildProcess, spawn } from 'child_process'
import { EventEmitter } from 'events'
import type { DownloadState } from '../shared/types'
import { DEFAULT_APP_ROOT, resolveCli } from './settings'
import { logError } from './log'

/**
 * 模型下载：spawn `oneasr-cli download --model <id>`，解析它写在 **stdout** 的机器行。
 *
 * 为什么走 CLI 而不是在 Node 里自己拉：Range 续传、重试退避、revision 校验、
 * 逐文件 SHA-256 全在 oneasr-core 里（GPUI 壳用了很久），没有理由再写第二份。
 *
 * 契约（见 `oneasr-cli --help` 的 download 段落）：
 *   [download] state=<downloading|verifying|done|error> model=<id> done=<n> total=<n> pct=<n> [speed=<n>] [dir="<path>"]
 * 人类可读的双语行在 stderr，不参与解析。
 *
 * 一次只跑一个（GPUI 的 start_model_download 也是这个门禁）；取消 = kill 子进程，
 * 下载器写的是临时文件 + Range 续传，下次重跑会接着下。
 */
export class DownloadManager extends EventEmitter {
  private child: ChildProcess | null = null
  private current: DownloadState | null = null

  state(): DownloadState | null {
    return this.current
  }

  /** 上一次的进度（渲染层重新打开设置时要用）。 */
  isBusy(): boolean {
    return this.child !== null
  }

  start(model: string): { ok: boolean; code?: 'busy' | 'spawn' } {
    if (this.child) return { ok: false, code: 'busy' }
    const cli = resolveCli()
    const args = ['download', '--model', model, '--app-root', DEFAULT_APP_ROOT, '--data-root', DEFAULT_APP_ROOT]
    this.current = {
      model,
      state: 'verifying',
      doneBytes: 0,
      totalBytes: 0,
      pct: 0,
      dir: undefined
    }
    this.emit('updated', this.current)

    let child: ChildProcess
    try {
      child = spawn(cli, args, {
        cwd: DEFAULT_APP_ROOT,
        env: { ...process.env, ONEASR_DATA_DIR: DEFAULT_APP_ROOT },
        windowsHide: true
      })
    } catch (error) {
      logError(`download spawn failed: ${cli}`, error)
      this.current = { ...this.current, state: 'error', message: String(error) }
      this.emit('updated', this.current)
      return { ok: false, code: 'spawn' }
    }
    this.child = child

    const onLine = (buf: Buffer, stream: 'out' | 'err'): void => {
      for (const raw of buf.toString('utf8').split(/\r?\n/)) {
        const line = raw.trim()
        if (!line) continue
        if (stream === 'err') continue // 双语人读行不进 UI（UI 有自己的文案）
        const parsed = parseEvent(line)
        if (!parsed) continue
        this.current = { ...parsed, dir: parsed.dir ?? this.current?.dir }
        this.emit('updated', this.current)
      }
    }
    child.stdout?.on('data', (b) => onLine(b, 'out'))
    child.stderr?.on('data', (b) => onLine(b, 'err'))

    child.on('close', (code) => {
      this.child = null
      const prev = this.current
      if (prev && prev.state !== 'done' && prev.state !== 'error') {
        // 没收到终态就退出了：按退出码判定（0 = 已存在/完成，其它 = 失败/被取消）
        this.current = {
          ...prev,
          state: code === 0 ? 'done' : 'error',
          pct: code === 0 ? 100 : prev.pct,
          message: code === 0 ? undefined : `exit ${code}`
        }
        this.emit('updated', this.current)
      }
    })
    child.on('error', (error) => {
      this.child = null
      logError('download child error', error)
      if (this.current) {
        this.current = { ...this.current, state: 'error', message: String(error) }
        this.emit('updated', this.current)
      }
    })
    return { ok: true }
  }

  cancel(): void {
    if (!this.child) return
    try {
      this.child.kill('SIGTERM')
    } catch {
      /* ignore */
    }
    if (this.current) {
      this.current = { ...this.current, state: 'error', message: 'cancelled' }
      this.emit('updated', this.current)
    }
  }

  /** 退出前收尾，别留孤儿。 */
  shutdown(): void {
    if (this.child) {
      try {
        this.child.kill('SIGTERM')
        this.child.kill('SIGKILL')
      } catch {
        /* ignore */
      }
    }
    this.child = null
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
    dir: kv.get('dir')
  }
}

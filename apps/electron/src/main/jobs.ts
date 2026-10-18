import { ChildProcess, spawn } from 'child_process'
import fs from 'fs'
import path from 'path'
import { EventEmitter } from 'events'
import type {
  ProbeResult,
  Settings,
  StartResult,
  StatsSnapshot,
  StatusSnapshot,
  TaskSnapshot
} from '../shared/types'
import { DEFAULT_APP_ROOT, loadSettings, resolveCli } from './settings'
import { runCliJson } from './cli'
import { parseCliEvent } from './cli-events'
import { appendRecord, loadRecords, localDay, summarize } from './stats-ledger'
import { logError, logWarn, writeTaskLog } from './log'
import { loadHotwords, wordsCsv } from './hotword'
import {
  JobQueue,
  type KillCommand,
  type QueueCommand,
  type RunId,
  type SpawnCommand
} from './queue'
import { killProcessTree } from './kill-tree'

let seq = 0
function nextId(): string {
  seq += 1
  return `t-${Date.now()}-${seq}`
}

/** 时长 / 尺寸 / 媒体类型都问 CLI（判据在 core），壳不再自己 spawn ffmpeg 正则 Duration。 */
async function probeInput(file: string): Promise<ProbeResult | undefined> {
  const r = await runCliJson<ProbeResult>(['probe', '--input', file, '--json'], 30000)
  if (!r.ok) logWarn(`probe failed for ${file}: ${r.error ?? 'unknown'}`)
  return r.ok ? r.data : undefined
}

export class JobManager extends EventEmitter {
  private tasks: TaskSnapshot[] = []
  /**
   * 运行态（谁是 current、runId、是否在取消）**只存在状态机里**；jobs.ts 不再保存
   * running / runningId / busy。这里只留 runId → child 的句柄表，供执行 kill 用。
   */
  private readonly queue = new JobQueue<TaskSnapshot>()
  private readonly children = new Map<RunId, ChildProcess>()
  private sessionRecent: StatsSnapshot['recentSession'] = []
  private lastOutput?: string
  private lastDurationMs?: number

  list(): TaskSnapshot[] {
    return this.tasks.map((t) => {
      const qi = this.queue.queueRank(t.id)
      return { ...t, queueRank: t.status === 'queued' && qi !== undefined ? qi : undefined }
    })
  }

  /** 退出前把在跑的 CLI 收掉，别留孤儿进程。 */
  shutdown(): void {
    // 与取消路径共用同一条树杀：Windows 上第一次调用就已经 /T /F 收了整棵树，
    // 第二次调用只是"万一没死透"的兜底（不是唯一的树杀时机）
    this.children.forEach((child) => {
      killProcessTree(child, 'SIGTERM')
      killProcessTree(child, 'SIGKILL')
    })
    this.children.clear()
    this.queue.shutdown()
  }

  /** 行内改一项（运行中锁定，与 GPUI 的 locks_row_actions 一致）。 */
  private patch(id: string, fn: (t: TaskSnapshot) => void): TaskSnapshot[] {
    const t = this.tasks.find((x) => x.id === id)
    if (t && t.status !== 'running') {
      fn(t)
      this.emitUpdate()
    }
    return this.list()
  }

  /** 行内改识别语言。 */
  setLanguage(id: string, language: string): TaskSnapshot[] {
    return this.patch(id, (t) => {
      t.language = language
    })
  }

  /** 行内切人声分离。 */
  setVocalSeparation(id: string, on: boolean): TaskSnapshot[] {
    return this.patch(id, (t) => {
      t.vocalSeparation = on
    })
  }

  /** 行内绑热词组（null = 不绑）。 */
  setHotwordGroup(id: string, groupId: string | null): TaskSnapshot[] {
    return this.patch(id, (t) => {
      t.hotwordGroupId = groupId
    })
  }

  getStats(): StatsSnapshot {
    return summarize(loadRecords(), this.sessionRecent, {
      output: this.lastOutput,
      durationMs: this.lastDurationMs
    })
  }

  add(paths: string[]): TaskSnapshot[] {
    const settings = loadSettings()
    const hw = loadHotwords()
    const defaultGroup =
      hw.default_group_enabled && hw.default_group
        ? hw.default_group
        : null
    for (const p of paths) {
      if (!p || !fs.existsSync(p)) continue
      if (this.tasks.some((t) => t.input === p && t.status !== 'done' && t.status !== 'error' && t.status !== 'cancelled'))
        continue
      const task: TaskSnapshot = {
        id: nextId(),
        input: p,
        name: path.basename(p),
        status: 'queued',
        progress: '',
        progressPct: 0,
        hotwordGroupId: defaultGroup,
        language: settings.language || 'zh',
        vocalSeparation: !!settings.vocal_separation
      }
      this.tasks.push(task)
      // 尺寸/时长/类型后台问 CLI，拿到再刷新那一行（不阻塞加入）
      void probeInput(p).then((probe) => {
        if (!probe) return
        task.sizeBytes = probe.size_bytes
        task.durationSec = probe.duration_sec ?? undefined
        task.mediaKind = probe.kind
        this.emitUpdate()
      })
    }
    this.emitUpdate()
    return this.list()
  }

  remove(id: string): TaskSnapshot[] {
    if (this.queue.isRunning(id)) this.cancel(id)
    this.tasks = this.tasks.filter((t) => t.id !== id)
    this.queue.removeQueued(id)
    this.emitUpdate()
    return this.list()
  }

  /**
   * 清空列表——**正在跑的那条留下**（GPUI 的行为）。
   * 以前这里会顺手 cancel 掉运行中的任务，等于「清空」= 杀掉正在干的活。
   */
  clear(): TaskSnapshot[] {
    this.queue.clearQueued()
    this.tasks = this.tasks.filter((t) => t.status === 'running')
    this.emitUpdate()
    return this.list()
  }

  /**
   * 启动门禁：**问 CLI**（`status --json` + 这一行的运行开关），判据是 Rust 的
   * `Settings::can_start()`。壳里不再自己判断「模型目录够不够」——之前那套
   * 「目录非空即就绪」比 Rust 松，可能放行缺文件的目录。
   * 返回值里的 reason 是 Rust 给的双语文案，壳原样显示。
   */
  private async canStart(task: TaskSnapshot): Promise<{ ok: boolean; reason?: string }> {
    const settings = loadSettings()
    const args = ['status', '--json', '--language', task.language || settings.language || 'zh']
    if (task.vocalSeparation) args.push('--vocal-separation')
    if (task.hotwordGroupId) args.push('--hotword-group', task.hotwordGroupId)
    if (settings.output_txt) args.push('--txt')
    if (settings.output_ass) args.push('--ass')
    if (!settings.output_srt) args.push('--no-srt')
    const r = await runCliJson<StatusSnapshot>(args, 20000)
    if (!r.ok || !r.data) return { ok: false, reason: r.error ?? 'cli status failed' }
    return r.data.can_start.ok
      ? { ok: true }
      : { ok: false, reason: r.data.can_start.reason ?? undefined }
  }

  /**
   * 把这一行的运行参数（识别语言 / 词库 / 人声分离）推给所有其它行。
   *
   * 拖 20 个文件进来挨个点 20 次太蠢，而「顶部预设」解决不了"已经加进来了"这件事；
   * 一次点击把当前行当作模板，比多选批量更省事（运行中的行锁定，跳过）。
   */
  applyToAll(id: string): TaskSnapshot[] {
    const src = this.tasks.find((t) => t.id === id)
    if (src) {
      for (const t of this.tasks) {
        if (t.id === src.id || t.status === 'running') continue
        t.language = src.language
        t.hotwordGroupId = src.hotwordGroupId
        t.vocalSeparation = src.vocalSeparation
      }
      this.emitUpdate()
    }
    return this.list()
  }

  /**
   * 取消：排队中的行"撤下来"，运行中的行"杀掉"。
   * 运行中的行只发 SIGTERM 并标记意图；**下一条要等这次 run 的 close**（状态机保证）。
   */
  cancel(id: string): void {
    const t = this.tasks.find((x) => x.id === id)
    if (!t) return

    const result = this.queue.cancel(id)
    if (result.outcome === 'queued') {
      t.status = 'cancelled'
      t.progress = 'cancelled'
      this.emitUpdate()
      return
    }
    if (result.outcome !== 'running') return

    t.status = 'cancelled'
    t.progress = 'cancelled'
    t.finishedAt = Date.now()
    this.emitUpdate()
    this.runCommands(result.commands)
    // 定时器捕获这一次 run：到点时句柄表/状态机都还要对得上，否则什么都不做
    if (
      result.runId != null &&
      result.commands.some((c) => c.type === 'kill' && c.signal === 'SIGTERM')
    ) {
      this.armKillTimer(result.runId)
    }
  }

  /** 一行进入"排队"时的公共重置（startOne / startAll 共用）。 */
  private prepareQueued(t: TaskSnapshot): void {
    t.status = 'queued'
    t.progress = ''
    t.stage = undefined
    t.progressPct = 0
    t.error = undefined
    t.outputPath = undefined
    // 上一轮的耗时明细要清掉，否则重跑时「耗时」chip 先显示旧数字
    t.stageTimings = undefined
    t.totalMs = undefined
  }

  async startOne(id: string): Promise<StartResult> {
    const t = this.tasks.find((x) => x.id === id)
    if (!t || t.status === 'running') return { ok: false, code: 'nothing-to-start' }
    const gate = await this.canStart(t)
    if (!gate.ok) return { ok: false, code: 'cli', reason: gate.reason }
    this.prepareQueued(t)
    this.runCommands(this.queue.enqueue(t))
    this.emitUpdate()
    return { ok: true }
  }

  async startAll(): Promise<StartResult> {
    const candidates = this.tasks.filter((t) => t.status !== 'running')
    // 每行可能带着不同的语言/人声分离/热词组，门禁逐个问（并发，N 很小）
    const gates = await Promise.all(candidates.map((t) => this.canStart(t)))
    let firstReason: string | undefined
    const accepted: TaskSnapshot[] = []
    for (let i = 0; i < candidates.length; i++) {
      const t = candidates[i]
      if (!gates[i].ok) {
        firstReason ??= gates[i].reason
        continue
      }
      this.prepareQueued(t)
      accepted.push(t)
    }
    if (accepted.length === 0) {
      return firstReason != null
        ? { ok: false, code: 'cli', reason: firstReason }
        : { ok: false, code: 'nothing-to-start' }
    }
    for (const t of accepted) this.runCommands(this.queue.enqueue(t))
    this.emitUpdate()
    return { ok: true }
  }

  private emitUpdate(): void {
    this.emit('tasks', this.list())
  }

  /** 执行状态机吐出的命令：spawn 真正落地，kill 只认当前 run。 */
  private runCommands(commands: QueueCommand<TaskSnapshot>[]): void {
    for (const cmd of commands) {
      if (cmd.type === 'spawn') this.startRun(cmd)
      else this.killRun(cmd)
    }
  }

  /** kill 命令带 runId：换人后晚到的旧定时器/旧命令在这里被挡掉。 */
  private killRun(cmd: KillCommand): void {
    // 裁决在状态机里：runId 与当前 run 不符的命令直接作废，不落到任何 child 上
    const allowed = this.queue.resolveKill(cmd)
    if (!allowed) return
    const child = this.children.get(allowed.runId)
    if (!child) return
    killProcessTree(child, allowed.signal)
  }

  /** SIGTERM 后宽限期到点仍没 close 才 SIGKILL；定时器绑定这一次 run。 */
  private armKillTimer(runId: RunId): void {
    const child = this.children.get(runId)
    setTimeout(() => {
      if (!child || this.children.get(runId) !== child) return
      if (!this.queue.isCurrentRun(runId)) return
      this.runCommands(this.queue.tick())
    }, this.queue.killGraceMs)
  }

  private startRun(cmd: SpawnCommand<TaskSnapshot>): void {
    const { runId, task } = cmd
    // 排队后被撤下（或行被 clear/remove 掉）的条目不该真起进程：这次 run 立刻作废
    if (task.status === 'cancelled' || !this.tasks.includes(task)) {
      this.runCommands(this.queue.childClosed(runId))
      return
    }
    const settings = loadSettings()
    const appRoot = DEFAULT_APP_ROOT
    const cli = resolveCli(appRoot)
    // 模型目录的解析归 CLI（apply_app_root_paths / settings），壳不再自己拼候选
    const outDir = settings.save_next_to_source
      ? path.dirname(task.input)
      : settings.output_dir || path.join(appRoot, 'output')
    try {
      fs.mkdirSync(outDir, { recursive: true })
    } catch {
      /* ignore */
    }
    const stem = path.basename(task.input, path.extname(task.input))
    const primaryExt = settings.output_srt ? '.srt' : settings.output_txt ? '.txt' : '.ass'
    const defaultOut = path.join(appRoot, 'output', `${stem}${primaryExt}`)
    const desiredOut = path.join(outDir, `${stem}${primaryExt}`)
    const outputPath =
      path.resolve(desiredOut) === path.resolve(defaultOut) ? undefined : desiredOut

    const args = this.buildArgs(task, outputPath, settings, appRoot)

    task.status = 'running'
    task.startedAt = Date.now()
    task.progress = 'starting…'
    task.progressPct = 2
    this.emitUpdate()

    const cmdline = [cli, ...args].map((a) => (a.includes(' ') ? `"${a}"` : a)).join(' ')
    const child = spawn(cli, args, {
      cwd: appRoot,
      env: { ...process.env, ONEASR_DATA_DIR: appRoot },
      windowsHide: true,
      // POSIX：独立进程组，取消/退出时才能把 ffmpeg 这类后代一起收掉
      detached: process.platform !== 'win32'
    })
    this.children.set(runId, child)
    let stderr = ''
    let settled = false
    const stageTimings: { key?: string; stage: string; ms: number }[] = []
    /** CLI 结构化报错（error 事件）；失败摘要优先用它，见下面的收尾处理 */
    let eventError: string | undefined

    const onLine = (buf: Buffer, stream: 'out' | 'err'): void => {
      const text = buf.toString('utf8')
      if (stream === 'err') stderr += text
      for (const line of text.split(/\r?\n/)) {
        const trimmed = line.trim()
        if (!trimmed) continue
        if (stream === 'err') {
          // 人读行进 UI 的「原始输出」（阶段类噪音不会再出现：阶段走事件流）
          task.progress = trimmed.slice(0, 220)
          this.emit('progress', { id: task.id, line: trimmed })
          this.emitUpdate()
          continue
        }
        const ev = parseCliEvent(trimmed)
        if (!ev) continue
        switch (ev.event) {
          case 'stage':
            // key 是稳定标识（界面文案按它出），label 只是 CLI 那边的说法，留着兜底
            if (ev.key) task.stageKey = ev.key
            if (ev.label) {
              task.stage = ev.label
              this.emit('progress', { id: task.id, line: ev.label })
            }
            if (typeof ev.pct === 'number') task.progressPct = ev.pct
            break
          case 'timing':
            if (ev.label && typeof ev.ms === 'number') {
              stageTimings.push({ key: ev.key, stage: ev.label, ms: ev.ms })
            }
            break
          case 'total':
            if (typeof ev.ms === 'number') task.totalMs = ev.ms
            break
          case 'done':
            // CLI 说的产物路径优先；字面量路径由 CLI 决定（它就是写盘的那个）
            if (ev.output) task.outputPath = ev.output
            break
          case 'error':
            if (ev.message) {
              eventError = ev.message
              task.error = ev.message
            }
            break
        }
        this.emitUpdate()
      }
    }

    child.stdout?.on('data', (b) => onLine(b, 'out'))
    child.stderr?.on('data', (b) => onLine(b, 'err'))

    child.on('close', (code) => {
      if (settled) return
      settled = true
      this.children.delete(runId)
      const wasCancel = task.status === 'cancelled'
      task.finishedAt = Date.now()
      const durationMs = (task.finishedAt || 0) - (task.startedAt || 0)
      this.lastDurationMs = durationMs

      if (wasCancel) {
        this.sessionRecent.unshift({
          name: task.name,
          status: 'cancelled',
          durationMs,
          at: Date.now()
        })
        this.sessionRecent = this.sessionRecent.slice(0, 20)
        this.emitUpdate()
        this.runCommands(this.queue.childClosed(runId))
        return
      }

      const produced =
        [desiredOut, defaultOut, outputPath, path.join(outDir, `${stem}.srt`), path.join(appRoot, 'output', `${stem}.srt`)]
          .filter((p): p is string => !!p)
          .find((p) => fs.existsSync(p) && fs.statSync(p).size > 0) || undefined

      let cues = 0
      if (produced) {
        try {
          cues = fs
            .readFileSync(produced, 'utf8')
            .split(/\n/)
            .filter((l) => /^\d+$/.test(l.trim()) || (/-->/.test(l) === false && l.trim() && !/^\d+$/.test(l.trim()))).length
          // rough cue count for srt: count arrow lines
          const arrows = fs.readFileSync(produced, 'utf8').split(/\n/).filter((l) => l.includes('-->')).length
          if (arrows > 0) cues = arrows
        } catch {
          cues = 0
        }
      }

      if (stageTimings.length) task.stageTimings = stageTimings

      const ok = code === 0 && !!produced
      if (ok) {
        task.status = 'done'
        task.outputPath = produced
        task.progress = 'done'
        task.progressPct = 100
        task.stage = 'Done'
        this.lastOutput = produced
      } else {
        task.status = 'error'
        // 摘要是 CLI 的 `error` 事件（Rust 的双语文案，结构化）；
        // 原始 stderr 只在没有事件时兜底，并且始终作为「详情」保留给展开/复制。
        task.error = eventError ?? (stderr || `exit ${code}`).slice(-600)
        task.errorDetail = stderr.trim() ? stderr.trim().slice(-8000) : undefined
        task.progress = 'error'
        task.progressPct = 100
        // 失败的现场全部落盘：命令行、退出码、耗时、stderr 尾部（截 8 KB）
        logError(
          `task failed: ${task.name}\n  cmd: ${cmdline}\n  exit: ${code} · ${(durationMs / 1000).toFixed(1)}s · input=${task.input}\n  stderr:\n${stderr.slice(-8000)}`
        )
      }

      // media_sec 用加入时探到的时长：统计页的「处理媒体 / 节省时长 / 平均倍速」全靠它，
      // 之前这里一直没写，Electron 跑出来的任务在统计里等于没有媒体时长。
      try {
        appendRecord({
          v: 1,
          day: localDay(),
          process_ms: durationMs,
          media_sec: task.durationSec,
          lang: task.language || settings.language || 'zh',
          sep: !!(task.vocalSeparation ?? settings.vocal_separation),
          transcript: false,
          ok,
          cues
        })
      } catch {
        /* non-fatal */
      }

      // 成功时默认不留盘；要现场就 ONEASR_KEEP_TASK_LOG=1
      writeTaskLog(
        task.id,
        [
          `task: ${task.name}`,
          `input: ${task.input}`,
          `cmd: ${cmdline}`,
          `exit: ${code} · ${(durationMs / 1000).toFixed(1)}s`,
          `output: ${task.outputPath ?? '(none)'}`,
          `stages: ${(task.stageTimings ?? []).map((s) => `${s.stage}=${s.ms}ms`).join(', ') || '(none)'}`,
          'stderr:',
          stderr
        ].join('\n')
      )

      this.sessionRecent.unshift({
        name: task.name,
        status: task.status,
        durationMs,
        at: Date.now()
      })
      this.sessionRecent = this.sessionRecent.slice(0, 20)
      this.emitUpdate()
      this.emit('stats', this.getStats())
      this.runCommands(this.queue.childClosed(runId))
    })

    child.on('error', (err) => {
      if (settled) return
      settled = true
      this.children.delete(runId)
      task.status = 'error'
      task.error = String(err)
      logError(`task spawn failed: ${task.name}\n  cmd: ${cmdline}`, err)
      task.finishedAt = Date.now()
      this.emitUpdate()
      this.runCommands(this.queue.childClosed(runId))
    })
  }

  private buildArgs(
    task: TaskSnapshot,
    outputPath: string | undefined,
    settings: Settings,
    appRoot: string
  ): string[] {
    const args = [
      'transcribe',
      '--events=json',
      '--input',
      task.input,
      '--app-root',
      appRoot,
      '--data-root',
      appRoot,
      '--language',
      task.language || settings.language || 'zh',
      '--backend',
      settings.backend || 'auto',
      '--chunk-seconds',
      String(settings.chunk_target_seconds || 60)
    ]
    if (outputPath) args.push('--output', outputPath)

    // 直接透传设置里的模型 id：CLI 的 parse_asr_arg / parse_aligner_arg 既认友好别名
    // 也认完整目录名（目录名只有 catalog 一处真源）。以前壳里自己按字符串猜
    // 「1.7B + int8」「qwen → qwen」——那是规则，属于后端。
    if (settings.aligner_model) args.push('--aligner', settings.aligner_model)
    if (settings.asr_model) args.push('--asr', settings.asr_model)

    if (settings.output_txt) args.push('--txt')
    if (settings.output_ass) args.push('--ass')
    if (!settings.output_srt) args.push('--no-srt')
    if (settings.subtitle_beautify) args.push('--beautify')
    if (settings.text_script === 'simplified' || settings.text_script === 'traditional') {
      args.push('--script', settings.text_script)
    }
    if (task.vocalSeparation ?? settings.vocal_separation) {
      args.push('--vocal-separation')
      if (settings.demucs_model_dir) args.push('--demucs-model-dir', settings.demucs_model_dir)
    }

    // Hotwords: pass if CLI accepts (ignored if unknown on older builds — may error)
    // Prefer oneasr-cli-hotword when present and group selected
    if (task.hotwordGroupId) {
      const store = loadHotwords(appRoot)
      const g = store.groups.find((x) => x.id === task.hotwordGroupId)
      const csv = wordsCsv(g)
      if (csv) {
        // runtime CLI may ignore unknown flags; hotword binary is separate
        args.push('--hotwords', csv)
        if (settings.sensevoice_model_dir) {
          args.push('--sensevoice-model-dir', settings.sensevoice_model_dir)
        }
      }
    }
    return args
  }
}

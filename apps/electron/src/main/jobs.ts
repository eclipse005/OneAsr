import { ChildProcess, spawn } from 'child_process'
import fs from 'fs'
import path from 'path'
import { EventEmitter } from 'events'
import type {
  AddTaskInput,
  ProbeResult,
  Settings,
  StartResult,
  StatsSnapshot,
  StatusSnapshot,
  TaskSnapshot,
  TranscriptEditResult,
  TranscriptInfo
} from '../shared/types'
import { DEFAULT_APP_ROOT, dataRoot, loadSettings, outputFlags, resolveCli } from './settings'
import { cliEnv, runCliJson } from './cli'
import { parseCliEvent } from './cli-events'
import { LineBuffer } from '../shared/line-buffer'
import { createLimit } from '../shared/limit'
import { countCues } from '../shared/cue-count'
import { appendRecord, loadRecords, localDay, summarize } from './stats-ledger'
import { logError, logWarn, writeTaskLog } from './log'
import { m } from './i18n'
import { loadHotwords, newTaskGroup, wordsCsv } from './hotword'
import { resolveRunOutcome } from '../shared/run-result'
import { hotwordGroupForLanguage } from '../shared/language'
import {
  CTC_ALIGNER_ID,
  failedTranscriptInfo,
  inspectTranscriptFile,
  inspectTranscriptText,
  removeTempTranscript,
  splitTranscriptText,
  writeTempTranscript
} from './transcript'
import type { TranscriptReadResult } from './transcript'
import {
  JobQueue,
  type KillCommand,
  type QueueCommand,
  type RunId,
  type SpawnCommand
} from './queue'
import { killProcessTree } from './kill-tree'
import { isBulkStartCandidate } from '../shared/start-policy'

let seq = 0
function nextId(): string {
  seq += 1
  return `t-${Date.now()}-${seq}`
}

/** 任务挂着的文稿正文只留在主进程；快照里只放摘要。 */
interface StagedTranscript {
  /** 当前（可能被编辑）的规范化纯文本 */
  text: string
  /** 从源文件读进来的样子；用来判断要不要给 align 写临时文件 */
  originalText: string
  /** 上一次成功打轴用的文本；没跑过就是 undefined */
  alignedText?: string
  info: TranscriptInfo
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
  /** 批量导入的探针闸门：每个探针都会 spawn CLI + ffmpeg，不能无界并发。 */
  private readonly probeLimit = createLimit(2)
  /**
   * 正在等 `status` 门禁返回的启动。取消可能落在这个窗口里：此时任务既不在
   * 队列里也没有 child，`queue.cancel` 只能返回 absent。把它记下来，取消时
   * 直接把行标成 cancelled，门禁返回后 startOne/startAll 就不会再起进程。
   */
  private readonly starting = new Set<string>()
  /** task id → 文稿正文；任务移除 / 清空时同步删除 */
  private readonly transcripts = new Map<string, StagedTranscript>()
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
    // 与取消路径共用同一条树杀。Windows 上 taskkill /T /F 就是最终手段，
    // 发两遍只会对可能已死的 pid 再开一个进程 —— 合并成一次。
    this.children.forEach((child) => {
      if (process.platform === 'win32') {
        killProcessTree(child, 'SIGKILL')
        return
      }
      killProcessTree(child, 'SIGTERM')
      killProcessTree(child, 'SIGKILL')
    })
    this.children.clear()
    this.queue.shutdown()
  }

  /** 行内改一项（运行中锁定，与旧版一致）。 */
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
      // 热词只接中文与英文：换到别的语言时，绑定一起清成「无」，
      // 免得界面显示无热词、任务里却还挂着一个永远不生效的组。
      t.hotwordGroupId = hotwordGroupForLanguage(language, t.hotwordGroupId)
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
      // 语言不支持时把这次绑定也吞掉：IPC 可能来自一份过期快照，
      // 不能让界面的禁用成为唯一防线。
      t.hotwordGroupId = hotwordGroupForLanguage(t.language, groupId)
    })
  }

  getStats(): StatsSnapshot {
    return summarize(loadRecords(), this.sessionRecent, {
      output: this.lastOutput,
      durationMs: this.lastDurationMs
    })
  }

  async add(items: Array<string | AddTaskInput>): Promise<TaskSnapshot[]> {
    const settings = loadSettings()
    const hw = loadHotwords()
    const language = settings.language || 'zh'
    // 默认组没词时 newTaskGroup 返回 null：新任务从第一刻起就是「无热词」。
    const defaultGroup = newTaskGroup(hw)
    for (const raw of items) {
      const item: AddTaskInput = typeof raw === 'string' ? { media: raw } : raw
      const p = item.media
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
        // 默认语言本来就是中文/英文时才带默认组；日语等语言直接「无热词」。
        hotwordGroupId: hotwordGroupForLanguage(language, defaultGroup),
        language,
        vocalSeparation: !!settings.vocal_separation
      }
      if (item.transcript) {
        // 先读完再放进行列表：这一行从出现的第一刻起就是「可开始」的，
        // 不会出现点了开始才发现文稿还在读的竞态。
        const r = await inspectTranscriptFile(item.transcript)
        if (r.ok && r.text != null && r.info) {
          const info: TranscriptInfo = { ...r.info, needsRealign: false }
          task.transcript = info
          this.transcripts.set(task.id, {
            text: r.text,
            originalText: r.text,
            info
          })
        } else {
          task.transcript = failedTranscriptInfo(
            item.transcript,
            r.error ?? 'transcript read failed'
          )
        }
      }
      this.tasks.push(task)
      // 尺寸/时长/类型后台问 CLI，拿到再刷新那一行（不阻塞加入）。
      // 探针有上限：一次拖入几十个文件时不能同时起几十个 CLI + ffmpeg。
      void this.probeLimit(() => probeInput(p)).then((probe) => {
        if (!probe) return
        task.sizeBytes = probe.size_bytes
        task.durationSec = probe.duration_sec ?? undefined
        task.mediaKind = probe.kind
        if (task.transcript && !task.transcript.error) {
          void this.refreshTranscriptMeta(task)
        }
        this.emitUpdate()
      })
    }
    this.emitUpdate()
    return this.list()
  }

  /** 时长探到后补一次语速 / 行数；用当前暂存文本，编辑过的稿子也算得对。 */
  private async refreshTranscriptMeta(task: TaskSnapshot): Promise<void> {
    const staged = this.transcripts.get(task.id)
    if (!staged || !task.durationSec) return
    const r = await inspectTranscriptText(staged.text, staged.info.name, task.durationSec)
    if (!r.ok || !r.info) return
    staged.info = {
      ...staged.info,
      lines: r.info.lines,
      chars: r.info.chars,
      rate: r.info.rate
    }
    task.transcript = staged.info
    this.emitUpdate()
  }

  private transcriptLocked(task: TaskSnapshot): boolean {
    // 新加入的行也是 status=queued，但 queueRank 还是 undefined（待处理）；
    // 只有真的进了运行队列（或正在跑）才把这次输入钉死。
    return (
      task.status === 'running' ||
      (task.status === 'queued' && this.queue.queueRank(task.id) !== undefined)
    )
  }

  private transcriptNeedsRealign(
    task: TaskSnapshot,
    staged: StagedTranscript,
    text: string
  ): boolean {
    if (staged.alignedText != null) return text !== staged.alignedText
    return !!task.outputPath
  }

  getTranscript(id: string): TranscriptEditResult {
    const staged = this.transcripts.get(id)
    if (!staged) return { ok: false, error: m().noTranscript }
    return { ok: true, text: staged.text, info: staged.info }
  }

  /** 挂上 / 替换 / 移除一份文稿。path=null 表示移除。 */
  async setTranscript(id: string, file: string | null): Promise<TaskSnapshot[]> {
    const task = this.tasks.find((t) => t.id === id)
    if (!task || this.transcriptLocked(task)) return this.list()
    if (!file) {
      this.transcripts.delete(id)
      task.transcript = undefined
      this.emitUpdate()
      return this.list()
    }
    const r = await inspectTranscriptFile(file)
    if (!r.ok || r.text == null || !r.info) {
      this.transcripts.delete(id)
      task.transcript = failedTranscriptInfo(file, r.error ?? 'transcript read failed')
      this.emitUpdate()
      return this.list()
    }
    const info: TranscriptInfo = {
      ...r.info,
      edited: false,
      needsRealign: !!task.outputPath
    }
    this.transcripts.set(id, {
      text: r.text,
      originalText: r.text,
      info
    })
    task.transcript = info
    this.emitUpdate()
    return this.list()
  }

  /** 保存面板里的编辑；只改暂存，不写回原文件。 */
  async saveTranscript(id: string, text: string): Promise<TranscriptEditResult> {
    const task = this.tasks.find((t) => t.id === id)
    const staged = this.transcripts.get(id)
    if (!task || !staged || this.transcriptLocked(task)) {
      return { ok: false, error: m().transcriptUnavailable, tasks: this.list() }
    }
    const r = await inspectTranscriptText(text, staged.info.name, task.durationSec)
    if (!r.ok || r.text == null || !r.info) {
      return { ok: false, error: r.error, tasks: this.list() }
    }
    const info: TranscriptInfo = {
      ...staged.info,
      lines: r.info.lines,
      chars: r.info.chars,
      rate: r.info.rate,
      edited: r.text !== staged.originalText,
      needsRealign: this.transcriptNeedsRealign(task, staged, r.text)
    }
    staged.text = r.text
    staged.info = info
    task.transcript = info
    this.emitUpdate()
    return { ok: true, text: r.text, info, tasks: this.list() }
  }

  /** 智能断句：作用于当前暂存文本，结果仍只留在应用里。 */
  async splitTranscript(id: string): Promise<TranscriptEditResult> {
    const task = this.tasks.find((t) => t.id === id)
    const staged = this.transcripts.get(id)
    if (!task || !staged || this.transcriptLocked(task)) {
      return { ok: false, error: m().transcriptUnavailable, tasks: this.list() }
    }
    const settings = loadSettings()
    const r = await splitTranscriptText(
      staged.text,
      task.language || settings.language || 'zh',
      settings.subtitle_length_preset || 'standard',
      staged.info.name,
      task.durationSec
    )
    if (!r.ok || r.text == null || !r.info) {
      return { ok: false, error: r.error, tasks: this.list() }
    }
    const info: TranscriptInfo = {
      ...staged.info,
      lines: r.info.lines,
      chars: r.info.chars,
      rate: r.info.rate,
      edited: r.text !== staged.originalText,
      needsRealign: this.transcriptNeedsRealign(task, staged, r.text)
    }
    staged.text = r.text
    staged.info = info
    task.transcript = info
    this.emitUpdate()
    return { ok: true, text: r.text, info, tasks: this.list() }
  }

  /** 丢掉编辑，重新读源文件。 */
  async rereadTranscript(id: string): Promise<TranscriptEditResult> {
    const task = this.tasks.find((t) => t.id === id)
    const staged = this.transcripts.get(id)
    if (!task || !staged || this.transcriptLocked(task)) {
      return { ok: false, error: m().transcriptUnavailable, tasks: this.list() }
    }
    if (!staged.info.path) {
      return { ok: false, error: m().transcriptUnavailable, tasks: this.list() }
    }
    const r = await inspectTranscriptFile(staged.info.path, task.durationSec)
    if (!r.ok || r.text == null || !r.info) {
      return { ok: false, error: r.error, tasks: this.list() }
    }
    const info: TranscriptInfo = {
      ...r.info,
      edited: false,
      needsRealign: this.transcriptNeedsRealign(task, staged, r.text)
    }
    staged.text = r.text
    staged.originalText = r.text
    staged.info = info
    task.transcript = info
    this.emitUpdate()
    return { ok: true, text: r.text, info, tasks: this.list() }
  }

  removeTranscript(id: string): TaskSnapshot[] {
    const task = this.tasks.find((t) => t.id === id)
    if (!task || this.transcriptLocked(task)) return this.list()
    this.transcripts.delete(id)
    task.transcript = undefined
    this.emitUpdate()
    return this.list()
  }

  remove(id: string): TaskSnapshot[] {
    if (this.queue.isRunning(id)) this.cancel(id)
    this.tasks = this.tasks.filter((t) => t.id !== id)
    this.transcripts.delete(id)
    this.queue.removeQueued(id)
    this.emitUpdate()
    return this.list()
  }

  /**
   * 清空列表——**正在跑的那条留下**（旧版的行为）。
   * 以前这里会顺手 cancel 掉运行中的任务，等于「清空」= 杀掉正在干的活。
   */
  clear(): TaskSnapshot[] {
    this.queue.clearQueued()
    this.tasks = this.tasks.filter((t) => t.status === 'running')
    const keep = new Set(this.tasks.map((t) => t.id))
    for (const id of Array.from(this.transcripts.keys())) {
      if (!keep.has(id)) this.transcripts.delete(id)
    }
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
    const isTranscript = !!task.transcript
    if (isTranscript && task.transcript?.error) {
      return { ok: false, reason: task.transcript.error }
    }
    if (isTranscript && !this.transcripts.has(task.id)) {
      return { ok: false, reason: m().transcriptUnavailable }
    }
    const args = ['status', '--json', '--language', task.language || settings.language || 'zh']
    // 把这一趟真正会用的模型 id 显式告诉 CLI：没有 settings.json 时 status
    // 与 transcribe 都按这个 id 解析，避免一边扫描、一边按精确 id 起跑。
    if (settings.asr_model) args.push('--asr', settings.asr_model)
    if (isTranscript) {
      // 文稿任务固定 CTC；--transcript 让 status 提前检查「至少 SRT 或 ASS」。
      args.push('--transcript', '--aligner', CTC_ALIGNER_ID)
    } else {
      if (settings.aligner_model) args.push('--aligner', settings.aligner_model)
      if (task.hotwordGroupId) args.push('--hotword-group', task.hotwordGroupId)
    }
    if (task.vocalSeparation) args.push('--vocal-separation')
    if (settings.output_txt) args.push('--txt')
    if (settings.output_ass) args.push('--ass')
    if (!settings.output_srt) args.push('--no-srt')
    const r = await runCliJson<StatusSnapshot>(args, 20000)
    if (!r.ok || !r.data) return { ok: false, reason: r.error ?? m().cliStatusFailed }
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
        // 目标行换成源行的语言后，再决定热词绑定是否保留。
        t.hotwordGroupId = hotwordGroupForLanguage(t.language, src.hotwordGroupId)
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
    if (result.outcome !== 'running') {
      // 还在等启动门禁：没有队列项、没有 child，但这一行确实“正在开始”。
      // 标记取消，门禁返回后的复查会放弃启动。
      if (this.starting.has(id)) {
        t.status = 'cancelled'
        t.progress = 'cancelled'
        t.finishedAt = Date.now()
        this.emitUpdate()
      }
      return
    }

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
    t.stageKey = undefined
    t.stageChunk = undefined
    t.stageChunkTotal = undefined
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
    this.starting.add(id)
    try {
      const gate = await this.canStart(t)
      if (!gate.ok) return { ok: false, code: 'cli', reason: gate.reason }
      // gate 是异步的（spawn CLI）。等待期间这一行可能已经被另一个启动调用
      // 跑起来/排上队；此时不能再 prepareQueued，否则会把 running 重置成 queued。
      if (!this.tasks.includes(t)) return { ok: false, code: 'nothing-to-start' }
      if (this.queue.isActive(t.id)) return { ok: true }
      if (t.status === 'cancelled') return { ok: false, code: 'nothing-to-start' }
      this.prepareQueued(t)
      this.runCommands(this.queue.enqueue(t))
      this.emitUpdate()
      return { ok: true }
    } finally {
      this.starting.delete(id)
    }
  }

  async startAll(): Promise<StartResult> {
    // 完成和取消的行留在原地；已经在跑或已经排队的不再入队一次。
    const candidates = this.tasks.filter((t) =>
      isBulkStartCandidate(t.status, this.queue.queueRank(t.id) !== undefined)
    )
    for (const t of candidates) this.starting.add(t.id)
    try {
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
        // 与 startOne 同一个复查：等待门禁期间被另一条路径启动/取消的行跳过，
        // 不重置它的运行态。
        if (
          !this.tasks.includes(t) ||
          t.status === 'cancelled' ||
          this.queue.isActive(t.id)
        ) {
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
    } finally {
      for (const t of candidates) this.starting.delete(t.id)
    }
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
    const dataRootDir = dataRoot()
    const cli = resolveCli(appRoot)
    if (!cli) {
      // 没引擎就没得跑：当场把任务标成失败（带可搜的错因），不要开子进程等 ENOENT
      const message = `oneasr-cli 未找到（app root = ${appRoot}）`
      task.status = 'error'
      task.error = message
      task.finishedAt = Date.now()
      logError(`task ${task.name}: ${message}`)
      this.emitUpdate()
      this.runCommands(this.queue.childClosed(runId))
      return
    }
    const staged = this.transcripts.get(task.id)
    const transcriptRun = !!staged && !staged.info.error
    /**
     * 文稿有编辑时，align 读一份临时纯文本；没编辑就直接读源文件（保留
     * .srt 的来源信息）。临时文件在子进程收尾时删除。
     */
    let tempTranscript: string | undefined
    let args: string[]
    if (transcriptRun && staged) {
      const useSource = staged.text === staged.originalText && !!staged.info.path
      const textPath = useSource
        ? staged.info.path!
        : (tempTranscript = writeTempTranscript(staged.text))
      args = this.buildAlignArgs(task, settings, appRoot, dataRootDir, textPath)
    } else {
      args = this.buildArgs(task, settings, appRoot, dataRootDir)
    }
    const cleanupTempTranscript = (): void => {
      if (!tempTranscript) return
      removeTempTranscript(tempTranscript)
      tempTranscript = undefined
    }
    // 输出目录的最终决定权在 Rust（srt_target_dir）：壳只把 GUI 的两个字段
    // 原样转成旗标，不自己拼路径——否则 SRT 与 TXT/ASS 会落到两个目录。

    task.status = 'running'
    task.startedAt = Date.now()
    task.progress = 'starting…'
    task.progressPct = 2
    this.emitUpdate()

    const cmdline = [cli, ...args].map((a) => (a.includes(' ') ? `"${a}"` : a)).join(' ')
    const child = spawn(cli, args, {
      cwd: appRoot,
      env: cliEnv(dataRootDir),
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

    const stdoutLines = new LineBuffer()
    const onStdoutLine = (line: string): void => {
      const ev = parseCliEvent(line)
      if (!ev) return
      switch (ev.event) {
        case 'stage':
          // key 是稳定标识（界面文案按它出），label 只是 CLI 那边的说法，留着兜底
          if (ev.key) task.stageKey = ev.key
          if (ev.label) task.stage = ev.label
          if (typeof ev.pct === 'number') task.progressPct = ev.pct
          // 没带分段（换阶段，或只有一段）就把上一阶段的 2/5 清掉。
          if (
            typeof ev.chunk === 'number' &&
            typeof ev.chunkTotal === 'number' &&
            ev.chunkTotal > 1
          ) {
            task.stageChunk = ev.chunk
            task.stageChunkTotal = ev.chunkTotal
          } else {
            task.stageChunk = undefined
            task.stageChunkTotal = undefined
          }
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
      // 只在事件行刷新 UI；stderr 的人读行不再逐行触发一次全量下发。
      this.emitUpdate()
    }

    child.stdout?.on('data', (b) => stdoutLines.push(b.toString('utf8'), onStdoutLine))
    child.stderr?.on('data', (b) => {
      stderr += b.toString('utf8')
    })

    child.on('close', (code) => {
      if (settled) return
      settled = true
      // 最后一行可能没有换行结尾（CLI 正常 println 会带，但不能依赖它）。
      stdoutLines.flush(onStdoutLine)
      this.children.delete(runId)
      cleanupTempTranscript()
      // 取消意图属于**这次 run**，不属于会变的 task.status：取消后用户可能
      // 马上重开同一行，状态那时候已经是 queued 了。
      const wasCancel = this.queue.isCancelling(runId)
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
        this.emit('stats', this.getStats())
        this.runCommands(this.queue.childClosed(runId))
        return
      }

      // 产物路径只有一个出处：CLI 的 `done` 事件（core 决定文件名）。
      // 壳**不猜**——文稿匹配写的是 `<stem>.aligned.srt`，猜出来的 `<stem>.srt`
      // 既可能不存在，也可能命中上一轮遗留的同名文件（把旧结果当成本次的
      // 产物报出去）。判据见 shared/run-result.ts。
      const outcome = resolveRunOutcome(code, task.outputPath)
      const produced = outcome.ok ? outcome.outputPath : undefined

      let cues = 0
      if (produced) {
        try {
          cues = countCues(produced, fs.readFileSync(produced, 'utf8'))
        } catch {
          cues = 0
        }
      }

      if (stageTimings.length) task.stageTimings = stageTimings

      const ok = outcome.ok
      if (ok) {
        task.status = 'done'
        task.outputPath = produced
        task.progress = 'done'
        task.progressPct = 100
        task.stage = 'Done'
        task.stageChunk = undefined
        task.stageChunkTotal = undefined
        this.lastOutput = produced
        if (transcriptRun && staged) {
          staged.alignedText = staged.text
          staged.info = { ...staged.info, edited: false, needsRealign: false }
          task.transcript = staged.info
        }
      } else {
        task.status = 'error'
        // 摘要是 CLI 的 `error` 事件（Rust 的双语文案，结构化）；
        // 原始 stderr 只在没有事件时兜底，并且始终作为「详情」保留给展开/复制。
        task.error =
          outcome.reason === 'no-output'
            ? m().taskNoOutput
            : eventError ?? (stderr || `exit ${code}`).slice(-600)
        task.errorDetail = stderr.trim() ? stderr.trim().slice(-8000) : undefined
        task.progress = 'error'
        task.progressPct = 100
        task.stageChunk = undefined
        task.stageChunkTotal = undefined
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
          transcript: transcriptRun,
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
          `transcript: ${task.transcript ? `${task.transcript.name} (${task.transcript.lines} lines)` : '(none)'}`,
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
      cleanupTempTranscript()
      task.status = 'error'
      task.error = String(err)
      logError(`task spawn failed: ${task.name}\n  cmd: ${cmdline}`, err)
      task.finishedAt = Date.now()
      this.emitUpdate()
      this.runCommands(this.queue.childClosed(runId))
    })
  }

  /**
   * 文稿任务：`align` 跳过识别，文字来自文稿；对齐器固定 CTC（CLI 也固定）。
   * 热词 / ASR 模型 / 语言模型一概不传——它们对这条路径没有消费者。
   */
  private buildAlignArgs(
    task: TaskSnapshot,
    settings: Settings,
    appRoot: string,
    dataRootDir: string,
    textPath: string
  ): string[] {
    const args = [
      'align',
      '--events=json',
      '--audio',
      task.input,
      '--text',
      textPath,
      '--app-root',
      appRoot,
      '--data-root',
      dataRootDir,
      '--language',
      task.language || settings.language || 'zh',
      '--backend',
      settings.backend || 'auto',
      '--chunk-seconds',
      String(settings.chunk_target_seconds || 60),
      // GUI 不消费 .timeline.json，align 也不需要边车文件。
      '--no-timeline'
    ]
    if (
      settings.text_script === 'original' ||
      settings.text_script === 'simplified' ||
      settings.text_script === 'traditional'
    ) {
      args.push('--script', settings.text_script)
    }
    if (settings.output_txt) args.push('--txt')
    if (settings.output_ass) args.push('--ass')
    if (!settings.output_srt) args.push('--no-srt')
    if (settings.subtitle_beautify) args.push('--beautify')
    args.push(...outputFlags(settings))
    if (task.vocalSeparation ?? settings.vocal_separation) {
      args.push('--vocal-separation')
      if (settings.demucs_model_dir) args.push('--demucs-model-dir', settings.demucs_model_dir)
    }
    return args
  }

  private buildArgs(
    task: TaskSnapshot,
    settings: Settings,
    appRoot: string,
    dataRootDir: string
  ): string[] {
    const args = [
      'transcribe',
      '--events=json',
      '--input',
      task.input,
      '--app-root',
      appRoot,
      '--data-root',
      dataRootDir,
      '--language',
      task.language || settings.language || 'zh',
      '--backend',
      settings.backend || 'auto',
      '--chunk-seconds',
      String(settings.chunk_target_seconds || 60),
      // GUI 不消费 .timeline.json；TXT-only 时也不该因此被迫加载对齐器。
      '--no-timeline'
    ]
    if (
      settings.subtitle_length_preset === 'short' ||
      settings.subtitle_length_preset === 'standard' ||
      settings.subtitle_length_preset === 'loose'
    ) {
      args.push('--preset', settings.subtitle_length_preset)
    }

    // 直接透传设置里的模型 id：CLI 的 parse_asr_arg / parse_aligner_arg 既认友好别名
    // 也认完整目录名（目录名只有 catalog 一处真源）。以前壳里自己按字符串猜
    // 「1.7B + int8」「qwen → qwen」——那是规则，属于后端。
    if (settings.aligner_model) args.push('--aligner', settings.aligner_model)
    if (settings.asr_model) args.push('--asr', settings.asr_model)

    if (settings.output_txt) args.push('--txt')
    if (settings.output_ass) args.push('--ass')
    if (!settings.output_srt) args.push('--no-srt')
    args.push(...outputFlags(settings))
    if (settings.subtitle_beautify) args.push('--beautify')
    if (
      settings.text_script === 'original' ||
      settings.text_script === 'simplified' ||
      settings.text_script === 'traditional'
    ) {
      args.push('--script', settings.text_script)
    }
    if (task.vocalSeparation ?? settings.vocal_separation) {
      args.push('--vocal-separation')
      if (settings.demucs_model_dir) args.push('--demucs-model-dir', settings.demucs_model_dir)
    }

    // Hotwords: pass if CLI accepts (ignored if unknown on older builds — may error)
    // Prefer oneasr-cli-hotword when present and group selected
    if (task.hotwordGroupId) {
      const store = loadHotwords(dataRootDir)
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

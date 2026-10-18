import { ChildProcess, spawn } from 'child_process'
import fs from 'fs'
import path from 'path'
import { EventEmitter } from 'events'
import type {
  Settings,
  StartBlockCode,
  StartResult,
  StatsSnapshot,
  TaskSnapshot
} from '../shared/types'
import {
  DEFAULT_APP_ROOT,
  checkModels,
  loadSettings,
  resolveAsrDir,
  resolveCli
} from './settings'
import { appendRecord, loadRecords, localDay, summarize } from './stats-ledger'
import { logError, logWarn, writeTaskLog } from './log'
import { loadHotwords, wordsCsv } from './hotword'

let seq = 0
function nextId(): string {
  seq += 1
  return `t-${Date.now()}-${seq}`
}

/**
 * CLI 的阶段表：中英两套名字都要认。
 * 之前只认 `[stage]` 和英文阶段名，而 CLI 的输出语言跟随系统（本机是中文），
 * 于是阶段药丸和百分比**一个都出不来**——这次按语言无关的方式重写。
 */
const STAGES: Array<[string, string]> = [
  // 顺序与 crates/oneasr-core 的 AsrStage::label 一致（9 个，其中
  // 「人声分离」「热词提取」是条件阶段，只有开关/绑了热词组时才出现）
  ['converting audio', '转码音频'],
  ['separating vocals', '人声分离'],
  ['planning segments', '分段规划'],
  ['extracting hotwords', '热词提取'],
  ['loading asr model', '加载识别模型'],
  ['transcribing', '转写'],
  ['loading aligner model', '加载对齐模型'],
  ['aligning', '打轴'],
  ['exporting subtitles', '导出字幕']
]

function stageIndex(name: string): number {
  const lower = name.toLowerCase()
  return STAGES.findIndex(([en, zh]) => lower.includes(en) || name.includes(zh))
}

/** `[阶段] 转写` / `[stage] Transcribing` → 阶段名 + 百分比。 */
function parseStage(line: string): { stage?: string; pct?: number } {
  const m = line.match(/\[(?:stage|阶段)\]\s*(.+)/i)
  if (m) {
    const stage = m[1].trim()
    const idx = stageIndex(stage)
    return { stage, pct: idx >= 0 ? Math.round(((idx + 1) / STAGES.length) * 100) : undefined }
  }
  if (/(?:OK|完成)\s+output=/i.test(line)) return { stage: 'Done', pct: 100 }
  return {}
}

/** 收尾的耗时行：`  转写             587 ms` / `  加载识别模型          5.4 s`。 */
function parseTiming(line: string): { stage: string; ms: number } | undefined {
  const m = line.match(/^\s{2,}(\S.*?)\s+([\d.]+)\s*(ms|s)$/)
  if (!m) return undefined
  const stage = m[1].trim()
  if (stageIndex(stage) < 0) return undefined
  const value = Number(m[2])
  if (!Number.isFinite(value)) return undefined
  return { stage, ms: m[3] === 's' ? Math.round(value * 1000) : Math.round(value) }
}

/** `wall=12.8s total_ms=12828 stages=7` */
function parseTotalMs(line: string): number | undefined {
  const m = line.match(/total_ms=(\d+)/)
  return m ? Number(m[1]) : undefined
}

/** 时长探测用捆绑的 ffmpeg：`{app_root}/bin/ffmpeg[.exe]`（与 Rust 侧同一约定）。 */
function resolveFfmpeg(appRoot = DEFAULT_APP_ROOT): string | undefined {
  const name = process.platform === 'win32' ? 'ffmpeg.exe' : 'ffmpeg'
  const p = path.join(appRoot, 'bin', name)
  return fs.existsSync(p) ? p : undefined
}

/** 只读文件头探时长（不解码）：ffmpeg 把 `Duration: 00:00:12.34` 打在 stderr。 */
function probeDurationSec(file: string, appRoot = DEFAULT_APP_ROOT): Promise<number | undefined> {
  const ffmpeg = resolveFfmpeg(appRoot)
  if (!ffmpeg) return Promise.resolve(undefined)
  return new Promise((resolve) => {
    let done = false
    let timer: NodeJS.Timeout | undefined
    const child = spawn(ffmpeg, ['-hide_banner', '-i', file], { stdio: ['ignore', 'ignore', 'pipe'] })
    let err = ''
    const finish = (value?: number): void => {
      if (done) return
      done = true
      if (timer) clearTimeout(timer)
      try {
        child.kill()
      } catch {
        /* ignore */
      }
      resolve(value)
    }
    const read = (): void => {
      const m = err.match(/Duration:\s*(\d+):(\d+):(\d+(?:\.\d+)?)/)
      if (m) finish(Number(m[1]) * 3600 + Number(m[2]) * 60 + Number(m[3]))
    }
    child.stderr?.on('data', (b) => {
      err += b.toString()
      read()
    })
    child.on('error', () => finish())
    child.on('close', () => {
      read()
      finish()
    })
    timer = setTimeout(() => finish(), 15000)
  })
}

export class JobManager extends EventEmitter {
  private tasks: TaskSnapshot[] = []
  private running: ChildProcess | null = null
  private runningId: string | null = null
  private queue: string[] = []
  private busy = false
  private sessionRecent: StatsSnapshot['recentSession'] = []
  private lastOutput?: string
  private lastDurationMs?: number

  list(): TaskSnapshot[] {
    return this.tasks.map((t) => {
      const qi = this.queue.indexOf(t.id)
      return { ...t, queueRank: t.status === 'queued' && qi >= 0 ? qi + 1 : undefined }
    })
  }

  /** 退出前把在跑的 CLI 收掉，别留孤儿进程。 */
  shutdown(): void {
    if (this.running) {
      try {
        this.running.kill('SIGTERM')
      } catch {
        /* ignore */
      }
      try {
        this.running.kill('SIGKILL')
      } catch {
        /* ignore */
      }
    }
    this.running = null
    this.runningId = null
    this.queue = []
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
      let sizeBytes: number | undefined
      try {
        sizeBytes = fs.statSync(p).size
      } catch {
        /* ignore */
      }
      const task: TaskSnapshot = {
        id: nextId(),
        input: p,
        name: path.basename(p),
        status: 'queued',
        progress: '',
        progressPct: 0,
        hotwordGroupId: defaultGroup,
        language: settings.language || 'zh',
        vocalSeparation: !!settings.vocal_separation,
        sizeBytes
      }
      this.tasks.push(task)
      // 时长后台探，探到再刷新那一行（不阻塞加入）
      void probeDurationSec(p).then((sec) => {
        if (sec == null) return
        task.durationSec = sec
        this.emitUpdate()
      })
    }
    this.emitUpdate()
    return this.list()
  }

  remove(id: string): TaskSnapshot[] {
    if (this.runningId === id) this.cancel(id)
    this.tasks = this.tasks.filter((t) => t.id !== id)
    this.queue = this.queue.filter((q) => q !== id)
    this.emitUpdate()
    return this.list()
  }

  /**
   * 清空列表——**正在跑的那条留下**（GPUI 的行为）。
   * 以前这里会顺手 cancel 掉运行中的任务，等于「清空」= 杀掉正在干的活。
   */
  clear(): TaskSnapshot[] {
    this.queue = []
    this.tasks = this.tasks.filter((t) => t.status === 'running')
    this.emitUpdate()
    return this.list()
  }

  /**
   * 启动前的门禁：能跑就返回 undefined，否则给出原因码。
   * 挡在「点开始」这一刻，比失败的 stderr 铺到卡片上再让人猜要好。
   */
  private blockReason(task: TaskSnapshot, models: ReturnType<typeof checkModels>): StartBlockCode | undefined {
    if (!models.ready) return 'models'
    if (task.vocalSeparation && !models.demucsReady) return 'sep-weights'
    if (task.hotwordGroupId) {
      const group = loadHotwords().groups.find((g) => g.id === task.hotwordGroupId)
      if (!group || !wordsCsv(group).trim()) return 'hotwords-empty'
    }
    return undefined
  }

  cancel(id: string): void {
    const t = this.tasks.find((x) => x.id === id)
    if (!t) return
    this.queue = this.queue.filter((q) => q !== id)
    if (this.runningId === id && this.running) {
      try {
        this.running.kill('SIGTERM')
        setTimeout(() => {
          try {
            this.running?.kill('SIGKILL')
          } catch {
            /* ignore */
          }
        }, 1500)
      } catch {
        /* ignore */
      }
      t.status = 'cancelled'
      t.progress = 'cancelled'
      t.finishedAt = Date.now()
      this.running = null
      this.runningId = null
      this.busy = false
      this.emitUpdate()
      void this.pump()
      return
    }
    if (t.status === 'queued') {
      t.status = 'cancelled'
      t.progress = 'cancelled'
      this.emitUpdate()
    }
  }

  startOne(id: string): StartResult {
    const t = this.tasks.find((x) => x.id === id)
    if (!t || t.status === 'running') return { ok: false, code: 'nothing-to-start' }
    const blocked = this.blockReason(t, checkModels(loadSettings()))
    if (blocked) return { ok: false, code: blocked }
    t.status = 'queued'
    t.progress = ''
    t.stage = undefined
    t.progressPct = 0
    t.error = undefined
    t.outputPath = undefined
    // 上一轮的耗时明细要清掉，否则重跑时「耗时」chip 先显示旧数字
    t.stageTimings = undefined
    t.totalMs = undefined
    if (!this.queue.includes(id)) this.queue.push(id)
    this.emitUpdate()
    void this.pump()
    return { ok: true }
  }

  startAll(): StartResult {
    const models = checkModels(loadSettings())
    let firstBlock: StartBlockCode | undefined
    let queued = 0
    for (const t of this.tasks) {
      if (t.status === 'running') continue
      const blocked = this.blockReason(t, models)
      if (blocked) {
        firstBlock ??= blocked
        continue
      }
      t.status = 'queued'
      t.progress = ''
      t.stage = undefined
      t.progressPct = 0
      t.error = undefined
      t.outputPath = undefined
      t.stageTimings = undefined
      t.totalMs = undefined
      if (!this.queue.includes(t.id)) this.queue.push(t.id)
      queued += 1
    }
    if (queued === 0) return { ok: false, code: firstBlock ?? 'nothing-to-start' }
    this.emitUpdate()
    void this.pump()
    return { ok: true }
  }

  private emitUpdate(): void {
    this.emit('tasks', this.list())
  }

  private async pump(): Promise<void> {
    if (this.busy) return
    const id = this.queue.shift()
    if (!id) return
    const task = this.tasks.find((t) => t.id === id)
    if (!task || task.status === 'cancelled') {
      void this.pump()
      return
    }
    this.busy = true
    await this.runTask(task)
    this.busy = false
    void this.pump()
  }

  private runTask(task: TaskSnapshot): Promise<void> {
    return new Promise((resolve) => {
      const settings = loadSettings()
      const appRoot = DEFAULT_APP_ROOT
      const cli = resolveCli(appRoot)
      const asrDir = resolveAsrDir(settings, appRoot)
      if (asrDir !== settings.asr_model_dir) {
        settings.asr_model_dir = asrDir
        settings.asr_model = path.basename(asrDir)
      }

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
      this.runningId = task.id
      this.emitUpdate()

      const cmdline = [cli, ...args].map((a) => (a.includes(' ') ? `"${a}"` : a)).join(' ')
      const child = spawn(cli, args, {
        cwd: appRoot,
        env: { ...process.env, ONEASR_DATA_DIR: appRoot }
      })
      this.running = child
      let stderr = ''
      const stageTimings: { stage: string; ms: number }[] = []

      const onLine = (buf: Buffer, stream: 'out' | 'err'): void => {
        const text = buf.toString('utf8')
        if (stream === 'err') stderr += text
        for (const line of text.split(/\r?\n/)) {
          const trimmed = line.trim()
          if (!trimmed) continue
          task.progress = trimmed.slice(0, 220)
          const { stage, pct } = parseStage(trimmed)
          if (stage) task.stage = stage
          if (pct != null) task.progressPct = pct
          // 用未 trim 的原始行：耗时行的缩进（两空格起）正是它与普通日志的区别
          const timing = parseTiming(line)
          if (timing) stageTimings.push(timing)
          const totalMs = parseTotalMs(trimmed)
          if (totalMs != null) task.totalMs = totalMs
          this.emit('progress', { id: task.id, line: trimmed })
          this.emitUpdate()
        }
      }

      child.stdout?.on('data', (b) => onLine(b, 'out'))
      child.stderr?.on('data', (b) => onLine(b, 'err'))

      child.on('close', (code) => {
        const wasCancel = task.status === 'cancelled'
        this.running = null
        this.runningId = null
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
          resolve()
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
          task.error = (stderr || `exit ${code}`).slice(-600)
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
        resolve()
      })

      child.on('error', (err) => {
        this.running = null
        this.runningId = null
        task.status = 'error'
        task.error = String(err)
        logError(`task spawn failed: ${task.name}\n  cmd: ${cmdline}`, err)
        task.finishedAt = Date.now()
        this.emitUpdate()
        resolve()
      })
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

    const aligner = settings.aligner_model || ''
    args.push('--aligner', aligner.toLowerCase().includes('qwen') ? 'qwen' : 'ctc')

    const asr = settings.asr_model || ''
    if (asr.includes('1.7B') && asr.includes('int8')) args.push('--asr', 'Qwen3-ASR-1.7B-int8')
    else if (asr.includes('1.7B')) args.push('--asr', '1.7b')
    else if (asr.includes('int8')) args.push('--asr', 'Qwen3-ASR-0.6B-int8')
    else args.push('--asr', '0.6b')

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

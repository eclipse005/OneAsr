import { ChildProcess, spawn } from 'child_process'
import fs from 'fs'
import path from 'path'
import { EventEmitter } from 'events'
import type { Settings, StatsSnapshot, TaskSnapshot } from '../shared/types'
import { DEFAULT_APP_ROOT, loadSettings, resolveAsrDir } from './settings'
import { appendRecord, loadRecords, localDay, summarize } from './stats-ledger'
import { loadHotwords, wordsCsv } from './hotword'

let seq = 0
function nextId(): string {
  seq += 1
  return `t-${Date.now()}-${seq}`
}

function parseStage(line: string): { stage?: string; pct?: number } {
  const m = line.match(/\[stage\]\s*(.+)/i)
  if (m) {
    const stage = m[1].trim()
    const order = [
      'Converting audio',
      'Planning segments',
      'Loading ASR model',
      'Transcribing',
      'Loading aligner model',
      'Aligning',
      'Exporting subtitles'
    ]
    const idx = order.findIndex((s) => stage.toLowerCase().includes(s.toLowerCase().split(' ')[0]!))
    const pct = idx >= 0 ? Math.round(((idx + 1) / order.length) * 100) : undefined
    return { stage, pct }
  }
  if (/OK output=/i.test(line)) return { stage: 'Done', pct: 100 }
  return {}
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
    return this.tasks.map((t) => ({ ...t }))
  }

  getStats(): StatsSnapshot {
    return summarize(loadRecords(), this.sessionRecent, {
      output: this.lastOutput,
      durationMs: this.lastDurationMs
    })
  }

  add(paths: string[]): TaskSnapshot[] {
    const hw = loadHotwords()
    const defaultGroup =
      hw.default_group_enabled && hw.default_group
        ? hw.default_group
        : null
    for (const p of paths) {
      if (!p || !fs.existsSync(p)) continue
      if (this.tasks.some((t) => t.input === p && t.status !== 'done' && t.status !== 'error' && t.status !== 'cancelled'))
        continue
      this.tasks.push({
        id: nextId(),
        input: p,
        name: path.basename(p),
        status: 'queued',
        progress: '',
        progressPct: 0,
        hotwordGroupId: defaultGroup
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

  clear(): TaskSnapshot[] {
    this.queue = []
    if (this.runningId) this.cancel(this.runningId)
    this.tasks = this.tasks.filter((t) => t.status === 'running')
    if (!this.busy) this.tasks = []
    this.emitUpdate()
    return this.list()
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

  startOne(id: string): void {
    const t = this.tasks.find((x) => x.id === id)
    if (!t || t.status === 'running') return
    t.status = 'queued'
    t.progress = ''
    t.stage = undefined
    t.progressPct = 0
    t.error = undefined
    t.outputPath = undefined
    if (!this.queue.includes(id)) this.queue.push(id)
    this.emitUpdate()
    void this.pump()
  }

  startAll(): void {
    for (const t of this.tasks) {
      if (t.status === 'running') continue
      t.status = 'queued'
      t.progress = ''
      t.stage = undefined
      t.progressPct = 0
      t.error = undefined
      t.outputPath = undefined
      if (!this.queue.includes(t.id)) this.queue.push(t.id)
    }
    this.emitUpdate()
    void this.pump()
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
      const cli = path.join(appRoot, 'oneasr-cli')
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

      const child = spawn(cli, args, {
        cwd: appRoot,
        env: { ...process.env, ONEASR_DATA_DIR: appRoot }
      })
      this.running = child
      let stderr = ''

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
        }

        // media duration unknown cheaply — leave undefined; process_ms recorded
        try {
          appendRecord({
            v: 1,
            day: localDay(),
            process_ms: durationMs,
            lang: settings.language || 'zh',
            sep: !!settings.vocal_separation,
            transcript: false,
            ok,
            cues
          })
        } catch {
          /* non-fatal */
        }

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
      settings.language || 'zh',
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
    if (settings.vocal_separation) {
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

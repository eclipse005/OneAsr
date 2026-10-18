import { ChildProcess, spawn } from 'child_process'
import fs from 'fs'
import path from 'path'
import { EventEmitter } from 'events'
import type { Settings, StatsSnapshot, TaskSnapshot, TaskStatus } from '../shared/types'
import { DEFAULT_APP_ROOT, loadSettings, resolveAsrDir } from './settings'

let seq = 0
function nextId(): string {
  seq += 1
  return `t-${Date.now()}-${seq}`
}

export class JobManager extends EventEmitter {
  private tasks: TaskSnapshot[] = []
  private running: ChildProcess | null = null
  private queue: string[] = []
  private busy = false
  private stats: StatsSnapshot = {
    totalRuns: 0,
    successCount: 0,
    errorCount: 0,
    recent: []
  }

  list(): TaskSnapshot[] {
    return this.tasks.map((t) => ({ ...t }))
  }

  getStats(): StatsSnapshot {
    return { ...this.stats, recent: [...this.stats.recent] }
  }

  add(paths: string[]): TaskSnapshot[] {
    for (const p of paths) {
      if (!p || !fs.existsSync(p)) continue
      if (this.tasks.some((t) => t.input === p && t.status !== 'done' && t.status !== 'error')) continue
      this.tasks.push({
        id: nextId(),
        input: p,
        name: path.basename(p),
        status: 'queued',
        progress: ''
      })
    }
    this.emitUpdate()
    return this.list()
  }

  remove(id: string): TaskSnapshot[] {
    this.tasks = this.tasks.filter((t) => t.id !== id)
    this.queue = this.queue.filter((q) => q !== id)
    this.emitUpdate()
    return this.list()
  }

  clear(): TaskSnapshot[] {
    if (this.busy) {
      // keep running task, clear others
      this.tasks = this.tasks.filter((t) => t.status === 'running')
      this.queue = []
    } else {
      this.tasks = []
      this.queue = []
    }
    this.emitUpdate()
    return this.list()
  }

  startOne(id: string): void {
    const t = this.tasks.find((x) => x.id === id)
    if (!t || t.status === 'running') return
    t.status = 'queued'
    t.progress = ''
    t.error = undefined
    t.outputPath = undefined
    if (!this.queue.includes(id)) this.queue.push(id)
    this.emitUpdate()
    void this.pump()
  }

  startAll(): void {
    for (const t of this.tasks) {
      if (t.status === 'queued' || t.status === 'error' || t.status === 'done') {
        if (t.status === 'done' || t.status === 'error') {
          t.status = 'queued'
          t.progress = ''
          t.error = undefined
          t.outputPath = undefined
        }
        if (!this.queue.includes(t.id)) this.queue.push(t.id)
      }
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
    if (!task) {
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
      // Persist healed path so CLI picks the same weights
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
      // Default CLI write is {data-root}/output/{stem}.srt. Passing the same path as
      // --output can truncate the file during copy — only pass --output when distinct.
      const defaultOut = path.join(appRoot, 'output', `${stem}${primaryExt}`)
      const desiredOut = path.join(outDir, `${stem}${primaryExt}`)
      const outputPath = path.resolve(desiredOut) === path.resolve(defaultOut) ? undefined : desiredOut

      const args = this.buildArgs(task.input, outputPath, settings, appRoot)

      task.status = 'running'
      task.startedAt = Date.now()
      task.progress = 'starting…'
      this.emitUpdate()

      const child = spawn(cli, args, {
        cwd: appRoot,
        env: {
          ...process.env,
          ONEASR_DATA_DIR: appRoot,
          ONEASR_PIPELINE_TRACE: process.env.ONEASR_PIPELINE_TRACE || '0'
        }
      })
      this.running = child
      let stderr = ''

      const onLine = (buf: Buffer, stream: 'out' | 'err'): void => {
        const text = buf.toString('utf8')
        if (stream === 'err') stderr += text
        for (const line of text.split(/\r?\n/)) {
          const trimmed = line.trim()
          if (!trimmed) continue
          task.progress = trimmed.slice(0, 200)
          this.emit('progress', { id: task.id, line: trimmed })
          this.emitUpdate()
        }
      }

      child.stdout?.on('data', (b) => onLine(b, 'out'))
      child.stderr?.on('data', (b) => onLine(b, 'err'))

      child.on('close', (code) => {
        this.running = null
        task.finishedAt = Date.now()
        const durationMs = (task.finishedAt || 0) - (task.startedAt || 0)
        this.stats.totalRuns += 1
        this.stats.lastDurationMs = durationMs

        const produced =
          [desiredOut, defaultOut, outputPath, path.join(outDir, `${stem}.srt`), path.join(appRoot, 'output', `${stem}.srt`)]
            .filter((p): p is string => !!p)
            .find((p) => fs.existsSync(p) && fs.statSync(p).size > 0) || undefined

        if (code === 0 && produced) {
          task.status = 'done'
          task.outputPath = produced
          task.progress = 'done'
          this.stats.successCount += 1
          this.stats.lastOutput = produced
        } else if (code === 0) {
          // CLI may write only under data-root/output
          const fallback = path.join(appRoot, 'output', `${stem}.srt`)
          if (fs.existsSync(fallback)) {
            task.status = 'done'
            task.outputPath = fallback
            task.progress = 'done'
            this.stats.successCount += 1
            this.stats.lastOutput = fallback
          } else {
            task.status = 'error'
            task.error = 'CLI exited 0 but output file missing'
            task.progress = 'error'
            this.stats.errorCount += 1
          }
        } else {
          task.status = 'error'
          task.error = (stderr || `exit ${code}`).slice(-500)
          task.progress = 'error'
          this.stats.errorCount += 1
        }

        this.stats.recent.unshift({
          name: task.name,
          status: task.status as TaskStatus,
          durationMs,
          at: Date.now()
        })
        this.stats.recent = this.stats.recent.slice(0, 20)
        this.emitUpdate()
        resolve()
      })

      child.on('error', (err) => {
        this.running = null
        task.status = 'error'
        task.error = String(err)
        task.finishedAt = Date.now()
        this.stats.totalRuns += 1
        this.stats.errorCount += 1
        this.emitUpdate()
        resolve()
      })
    })
  }

  private buildArgs(
    input: string,
    outputPath: string | undefined,
    settings: Settings,
    appRoot: string
  ): string[] {
    const args = [
      'transcribe',
      '--input',
      input,
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
    if (outputPath) {
      args.push('--output', outputPath)
    }

    // Aligner
    const aligner = settings.aligner_model || ''
    if (aligner.toLowerCase().includes('qwen')) {
      args.push('--aligner', 'qwen')
    } else {
      args.push('--aligner', 'ctc')
    }

    // ASR size hint when CLI supports --asr (ignored if unknown on older builds)
    const asr = settings.asr_model || ''
    if (asr.includes('1.7B')) args.push('--asr', '1.7b')
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
      if (settings.demucs_model_dir) {
        args.push('--demucs-model-dir', settings.demucs_model_dir)
      }
    }
    return args
  }
}

import fs from 'fs'
import os from 'os'
import path from 'path'
import { app } from 'electron'

/**
 * 诊断日志：`{app_root}/oneasr-error.log`，超过 2 MB 轮转到 `.old`。
 *
 * 与 GPUI 版（crates/oneasr/src/crashlog.rs）同一套口径：
 * - **会话上下文先缓存在内存里**，只有真的出问题才连同正文一起落盘 ——
 *   一次正常启动不该在日志里留东西，但真出问题时「机器/路径/模型状态」必须已经在那儿。
 * - 失败的任务把命令行、退出码、耗时、stderr 尾部写进去（GPUI 的 task journal 同义）。
 *   成功时不写；想留成功现场就设 ONEASR_KEEP_TASK_LOG=1，落 `oneasr-task-<id>.log`。
 */
const LOG_NAME = 'oneasr-error.log'
const TASK_LOG_PREFIX = 'oneasr-task-'
const MAX_BYTES = 2 * 1024 * 1024

let logFile: string | undefined
let appRootDir = ''
let sessionContext = ''
let contextWritten = false

function describe(error: unknown): string {
  if (error instanceof Error) return `${error.message}\n${error.stack ?? ''}`
  return String(error)
}

/** dev 下 app.getVersion() 给的是 Electron 版本，这里读本应用的 package.json。 */
function appVersion(): string {
  try {
    const pkg = JSON.parse(
      fs.readFileSync(path.join(app.getAppPath(), 'package.json'), 'utf8')
    ) as { version?: string }
    return pkg.version ?? app.getVersion()
  } catch {
    return app.getVersion()
  }
}

function buildContext(root: string): string {
  const lines = [
    '=== OneAsr (Electron) session ===',
    `time:      ${new Date().toISOString()}`,
    `app:       ${appVersion()} (electron ${process.versions.electron}, chrome ${process.versions.chrome}, node ${process.versions.node})`,
    `platform:  ${process.platform} ${process.arch} · ${os.release()}`,
    `cpu/ram:   ${os.cpus()[0]?.model ?? '?'} · ${Math.round(os.totalmem() / 1024 ** 3)} GB`,
    `app_root:  ${root}`,
    `log:       ${path.join(root, LOG_NAME)}`
  ]
  return lines.join('\n')
}

/** 启动时调用一次：定位日志、铺垫会话上下文（此时不写盘）。 */
export function initLog(appRoot: string): void {
  appRootDir = appRoot
  logFile = path.join(appRoot, LOG_NAME)
  sessionContext = buildContext(appRoot)
}

/** 往会话上下文里补一段（例如模型就绪情况、GPU 信息），出问题时会一起落盘。 */
export function addSessionContext(section: string, details: string): void {
  sessionContext += `\n--- ${section} ---\n${details}`
}

export function logPathname(): string | undefined {
  return logFile
}

function rotateIfNeeded(): void {
  if (!logFile) return
  try {
    if (fs.existsSync(logFile) && fs.statSync(logFile).size > MAX_BYTES) {
      fs.renameSync(logFile, `${logFile}.old`)
    }
  } catch {
    /* 轮转失败就让文件继续长，不能因此挡住写日志 */
  }
}

function append(level: string, message: string): void {
  if (!logFile) return
  try {
    rotateIfNeeded()
    const context = contextWritten ? '' : `${sessionContext}\n`
    contextWritten = true
    fs.appendFileSync(logFile, `${context}${new Date().toISOString()} [${level}] ${message}\n`, 'utf8')
  } catch {
    /* 日志写不进去也不能影响主流程 */
  }
}

export function logWarn(message: string): void {
  append('WARN', message)
}

export function logError(message: string, error?: unknown): void {
  append('ERROR', error === undefined ? message : `${message}\n${describe(error)}`)
}

/** 成功任务的可选现场（ONEASR_KEEP_TASK_LOG=1）。 */
export function writeTaskLog(id: string, body: string): void {
  if (!appRootDir) return
  if (!process.env.ONEASR_KEEP_TASK_LOG) return
  try {
    const file = path.join(appRootDir, `${TASK_LOG_PREFIX}${id}.log`)
    fs.writeFileSync(file, `${sessionContext}\n\n${body}\n`, 'utf8')
  } catch {
    /* ignore */
  }
}

/** 主进程级的兜底：未捕获异常与未处理的 Promise 拒绝都要留现场。 */
export function installProcessHandlers(): void {
  process.on('uncaughtException', (error) => {
    append('PANIC', `uncaughtException\n${describe(error)}`)
  })
  process.on('unhandledRejection', (reason) => {
    append('ERROR', `unhandledRejection\n${describe(reason)}`)
  })
}

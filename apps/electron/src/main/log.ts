import fs from 'fs'
import os from 'os'
import path from 'path'
import { app } from 'electron'
import {
  ERROR_LOG_NAME,
  clearDiagnosticLogs,
  diagnosticLogInfo,
  diagnosticLogPaths,
  diagnosticLogToOpen,
  type DiagnosticLogInfo
} from './log-files'

/**
 * 诊断日志：`{app_root}/oneasr-error.log`，超过 2 MB 轮转到 `.old`。
 *
 * 与旧版 crash log 同一套口径：
 * - **会话上下文先缓存在内存里**，只有真的出问题才连同正文一起落盘 ——
 *   一次正常启动不该在日志里留东西，但真出问题时「机器/路径/模型状态」必须已经在那儿。
 * - 失败的任务把命令行、退出码、耗时、stderr 尾部写进去（与旧版 task journal 同义）。
 *   成功时不写；想留成功现场就设 ONEASR_KEEP_TASK_LOG=1，落 `oneasr-task-<id>.log`。
 */
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
    `log:       ${path.join(root, ERROR_LOG_NAME)}`
  ]
  return lines.join('\n')
}

/** 启动时调用一次：定位日志、铺垫会话上下文（此时不写盘）。 */
export function initLog(appRoot: string): void {
  appRootDir = appRoot
  try {
    fs.mkdirSync(appRoot, { recursive: true })
  } catch {
    /* 日志目录建不出来也不能挡住启动 */
  }
  logFile = diagnosticLogPaths(appRoot)[0]
  sessionContext = buildContext(appRoot)
}

/** 往会话上下文里补一段（例如模型就绪情况、GPU 信息），出问题时会一起落盘。 */
export function addSessionContext(section: string, details: string): void {
  sessionContext += `\n--- ${section} ---\n${details}`
}

/** 设置页显示的大小：当前日志 + 轮转后的 `.old`。 */
export function logInfo(): Pick<DiagnosticLogInfo, 'bytes'> {
  if (!appRootDir) return { bytes: 0 }
  return { bytes: diagnosticLogInfo(appRootDir).bytes }
}

/**
 * 删掉当前日志和 `.old`。当前文件没了就忘掉「会话头已写过」，
 * 下一笔错误仍会带上机器和路径。
 */
export function clearDiagnosticLog(): Pick<DiagnosticLogInfo, 'bytes'> {
  if (!appRootDir) return { bytes: 0 }
  const info = clearDiagnosticLogs(appRootDir)
  if (!info.currentExists) contextWritten = false
  return { bytes: info.bytes }
}

/** 打开按钮：当前文件优先，否则上一份轮转。都没有就 undefined，调用方改开目录。 */
export function logFileToOpen(): string | undefined {
  if (!appRootDir) return undefined
  return diagnosticLogToOpen(appRootDir)
}

function rotateIfNeeded(): void {
  if (!logFile || !appRootDir) return
  try {
    const [current, old] = diagnosticLogPaths(appRootDir)
    if (fs.existsSync(current) && fs.statSync(current).size > MAX_BYTES) {
      fs.renameSync(current, old)
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
    // 加了 handler 后 Node 不再按默认行为退出；继续跑只会在未知状态下
    // 产生更多错误。留完现场就退出，交给用户重启。
    app.exit(1)
  })
  process.on('unhandledRejection', (reason) => {
    append('ERROR', `unhandledRejection\n${describe(reason)}`)
  })
}

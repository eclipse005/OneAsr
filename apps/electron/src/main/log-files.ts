import fs from 'fs'
import path from 'path'

/**
 * 诊断日志落在 `{app_root}/oneasr-error.log`。
 * 超过 2 MB 时 `log.ts` 把它改名为同目录的 `.old`，所以界面上的
 * 「大小」和「清理」都对着这一对，不碰可选的 `oneasr-task-*.log`。
 */
export const ERROR_LOG_NAME = 'oneasr-error.log'

export interface DiagnosticLogInfo {
  /** 当前文件与 `.old` 的字节和。没有文件时是 0。 */
  bytes: number
  /** 当前这份还在不在。不在时下一笔错误要重新写会话头。 */
  currentExists: boolean
}

/** 当前日志在前，轮转后的上一份在后。 */
export function diagnosticLogPaths(appRoot: string): [string, string] {
  const current = path.join(appRoot, ERROR_LOG_NAME)
  return [current, `${current}.old`]
}

function statFile(file: string): { exists: boolean; bytes: number } {
  try {
    const st = fs.statSync(file)
    if (!st.isFile()) return { exists: false, bytes: 0 }
    return { exists: true, bytes: st.size }
  } catch {
    return { exists: false, bytes: 0 }
  }
}

export function diagnosticLogInfo(appRoot: string): DiagnosticLogInfo {
  const [current, old] = diagnosticLogPaths(appRoot)
  const head = statFile(current)
  const rotated = statFile(old)
  return { bytes: head.bytes + rotated.bytes, currentExists: head.exists }
}

/** 打开按钮：优先当前文件，没有就打开上一份轮转。 */
export function diagnosticLogToOpen(appRoot: string): string | undefined {
  for (const file of diagnosticLogPaths(appRoot)) {
    if (statFile(file).exists) return file
  }
  return undefined
}

/** 删掉当前日志和 `.old`。删不掉的留下，返回值是清理后的实测大小。 */
export function clearDiagnosticLogs(appRoot: string): DiagnosticLogInfo {
  for (const file of diagnosticLogPaths(appRoot)) {
    try {
      fs.rmSync(file, { force: true })
    } catch {
      /* 占用或权限问题：下面再量一次，界面按剩下的大小显示 */
    }
  }
  return diagnosticLogInfo(appRoot)
}

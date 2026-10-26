import fs from 'fs'
import { spawn } from 'child_process'
import { DEFAULT_APP_ROOT, dataRoot, resolveCli } from './settings'
import { logError } from './log'
import { mainLang } from './i18n'
import type { CliCapabilities } from '../shared/types'

/**
 * 所有 CLI 子进程共用的环境：数据目录 + 界面语言。
 *
 * `ONEASR_UI_LANG` 让 CLI 的运行期错误/摘要与界面语言一致；没有它，
 * 中文系统 + 英文界面时错误会以系统语言出现。
 */
export function cliEnv(root = dataRoot()): NodeJS.ProcessEnv {
  return {
    ...process.env,
    ONEASR_DATA_DIR: root,
    ONEASR_UI_LANG: mainLang()
  }
}

/**
 * 调 oneasr-cli 并解析它 **stdout 上的 JSON**。
 *
 * 架构约定：Electron 只是壳，业务规则（就绪判据、路径解析、阶段编号、
 * 时长探测…）全部归 CLI / oneasr-core。壳只做「呈现 + 转发」，
 * 所以这里只认 JSON，不去正则人读文案。
 */
export interface CliResult<T> {
  ok: boolean
  data?: T
  error?: string
}

export function runCliJson<T>(args: string[], timeoutMs = 60000): Promise<CliResult<T>> {
  return new Promise((resolve) => {
    const cli = resolveCli()
    let child
    try {
      child = spawn(cli, args, {
        cwd: DEFAULT_APP_ROOT,
        env: cliEnv(),
        // Windows 上不加这个每次 spawn 都会闪一个控制台窗口
        //（probe/status/transcribe/download 都走这里）
        windowsHide: true
      })
    } catch (error) {
      logError(`cli spawn failed: ${cli} ${args.join(' ')}`, error)
      resolve({ ok: false, error: String(error) })
      return
    }
    let stdout = ''
    let stderr = ''
    let settled = false
    const timer = setTimeout(() => {
      if (settled) return
      settled = true
      try {
        child.kill('SIGTERM')
        child.kill('SIGKILL')
      } catch {
        /* ignore */
      }
      resolve({ ok: false, error: `cli timeout after ${timeoutMs}ms` })
    }, timeoutMs)

    child.stdout?.on('data', (b) => {
      stdout += b.toString('utf8')
    })
    child.stderr?.on('data', (b) => {
      stderr += b.toString('utf8')
    })
    child.on('error', (error) => {
      if (settled) return
      settled = true
      clearTimeout(timer)
      logError(`cli error: ${cli} ${args.join(' ')}`, error)
      resolve({ ok: false, error: String(error) })
    })
    child.on('close', (code) => {
      if (settled) return
      settled = true
      clearTimeout(timer)
      const text = stdout.trim()
      if (code !== 0) {
        resolve({ ok: false, error: (stderr || `exit ${code}`).trim().slice(-600) })
        return
      }
      try {
        resolve({ ok: true, data: JSON.parse(text) as T })
      } catch {
        resolve({ ok: false, error: `cli did not return JSON: ${text.slice(0, 200)}` })
      }
    })
  })
}

export function runCliCapture(
  args: string[],
  timeoutMs = 8000
): Promise<{ code: number | null; stdout: string; stderr: string }> {
  return new Promise((resolve) => {
    const cli = resolveCli()
    let child
    try {
      child = spawn(cli, args, {
        cwd: DEFAULT_APP_ROOT,
        env: cliEnv(),
        windowsHide: true
      })
    } catch (error) {
      resolve({ code: -1, stdout: '', stderr: String(error) })
      return
    }
    let stdout = ''
    let stderr = ''
    let settled = false
    const timer = setTimeout(() => {
      if (settled) return
      settled = true
      try {
        child.kill('SIGTERM')
        child.kill('SIGKILL')
      } catch {
        /* ignore */
      }
      resolve({ code: -1, stdout, stderr: stderr || `cli timeout after ${timeoutMs}ms` })
    }, timeoutMs)
    child.stdout?.on('data', (b) => {
      stdout += b.toString('utf8')
    })
    child.stderr?.on('data', (b) => {
      stderr += b.toString('utf8')
    })
    child.on('error', (error) => {
      if (settled) return
      settled = true
      clearTimeout(timer)
      resolve({ code: -1, stdout, stderr: String(error) })
    })
    child.on('close', (code) => {
      if (settled) return
      settled = true
      clearTimeout(timer)
      resolve({ code, stdout, stderr })
    })
  })
}

/** spawn 失败 / 路径不存在 → 未找到引擎，不是「过旧」。 */
export function isCliUnreachable(code: number | null, stderr: string): boolean {
  if (code !== -1 && code !== null) return false
  return /ENOENT/i.test(stderr) || /spawn .* ENOENT/i.test(stderr) || /not found/i.test(stderr)
}

/**
 * 紧探测 transcript：认 `transcript inspect`（缺 --input 的报错）或 help 里的真子命令行。
 * 不要认裸 `transcript` 词（help 里的 transcript_exts / --transcript 会误伤）。
 */
export function transcriptCapabilityFromOutput(stdout: string, stderr: string): boolean {
  const text = `${stdout}\n${stderr}`
  if (/unknown command:\s*transcript/i.test(text)) return false
  // 真子命令：缺参时会要 --input
  if (/missing required --input/i.test(text)) return true
  // help 用法行写明 transcript inspect
  if (/transcript\s+inspect/.test(text)) return true
  if (stdout.trim().startsWith('{')) return true
  return false
}

export function statusCapabilityFromOutput(
  code: number | null,
  stdout: string,
  stderr: string
): boolean {
  const text = `${stdout}\n${stderr}`
  if (/unknown command:\s*status/i.test(text)) return false
  if (isCliUnreachable(code, stderr)) return false
  return code === 0 || stdout.trim().startsWith('{')
}

/**
 * 探测壳依赖的 CLI 子命令是否存在。
 * - 二进制缺失 / spawn ENOENT → missing（不是 outdated）
 * - 能跑但缺 status / transcript inspect → outdated
 */
let capsCache: CliCapabilities | null = null

export async function probeCliCapabilities(): Promise<CliCapabilities> {
  if (capsCache) return capsCache
  const cliPath = resolveCli()
  if (!fs.existsSync(cliPath)) {
    capsCache = { status: false, transcript: false, missing: true, outdated: false }
    return capsCache
  }

  const help = await runCliCapture(['help'], 6000)
  if (isCliUnreachable(help.code, help.stderr)) {
    capsCache = { status: false, transcript: false, missing: true, outdated: false }
    return capsCache
  }

  const helpText = `${help.stdout}\n${help.stderr}`
  let statusOk = /\bstatus\b/.test(helpText) && !/unknown command:\s*status/i.test(helpText)
  let transcriptOk = /transcript\s+inspect/.test(helpText)

  if (!statusOk) {
    const r = await runCliCapture(['status', '--json'], 8000)
    if (isCliUnreachable(r.code, r.stderr)) {
      capsCache = { status: false, transcript: false, missing: true, outdated: false }
      return capsCache
    }
    statusOk = statusCapabilityFromOutput(r.code, r.stdout, r.stderr)
  }

  if (!transcriptOk) {
    // 紧探测：真子命令，不要裸 `transcript`（旧逻辑把 help / ENOENT 都当成有）
    const r = await runCliCapture(['transcript', 'inspect'], 6000)
    if (isCliUnreachable(r.code, r.stderr)) {
      capsCache = { status: false, transcript: false, missing: true, outdated: false }
      return capsCache
    }
    transcriptOk = transcriptCapabilityFromOutput(r.stdout, r.stderr)
  }

  capsCache = {
    status: statusOk,
    transcript: transcriptOk,
    missing: false,
    outdated: !(statusOk && transcriptOk)
  }
  return capsCache
}

/** 测试或换 CLI 路径后清缓存。 */
export function clearCliCapabilitiesCache(): void {
  capsCache = null
}

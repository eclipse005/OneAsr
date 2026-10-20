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

/**
 * 探测壳依赖的 CLI 子命令是否存在。
 * 不要把 `--help` 里的碎片当错误：只认「unknown command」或帮助正文里有没有该子命令。
 */
let capsCache: CliCapabilities | null = null

export async function probeCliCapabilities(): Promise<CliCapabilities> {
  if (capsCache) return capsCache
  const help = await runCliCapture(['help'], 6000)
  const text = `${help.stdout}\n${help.stderr}`
  let statusOk = /\bstatus\b/.test(text) && !/unknown command:\s*status/i.test(text)
  let transcriptOk =
    /transcript\s+inspect/.test(text) ||
    (/\btranscript\b/.test(text) && !/unknown command:\s*transcript/i.test(text))
  if (!statusOk) {
    const r = await runCliCapture(['status', '--json'], 8000)
    const err = `${r.stdout}\n${r.stderr}`
    statusOk =
      !/unknown command:\s*status/i.test(err) &&
      (r.code === 0 || r.stdout.trim().startsWith('{'))
  }
  if (!transcriptOk) {
    const r = await runCliCapture(['transcript'], 6000)
    const err = `${r.stdout}\n${r.stderr}`
    transcriptOk = !/unknown command:\s*transcript/i.test(err)
  }
  capsCache = {
    status: statusOk,
    transcript: transcriptOk,
    outdated: !(statusOk && transcriptOk)
  }
  return capsCache
}

/** 测试或换 CLI 路径后清缓存。 */
export function clearCliCapabilitiesCache(): void {
  capsCache = null
}

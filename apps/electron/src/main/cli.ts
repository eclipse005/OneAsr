import { spawn } from 'child_process'
import { DEFAULT_APP_ROOT, dataRoot, resolveCli } from './settings'
import { logError } from './log'
import { mainLang } from './i18n'

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

/**
 * 没找到 CLI 时的统一答复：一句能直接搜到原因的话，不冒充“CLI 跑挂了”。
 * 界面不展示它（用户修不了），但会进 oneasr-error.log 与返回给调用方的 error。
 */
function missingCliMessage(): string {
  return `oneasr-cli 未找到（app root = ${DEFAULT_APP_ROOT}）`
}

export function runCliJson<T>(args: string[], timeoutMs = 60000): Promise<CliResult<T>> {
  return new Promise((resolve) => {
    const cli = resolveCli()
    if (!cli) {
      const error = missingCliMessage()
      logError(`${error}：${args.join(' ')}`)
      resolve({ ok: false, error })
      return
    }
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

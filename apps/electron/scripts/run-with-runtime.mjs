/**
 * npm run start / dev 入口：未设置 ONEASR_APP_ROOT 时指向本机 runtime。
 *
 * 开发期仓库根常有 bin/ffmpeg 软链却没有 oneasr-cli；smoke 脚本一直有
 * ONEASR_APP_ROOT=/workspace/oneasr-runtime，这里与之对齐，避免状态栏假「CLI 过旧」。
 *
 * 用法：
 *   node scripts/run-with-runtime.mjs start
 *   node scripts/run-with-runtime.mjs dev
 *   node scripts/run-with-runtime.mjs preview
 */
import { spawn } from 'child_process'
import fs from 'fs'
import path from 'path'
import { fileURLToPath } from 'url'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const electronRoot = path.resolve(__dirname, '..')
const repoRoot = path.resolve(electronRoot, '../..')
const mode = process.argv[2] || 'start'

function pickDefaultAppRoot() {
  if (process.env.ONEASR_APP_ROOT) return process.env.ONEASR_APP_ROOT
  const candidates = [
    path.join(path.dirname(repoRoot), 'oneasr-runtime'),
    '/workspace/oneasr-runtime',
    repoRoot
  ]
  for (const root of candidates) {
    const cli = path.join(root, process.platform === 'win32' ? 'oneasr-cli.exe' : 'oneasr-cli')
    if (fs.existsSync(cli)) return root
  }
  return candidates[0]
}

const env = {
  ...process.env,
  ONEASR_APP_ROOT: pickDefaultAppRoot()
}

const npmCmd = process.platform === 'win32' ? 'npm.cmd' : 'npm'
const script =
  mode === 'dev' ? 'dev:raw' : mode === 'preview' ? 'preview:raw' : mode === 'start' ? 'start:raw' : null

if (!script) {
  console.error(`Usage: node scripts/run-with-runtime.mjs <start|dev|preview>`)
  process.exit(2)
}

const child = spawn(npmCmd, ['run', script], {
  cwd: electronRoot,
  env,
  stdio: 'inherit',
  shell: process.platform === 'win32'
})

child.on('exit', (code, signal) => {
  if (signal) process.kill(process.pid, signal)
  process.exit(code ?? 1)
})

/**
 * Launch the built Electron app briefly on $DISPLAY and exit 0 if it stays up.
 */
import { spawn } from 'child_process'
import path from 'path'
import { fileURLToPath } from 'url'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const root = path.resolve(__dirname, '..')
const electronBin = path.join(root, 'node_modules', 'electron', 'cli.js')
const mainJs = path.join(root, 'out', 'main', 'index.js')

const env = {
  ...process.env,
  DISPLAY: process.env.DISPLAY || ':4',
  ELECTRON_DISABLE_GPU: '1',
  ELECTRON_OZONE_PLATFORM_HINT: 'x11',
  ONEASR_APP_ROOT: process.env.ONEASR_APP_ROOT || '/workspace/oneasr-runtime'
}

const child = spawn(process.execPath, [electronBin, mainJs], {
  cwd: root,
  env,
  stdio: ['ignore', 'pipe', 'pipe']
})

let stderr = ''
let intentionalStop = false
child.stderr.on('data', (d) => {
  stderr += d.toString()
})
child.stdout.on('data', (d) => {
  process.stdout.write(d)
})

const waitMs = Number(process.env.LAUNCH_WAIT_MS || 8000)
const timer = setTimeout(() => {
  intentionalStop = true
  console.log(`PASS: Electron stayed up ${waitMs}ms on DISPLAY=${env.DISPLAY}`)
  child.kill('SIGTERM')
  setTimeout(() => {
    if (!child.killed) child.kill('SIGKILL')
  }, 2000)
}, waitMs)

child.on('exit', (code, signal) => {
  clearTimeout(timer)
  if (intentionalStop) {
    process.exit(0)
  }
  console.error('FAIL: Electron exited early', { code, signal })
  console.error(stderr.slice(-2000))
  process.exit(1)
})

#!/usr/bin/env node
import { spawn } from 'child_process'
import path from 'path'
import fs from 'fs'
import { fileURLToPath } from 'url'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const root = path.resolve(__dirname, '..')
const shotDir = process.env.ONEASR_SHOT_DIR || path.join(root, 'shots')
fs.mkdirSync(shotDir, { recursive: true })

const electronBin = path.join(root, 'node_modules', 'electron', 'cli.js')
const mainJs = path.join(root, 'out', 'main', 'index.js')

const env = {
  ...process.env,
  DISPLAY: process.env.DISPLAY || ':4',
  ELECTRON_DISABLE_GPU: '1',
  // app root 交给主进程的 resolveAppRoot()（唯一决策点）；想指定就 export。
  ONEASR_AUTO_SHOT: '1',
  ONEASR_AUTO_SHOT_EXIT: '1',
  ONEASR_SHOT_DIR: shotDir
}
// shotDir already applied above; keep env consistent


console.log('Taking shots →', shotDir)
const child = spawn(process.execPath, [electronBin, mainJs], {
  cwd: root,
  env,
  stdio: ['ignore', 'pipe', 'pipe']
})
child.stdout.on('data', (d) => process.stdout.write(d))
child.stderr.on('data', (d) => process.stderr.write(d))
child.on('exit', (code) => {
  const files = fs.readdirSync(shotDir).filter((f) => f.endsWith('.png')).sort()
  console.log('files:', files)
  process.exit(code === 0 || files.length >= 5 ? 0 : code || 1)
})

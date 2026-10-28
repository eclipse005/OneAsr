import { spawn } from 'child_process'
import fs from 'fs'
import path from 'path'

// 不猜路径：app root 与样本音频都由调用方指定。
const appRoot = process.env.ONEASR_APP_ROOT
if (!appRoot) {
  console.error(
    '需要 ONEASR_APP_ROOT：指向包含 oneasr-cli 的目录。例：\n' +
      '  ONEASR_APP_ROOT=/path/to/oneasr-runtime node scripts/smoke-transcribe.mjs'
  )
  process.exit(2)
}
const input = process.env.ONEASR_SMOKE_INPUT
if (!input) {
  console.error('需要 ONEASR_SMOKE_INPUT：要转写的音频文件路径')
  process.exit(2)
}
const cli = path.join(appRoot, 'oneasr-cli')
const outDir = path.join(appRoot, 'output')
fs.mkdirSync(outDir, { recursive: true })
const stem = path.basename(input, path.extname(input))
// Avoid --output === default SRT path: CLI copy can truncate the source to 0 bytes.
const output = path.join(outDir, `${stem}.electron-smoke.srt`)
if (fs.existsSync(output)) fs.unlinkSync(output)
const defaultSrt = path.join(outDir, `${stem}.srt`)
if (fs.existsSync(defaultSrt)) fs.unlinkSync(defaultSrt)

const settings = JSON.parse(fs.readFileSync(path.join(appRoot, 'settings.json'), 'utf8'))
const language = settings.language === 'zh' ? 'en' : settings.language || 'en'
// Librispeech sample is English
const args = [
  'transcribe',
  '--input', input,
  '--app-root', appRoot,
  '--data-root', appRoot,
  '--language', 'en',
  '--backend', 'cpu',
  '--aligner', 'ctc',
  '--asr', 'Qwen3-ASR-0.6B-int8',
  '--chunk-seconds', String(settings.chunk_target_seconds || 60),
  '--output', output
]

console.log('CMD:', cli, args.join(' '))
const t0 = Date.now()
const child = spawn(cli, args, { cwd: appRoot, env: { ...process.env, ONEASR_DATA_DIR: appRoot } })
let out = ''
child.stdout.on('data', (d) => {
  out += d
  process.stdout.write(d)
})
child.stderr.on('data', (d) => {
  out += d
  process.stderr.write(d)
})
child.on('close', (code) => {
  const ms = Date.now() - t0
  const candidates = [output, path.join(outDir, `${stem}.srt`)]
  const hit = candidates.find((p) => fs.existsSync(p) && fs.statSync(p).size > 0)
  const size = hit ? fs.statSync(hit).size : 0
  console.log('\n---')
  console.log(JSON.stringify({ exit: code, wall_ms: ms, wall_s: +(ms / 1000).toFixed(2), output, hit, size }, null, 2))
  if (code !== 0 || !hit) {
    console.error('FAIL: bad SRT', { code, hit, size })
    process.exit(1)
  }
  const body = fs.readFileSync(hit, 'utf8')
  console.log('SRT bytes=', Buffer.byteLength(body), 'lines=', body.split(/\n/).length)
  console.log('SRT head:\n' + body.slice(0, 600))
  console.log('PASS: smoke transcribe')
})

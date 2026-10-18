import fs from 'fs'
import path from 'path'

const appRoot = process.env.ONEASR_APP_ROOT || '/workspace/oneasr-runtime'
const settingsPath = path.join(appRoot, 'settings.json')

function load() {
  return JSON.parse(fs.readFileSync(settingsPath, 'utf8'))
}

const before = load()
const flipped = !before.sound
const next = { ...before, sound: flipped }
fs.writeFileSync(settingsPath, JSON.stringify(next, null, 2) + '\n')
const after = load()
if (after.sound !== flipped) {
  console.error('FAIL: sound did not persist', after.sound)
  process.exit(1)
}
// restore original
fs.writeFileSync(settingsPath, JSON.stringify({ ...after, sound: before.sound }, null, 2) + '\n')
const restored = load()
if (restored.sound !== before.sound) {
  console.error('FAIL: restore sound')
  process.exit(1)
}
console.log('PASS: settings load/save/persist (sound toggle)', {
  before: before.sound,
  flipped,
  restored: restored.sound,
  asr: restored.asr_model,
  asr_dir: restored.asr_model_dir
})

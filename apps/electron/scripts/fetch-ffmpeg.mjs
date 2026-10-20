#!/usr/bin/env node
/**
 * Fetch the pinned ffmpeg archive for Electron packaging.
 *
 * Source URL and archive hash come from scripts/ffmpeg-checksums.txt — the same
 * single source of truth the GPUI pack scripts use. The archive is verified
 * before extraction; a mismatch aborts instead of shipping an unknown binary.
 *
 *   node scripts/fetch-ffmpeg.mjs --os windows --arch x64 --dest runtime/bin/ffmpeg.exe
 *   node scripts/fetch-ffmpeg.mjs --os linux   --arch x64 --dest runtime/bin/ffmpeg
 *   node scripts/fetch-ffmpeg.mjs --os macos   --arch arm64 --dest runtime/bin/ffmpeg
 */
import { createHash } from 'crypto'
import fs from 'fs'
import path from 'path'
import { pipeline } from 'stream/promises'
import { fileURLToPath } from 'url'
import unzipper from 'unzipper'

const HERE = path.dirname(fileURLToPath(import.meta.url))
const APP_DIR = path.resolve(HERE, '..')
const ROOT = path.resolve(APP_DIR, '..', '..')
const MANIFEST = path.join(ROOT, 'scripts', 'ffmpeg-checksums.txt')
const CACHE = path.join(ROOT, 'dist', 'ffmpeg-cache')

function arg(name) {
  const i = process.argv.indexOf(`--${name}`)
  return i >= 0 ? process.argv[i + 1] : undefined
}

const osName = arg('os')
const arch = arg('arch')
const dest = arg('dest')
if (!['windows', 'linux', 'macos'].includes(osName) || !['x64', 'arm64'].includes(arch) || !dest) {
  console.error('usage: fetch-ffmpeg.mjs --os windows|linux|macos --arch x64|arm64 --dest <file>')
  process.exit(2)
}
const asset = {
  'windows-x64': 'ffmpeg-windows-x64.zip',
  'linux-x64': 'ffmpeg-linux-x64.zip',
  'linux-arm64': 'ffmpeg-linux-arm64.zip',
  'macos-x64': 'ffmpeg-osx-x64.zip',
  'macos-arm64': 'ffmpeg-osx-arm64.zip'
}[`${osName}-${arch}`]
if (!asset) {
  console.error(`no ffmpeg asset for ${osName}-${arch}`)
  process.exit(2)
}

function manifestEntry(file) {
  const text = fs.readFileSync(MANIFEST, 'utf8')
  const base = text.match(/^# base-url (https:\/\/\S+)$/m)?.[1]
  if (!base) throw new Error(`no '# base-url <https://...>' line in ${MANIFEST}`)
  const want = text
    .split(/\r?\n/)
    .map((line) => line.match(/^([0-9a-fA-F]{64})\s+(\S+)\s*$/))
    .find((m) => m && m[2] === file)
  if (!want) throw new Error(`no sha256 for '${file}' in ${MANIFEST}`)
  return { url: `${base}/${file}`, sha: want[1].toLowerCase() }
}

function sha256File(file) {
  const hash = createHash('sha256')
  const fd = fs.openSync(file, 'r')
  const buf = Buffer.allocUnsafe(1024 * 1024)
  try {
    let n
    while ((n = fs.readSync(fd, buf, 0, buf.length, null)) > 0) hash.update(buf.subarray(0, n))
  } finally {
    fs.closeSync(fd)
  }
  return hash.digest('hex')
}

async function download(url, file) {
  const res = await fetch(url)
  if (!res.ok) throw new Error(`download ${url} failed: HTTP ${res.status}`)
  const tmp = `${file}.part`
  fs.writeFileSync(tmp, Buffer.from(await res.arrayBuffer()))
  fs.renameSync(tmp, file)
}

async function ensuredArchive() {
  const { url, sha } = manifestEntry(asset)
  fs.mkdirSync(CACHE, { recursive: true })
  const archive = path.join(CACHE, asset)
  if (fs.existsSync(archive) && sha256File(archive) === sha) {
    console.log(`    cached   ${asset}`)
    return archive
  }
  fs.rmSync(archive, { force: true })
  console.log(`==> downloading ${asset}`)
  await download(url, archive)
  const got = sha256File(archive)
  if (got !== sha) {
    fs.rmSync(archive, { force: true })
    throw new Error(`checksum mismatch for ${asset}: expected ${sha}, got ${got}`)
  }
  console.log(`    sha256 ok  ${asset}`)
  return archive
}

async function extract(archive, out) {
  const wanted = osName === 'windows' ? 'ffmpeg.exe' : 'ffmpeg'
  const directory = await unzipper.Open.file(archive)
  const entries = directory.files.filter(
    (entry) => !entry.path.endsWith('/') && path.posix.basename(entry.path) === wanted
  )
  if (entries.length !== 1) {
    throw new Error(`expected one ${wanted} inside ${path.basename(archive)}, found ${entries.length}`)
  }
  await pipeline(entries[0].stream(), fs.createWriteStream(out))
  if (osName !== 'windows') fs.chmodSync(out, 0o755)
}

const archive = await ensuredArchive()
fs.mkdirSync(path.dirname(dest), { recursive: true })
await extract(archive, dest)
console.log(`    ffmpeg -> ${dest}`)

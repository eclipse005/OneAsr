import fs from 'fs'
import os from 'os'
import path from 'path'
import type { TranscriptInfo, TranscriptRate } from '../shared/types'
import { runCliJson } from './cli'

/** 文稿匹配固定 CTC：core / CLI 的 align 也固定它；门禁问的必须是同一个。 */
export const CTC_ALIGNER_ID = 'omniASR-CTC-300M-v2-hf'

interface CliRate {
  per_second: number
  spaced: boolean
  plausible: boolean
}

interface CliTranscript {
  kind: 'plain' | 'subrip'
  text: string
  lines: number
  chars: number
  dropped_timecodes: boolean
  speech_rate: CliRate | null
}

export interface TranscriptReadResult {
  ok: boolean
  text?: string
  info?: TranscriptInfo
  error?: string
}

function toRate(raw: CliRate | null | undefined): TranscriptRate | null {
  if (!raw) return null
  return { perSecond: raw.per_second, spaced: raw.spaced, plausible: raw.plausible }
}

function toInfo(raw: CliTranscript, pathValue: string | null, name: string): TranscriptInfo {
  return {
    path: pathValue,
    name,
    lines: raw.lines,
    chars: raw.chars,
    droppedTimecodes: raw.dropped_timecodes,
    rate: toRate(raw.speech_rate),
    edited: false,
    needsRealign: false
  }
}

export function failedTranscriptInfo(file: string, error: string): TranscriptInfo {
  return {
    path: file,
    name: path.basename(file),
    lines: 0,
    chars: 0,
    droppedTimecodes: false,
    rate: null,
    edited: false,
    needsRealign: false,
    error
  }
}

function audioArgs(audioSeconds?: number): string[] {
  return audioSeconds && Number.isFinite(audioSeconds) && audioSeconds > 0
    ? ['--audio-seconds', String(audioSeconds)]
    : []
}

/** 读 + 解析 + 摘要。path 保留，正文由调用方存到内存里。 */
export async function inspectTranscriptFile(
  file: string,
  audioSeconds?: number
): Promise<TranscriptReadResult> {
  const r = await runCliJson<CliTranscript>(
    ['transcript', 'inspect', '--input', file, '--json', ...audioArgs(audioSeconds)],
    30000
  )
  if (!r.ok || !r.data) return { ok: false, error: r.error ?? 'transcript inspect failed' }
  return { ok: true, text: r.data.text, info: toInfo(r.data, file, path.basename(file)) }
}

/**
 * 对「内存里的文本」做同一次检查：写临时文件 → CLI inspect。面板保存 / 断句
 * 都走它，统计与规范化仍只有 core 一份实现。
 */
export async function inspectTranscriptText(
  text: string,
  name: string,
  audioSeconds?: number
): Promise<TranscriptReadResult> {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'oneasr-transcript-'))
  const file = path.join(dir, 'transcript.txt')
  try {
    fs.writeFileSync(file, text, 'utf8')
    const r = await inspectTranscriptFile(file, audioSeconds)
    if (!r.ok || !r.info) return r
    return { ...r, info: { ...r.info, path: null, name } }
  } finally {
    try {
      fs.rmSync(dir, { recursive: true, force: true })
    } catch {
      /* temp cleanup is best-effort */
    }
  }
}

/** 智能断句：只拆过长行，源文件一个字都不写。 */
export async function splitTranscriptText(
  text: string,
  lang: string,
  preset: string,
  name: string,
  audioSeconds?: number
): Promise<TranscriptReadResult> {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'oneasr-transcript-'))
  const file = path.join(dir, 'transcript.txt')
  try {
    fs.writeFileSync(file, text, 'utf8')
    const r = await runCliJson<CliTranscript>(
      [
        'transcript',
        'split',
        '--input',
        file,
        '--lang',
        lang,
        '--preset',
        preset,
        '--json',
        ...audioArgs(audioSeconds)
      ],
      60000
    )
    if (!r.ok || !r.data) return { ok: false, error: r.error ?? 'transcript split failed' }
    return { ok: true, text: r.data.text, info: toInfo(r.data, null, name) }
  } finally {
    try {
      fs.rmSync(dir, { recursive: true, force: true })
    } catch {
      /* temp cleanup is best-effort */
    }
  }
}

/**
 * 编辑后的文稿要交给 align 读：写一份临时纯文本，CLI 读完就删。
 * 返回文件路径；调用方负责在子进程结束后 `removeTempTranscript`。
 */
export function writeTempTranscript(text: string): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'oneasr-align-'))
  const file = path.join(dir, 'transcript.txt')
  fs.writeFileSync(file, text, 'utf8')
  return file
}

export function removeTempTranscript(file: string): void {
  try {
    fs.rmSync(path.dirname(file), { recursive: true, force: true })
  } catch {
    /* best-effort */
  }
}

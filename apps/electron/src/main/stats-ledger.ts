import fs from 'fs'
import path from 'path'
import type { DayStat, StatsRecord, StatsSnapshot, TaskStatus } from '../shared/types'
import { dataRoot } from './settings'

export function ledgerPath(root = dataRoot()): string {
  return path.join(root, 'stats.jsonl')
}

export function loadRecords(root = dataRoot()): StatsRecord[] {
  const p = ledgerPath(root)
  if (!fs.existsSync(p)) return []
  const lines = fs.readFileSync(p, 'utf8').split(/\r?\n/)
  const out: StatsRecord[] = []
  for (const line of lines) {
    const t = line.trim()
    if (!t) continue
    try {
      const row = JSON.parse(t) as StatsRecord
      if (row && typeof row.day === 'string' && typeof row.process_ms === 'number') out.push(row)
    } catch {
      /* skip torn line */
    }
  }
  return out
}

export function appendRecord(rec: StatsRecord, root = dataRoot()): void {
  const p = ledgerPath(root)
  fs.mkdirSync(path.dirname(p), { recursive: true })
  fs.appendFileSync(p, JSON.stringify(rec) + '\n', 'utf8')
}

export function localDay(d = new Date()): string {
  const y = d.getFullYear()
  const m = String(d.getMonth() + 1).padStart(2, '0')
  const day = String(d.getDate()).padStart(2, '0')
  return `${y}-${m}-${day}`
}

export function summarize(
  records: StatsRecord[],
  sessionRecent: StatsSnapshot['recentSession'] = [],
  last?: { output?: string; durationMs?: number }
): StatsSnapshot {
  let tasksOk = 0
  let tasksErr = 0
  let mediaSec = 0
  let processMs = 0
  let cues = 0
  let sepTasks = 0
  let longestMediaSec: number | null = null
  let fastestSpeed: number | null = null
  let firstDay: string | null = null
  const langMap = new Map<string, number>()
  const dayMedia = new Map<string, number>()
  const dayTasks = new Map<string, number>()
  const dayErr = new Map<string, number>()

  for (const r of records) {
    if (!firstDay || r.day < firstDay) firstDay = r.day
    if (r.ok) {
      tasksOk += 1
      if (r.media_sec != null) {
        mediaSec += r.media_sec
        if (longestMediaSec == null || r.media_sec > longestMediaSec) longestMediaSec = r.media_sec
        const speed = r.process_ms > 0 ? r.media_sec / (r.process_ms / 1000) : null
        if (speed != null && (fastestSpeed == null || speed > fastestSpeed)) fastestSpeed = speed
        dayMedia.set(r.day, (dayMedia.get(r.day) || 0) + r.media_sec)
      }
      processMs += r.process_ms
      cues += r.cues || 0
      langMap.set(r.lang || '?', (langMap.get(r.lang || '?') || 0) + 1)
      dayTasks.set(r.day, (dayTasks.get(r.day) || 0) + 1)
    } else {
      tasksErr += 1
      dayErr.set(r.day, (dayErr.get(r.day) || 0) + 1)
    }
    if (r.sep) sepTasks += 1
  }

  // Array.from 而不是展开 Map 迭代器：node 那个 tsconfig 没有 downlevelIteration
  const langs = Array.from(langMap.entries())
    .map(([lang, count]) => ({ lang, count }))
    .sort((a, b) => b.count - a.count || a.lang.localeCompare(b.lang))

  // Last 90 days grid window
  const perDay: DayStat[] = []
  const today = new Date()
  for (let i = 89; i >= 0; i--) {
    const d = new Date(today)
    d.setDate(d.getDate() - i)
    const key = localDay(d)
    perDay.push({
      day: key,
      mediaSec: dayMedia.get(key) || 0,
      tasks: dayTasks.get(key) || 0,
      errors: dayErr.get(key) || 0
    })
  }

  const savedSec = mediaSec > 0 ? Math.max(0, mediaSec - processMs / 1000) : null
  const avgRealtime = processMs > 0 && mediaSec > 0 ? mediaSec / (processMs / 1000) : null

  return {
    tasksOk,
    tasksErr,
    tasksTotal: tasksOk + tasksErr,
    mediaSec,
    processMs,
    savedSec,
    avgRealtime,
    cues,
    sepTasks,
    langs,
    perDay,
    longestMediaSec,
    fastestSpeed,
    firstDay,
    recentSession: sessionRecent,
    lastOutput: last?.output,
    lastDurationMs: last?.durationMs
  }
}

export function emptySessionRecent(): StatsSnapshot['recentSession'] {
  return []
}

export type { TaskStatus }

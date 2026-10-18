import fs from 'fs'
import path from 'path'
import type { HotwordGroup, HotwordStore } from '../shared/types'
import { DEFAULT_APP_ROOT } from './settings'

export function hotwordPath(appRoot = DEFAULT_APP_ROOT): string {
  return path.join(appRoot, 'hotword.json')
}

export function defaultHotwordStore(): HotwordStore {
  return {
    groups: [{ id: 'default', name: '', words: '' }],
    default_group: null,
    default_group_enabled: false,
    top_k: 40,
    threshold: 0.7
  }
}

export function loadHotwords(appRoot = DEFAULT_APP_ROOT): HotwordStore {
  const p = hotwordPath(appRoot)
  if (!fs.existsSync(p)) {
    const d = defaultHotwordStore()
    saveHotwords(d, appRoot)
    return d
  }
  try {
    const raw = JSON.parse(fs.readFileSync(p, 'utf8')) as Partial<HotwordStore>
    const base = defaultHotwordStore()
    const groups: HotwordGroup[] =
      Array.isArray(raw.groups) && raw.groups.length
        ? raw.groups.map((g, i) => ({
            id: g.id || `g${i + 1}`,
            name: g.name || '',
            words: g.words || ''
          }))
        : base.groups
    return {
      groups,
      default_group: raw.default_group ?? null,
      default_group_enabled: !!raw.default_group_enabled,
      top_k: typeof raw.top_k === 'number' ? raw.top_k : 40,
      threshold: typeof raw.threshold === 'number' ? raw.threshold : 0.7
    }
  } catch {
    return defaultHotwordStore()
  }
}

export function saveHotwords(store: HotwordStore, appRoot = DEFAULT_APP_ROOT): HotwordStore {
  const normalized: HotwordStore = {
    ...store,
    groups:
      store.groups.length > 0
        ? store.groups
        : [{ id: 'default', name: '', words: '' }],
    top_k: Math.max(1, Math.min(200, store.top_k || 40)),
    threshold: Math.max(0, Math.min(1, store.threshold ?? 0.7))
  }
  const p = hotwordPath(appRoot)
  fs.writeFileSync(p, JSON.stringify(normalized, null, 2) + '\n', 'utf8')
  return normalized
}

export function wordsCsv(group: HotwordGroup | undefined): string {
  if (!group?.words) return ''
  return group.words
    .split(/[,，\n\r]+/)
    .map((w) => w.trim())
    .filter(Boolean)
    .join(',')
}

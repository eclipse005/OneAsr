import { AnimatePresence, motion } from 'framer-motion'
import { useEffect, useMemo, useState } from 'react'
import { t, UiLang } from '../i18n'
import type { HotwordGroup, HotwordStore } from '../../../shared/types'

function parseWords(raw: string): string[] {
  const seen = new Set<string>()
  const out: string[] = []
  for (const part of raw.split(/[,，\n\r\t]+/)) {
    const w = part.trim()
    if (!w) continue
    const key = w.toLowerCase()
    if (seen.has(key)) continue
    seen.add(key)
    out.push(w)
  }
  return out
}

function joinWords(words: string[]): string {
  return words.join('\n')
}

/** Core hotword editor — embeddable inside SettingsDrawer. */
export function HotwordEditor({
  lang,
  store,
  onSave,
  onDirtyChange,
  compact = false
}: {
  lang: UiLang
  store: HotwordStore
  onSave: (s: HotwordStore) => Promise<void>
  onDirtyChange?: (dirty: boolean) => void
  compact?: boolean
}) {
  const s = t(lang)
  const [draft, setDraft] = useState<HotwordStore>(() => structuredClone(store))
  const [sel, setSel] = useState(
    () => store.default_group || store.groups[0]?.id || 'default'
  )
  const [dirty, setDirty] = useState(false)
  const [addRaw, setAddRaw] = useState('')
  const [saving, setSaving] = useState(false)
  const [lastAdded, setLastAdded] = useState<number | null>(null)

  useEffect(() => {
    onDirtyChange?.(dirty)
  }, [dirty, onDirtyChange])

  // Reset when store prop identity changes (after external save / reopen)
  useEffect(() => {
    setDraft(structuredClone(store))
    setSel(store.default_group || store.groups[0]?.id || 'default')
    setDirty(false)
    setAddRaw('')
  }, [store])

  const group = draft.groups.find((g) => g.id === sel) || draft.groups[0]
  const words = useMemo(() => parseWords(group?.words || ''), [group?.words])

  const touch = (next: HotwordStore) => {
    setDraft(next)
    setDirty(true)
  }

  const updateGroup = (id: string, patch: Partial<HotwordGroup>) => {
    touch({
      ...draft,
      groups: draft.groups.map((g) => (g.id === id ? { ...g, ...patch } : g))
    })
  }

  const setWords = (id: string, list: string[]) => {
    updateGroup(id, { words: joinWords(list) })
  }

  const applyAdd = () => {
    if (!group) return
    const incoming = parseWords(addRaw)
    if (!incoming.length) return
    const before = new Set(words.map((w) => w.toLowerCase()))
    const merged = [...words]
    let added = 0
    for (const w of incoming) {
      if (before.has(w.toLowerCase())) continue
      before.add(w.toLowerCase())
      merged.push(w)
      added += 1
    }
    setWords(group.id, merged)
    setAddRaw('')
    setLastAdded(added)
    window.setTimeout(() => setLastAdded(null), 2200)
  }

  const removeWord = (w: string) => {
    if (!group) return
    setWords(
      group.id,
      words.filter((x) => x !== w)
    )
  }

  const discard = () => {
    setDraft(structuredClone(store))
    setSel(store.default_group || store.groups[0]?.id || 'default')
    setDirty(false)
    setAddRaw('')
  }

  const save = () => {
    setSaving(true)
    const normalized: HotwordStore = {
      ...draft,
      groups: draft.groups.map((g) => ({
        ...g,
        words: joinWords(parseWords(g.words))
      }))
    }
    void onSave(normalized)
      .then(() => {
        setDraft(structuredClone(normalized))
        setDirty(false)
      })
      .finally(() => setSaving(false))
  }

  return (
    <div className={`hw-embed vertical ${compact ? 'compact' : ''}`}>
      {/* Top: compact group management */}
      <div className="hw-group-bar">
        <select
          className="hw-group-select"
          value={group?.id || ''}
          onChange={(e) => setSel(e.target.value)}
          aria-label={s.groups}
        >
          {draft.groups.map((g) => {
            const n = parseWords(g.words).length
            const isDef = draft.default_group === g.id
            const label = g.name.trim() || s.emptyGroup
            return (
              <option key={g.id} value={g.id}>
                {label}
                {isDef ? ` · ${s.isDefault}` : ''}
                {` (${n})`}
              </option>
            )
          })}
        </select>
        <div className="hw-default-toggle" title={s.defaultGroup}>
          <span className="pref-label">{s.defaultGroupShort}</span>
          <button
            type="button"
            className={`toggle ${draft.default_group_enabled ? 'on' : ''}`}
            role="switch"
            aria-checked={draft.default_group_enabled}
            aria-label={s.defaultGroup}
            disabled={!group}
            onClick={() =>
              touch({
                ...draft,
                default_group_enabled: !draft.default_group_enabled,
                default_group: draft.default_group || group?.id || null
              })
            }
          />
        </div>
        <input
          className="text-input hw-name"
          placeholder={s.groupNamePh}
          value={group?.name || ''}
          disabled={!group}
          onChange={(e) => group && updateGroup(group.id, { name: e.target.value })}
          aria-label={s.groupName}
        />
        <span className="hw-count">
          {words.length} {s.wordCount}
        </span>
        <button
          type="button"
          className="btn btn-ghost btn-sm"
          onClick={() => {
            const id = `g${Date.now()}`
            touch({
              ...draft,
              groups: [...draft.groups, { id, name: '', words: '' }]
            })
            setSel(id)
          }}
        >
          + {s.addGroup}
        </button>
        <button
          type="button"
          className={`chip ${group && draft.default_group === group.id ? 'active' : ''}`}
          disabled={!group}
          onClick={() =>
            group &&
            touch({
              ...draft,
              default_group: group.id,
              default_group_enabled: true
            })
          }
        >
          {s.setDefault}
        </button>
        <button
          type="button"
          className="btn btn-ghost btn-danger-ghost btn-sm"
          disabled={!group || draft.groups.length <= 1}
          onClick={() => {
            if (!group) return
            if (!confirm(s.confirmDeleteGroup)) return
            const next = draft.groups.filter((g) => g.id !== group.id)
            const nextDef =
              draft.default_group === group.id ? next[0]?.id || null : draft.default_group
            touch({ ...draft, groups: next, default_group: nextDef })
            setSel(next[0]?.id || 'default')
          }}
        >
          {s.deleteGroup}
        </button>
      </div>

      {/* Middle: chip cloud */}
      <div className={`hw-chips ${words.length === 0 ? 'empty' : ''}`}>
        {!group ? (
          <span>{s.emptyGroup}</span>
        ) : words.length === 0 ? (
          <span>{s.emptyChips}</span>
        ) : (
          <AnimatePresence initial={false}>
            {words.map((w) => (
              <motion.span
                key={w}
                className="hw-chip"
                layout
                initial={{ opacity: 0, scale: 0.88 }}
                animate={{ opacity: 1, scale: 1 }}
                exit={{ opacity: 0, scale: 0.88 }}
                transition={{ duration: 0.15 }}
              >
                {w}
                <button type="button" aria-label={s.removeWord} onClick={() => removeWord(w)}>
                  ×
                </button>
              </motion.span>
            ))}
          </AnimatePresence>
        )}
      </div>

      {/* Bottom: add only */}
      <div className="hw-bottom">
        <div className="hw-add">
          <input
            className="text-input"
            placeholder={s.addWordPh}
            value={addRaw}
            disabled={!group}
            onChange={(e) => setAddRaw(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && !e.shiftKey) {
                e.preventDefault()
                applyAdd()
              }
            }}
          />
          <button type="button" className="btn btn-primary btn-sm" disabled={!group} onClick={applyAdd}>
            {s.addWord}
          </button>
          {lastAdded != null && lastAdded > 0 && (
            <span className="hw-add-feedback">+{lastAdded}</span>
          )}
        </div>
      </div>

      <div className="hw-embed-foot">
        <span className="hint">{dirty ? s.unsaved : ''}</span>
        <button type="button" className="btn btn-ghost btn-sm" disabled={!dirty} onClick={discard}>
          {s.discard}
        </button>
        <button
          type="button"
          className="btn btn-primary btn-sm"
          disabled={!dirty || saving}
          onClick={save}
        >
          {s.save}
        </button>
      </div>
    </div>
  )

}

/** @deprecated Standalone modal — prefer SettingsDrawer hotwords tab. Kept for rare callers. */
export function HotwordPanel({
  lang,
  store,
  onSave,
  onClose
}: {
  lang: UiLang
  store: HotwordStore
  onSave: (s: HotwordStore) => Promise<void>
  onClose: () => void
}) {
  const s = t(lang)
  const [dirty, setDirty] = useState(false)
  const tryClose = () => {
    if (dirty && !confirm(s.confirmDiscard)) return
    onClose()
  }
  return (
    <div className="overlay" onClick={tryClose}>
      <motion.div
        className="modal-shell hw-shell"
        role="dialog"
        aria-modal="true"
        onClick={(e) => e.stopPropagation()}
        initial={{ opacity: 0, y: 20, scale: 0.96 }}
        animate={{ opacity: 1, y: 0, scale: 1 }}
        exit={{ opacity: 0, y: 12, scale: 0.97 }}
        transition={{ type: 'spring', stiffness: 380, damping: 32 }}
      >
        <div className="modal-header">
          <h3>{s.hotwordTitle}</h3>
          <div style={{ flex: 1 }} />
          <button type="button" data-shot="close-modal" className="btn btn-ghost" onClick={tryClose}>
            {s.close}
          </button>
        </div>
        <div className="modal-body" style={{ gap: 0, overflow: 'hidden', padding: 10 }}>
          <HotwordEditor lang={lang} store={store} onSave={onSave} onDirtyChange={setDirty} />
        </div>
      </motion.div>
    </div>
  )
}

import { AnimatePresence, motion } from 'framer-motion'
import { useMemo, useState } from 'react'
import { t, UiLang } from '../i18n'
import type { HotwordGroup, HotwordStore } from '../../../shared/types'

function parseWords(raw: string): string[] {
  const seen = new Set<string>()
  const out: string[] = []
  for (const part of raw.split(/[,，\n\r]+/)) {
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
  const [draft, setDraft] = useState<HotwordStore>(() => structuredClone(store))
  const [sel, setSel] = useState(
    () => store.default_group || store.groups[0]?.id || 'default'
  )
  const [dirty, setDirty] = useState(false)
  const [quick, setQuick] = useState('')
  const [saving, setSaving] = useState(false)

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

  const addQuick = () => {
    if (!group) return
    const w = quick.trim()
    if (!w) return
    const next = parseWords([...words, w].join('\n'))
    setWords(group.id, next)
    setQuick('')
  }

  const removeWord = (w: string) => {
    if (!group) return
    setWords(
      group.id,
      words.filter((x) => x !== w)
    )
  }

  const tryClose = () => {
    if (dirty && !confirm(lang === 'zh' ? '有未保存更改，确定关闭？' : 'Discard unsaved changes?')) {
      return
    }
    onClose()
  }

  return (
    <div className="overlay" onClick={tryClose}>
      <motion.div
        className="modal-shell wide tall"
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
          <span className="hw-count">
            {draft.groups.length} {s.groups}
          </span>
          <div style={{ flex: 1 }} />
          <button type="button" className="btn btn-ghost" onClick={tryClose}>
            {s.close}
          </button>
        </div>

        <div className="modal-body" style={{ gap: 12 }}>
          <div className="note">{s.hotwordHint}</div>

          <div className="hw-layout">
            <aside className="hw-sidebar">
              <div className="hw-sidebar-head">{s.groups}</div>
              <div className="hw-group-list">
                <AnimatePresence initial={false}>
                  {draft.groups.map((g) => {
                    const n = parseWords(g.words).length
                    const isDef = draft.default_group === g.id
                    return (
                      <motion.button
                        key={g.id}
                        type="button"
                        layout
                        className={`hw-group-item ${g.id === sel ? 'active' : ''}`}
                        onClick={() => setSel(g.id)}
                        initial={{ opacity: 0, x: -6 }}
                        animate={{ opacity: 1, x: 0 }}
                        exit={{ opacity: 0, x: -6 }}
                      >
                        <div>
                          {g.name.trim() || s.emptyGroup}
                          {isDef ? ` · ${s.isDefault}` : ''}
                        </div>
                        <div className="meta">
                          {n} {s.wordCount}
                        </div>
                      </motion.button>
                    )
                  })}
                </AnimatePresence>
              </div>
              <div className="hw-sidebar-foot">
                <button
                  type="button"
                  className="btn btn-ghost"
                  style={{ width: '100%' }}
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
              </div>
            </aside>

            <div className="hw-editor">
              {group ? (
                <>
                  <div className="field stack">
                    <label>{s.groupName}</label>
                    <input
                      className="text-input"
                      placeholder={s.groupNamePh}
                      value={group.name}
                      onChange={(e) => updateGroup(group.id, { name: e.target.value })}
                    />
                  </div>

                  <div className="hw-toolbar">
                    <span className="hw-count">
                      {words.length} {s.wordCount}
                    </span>
                    <button
                      type="button"
                      className={`chip ${draft.default_group === group.id ? 'active' : ''}`}
                      onClick={() =>
                        touch({
                          ...draft,
                          default_group: group.id,
                          default_group_enabled: true
                        })
                      }
                    >
                      {s.setDefault}
                    </button>
                    <div style={{ flex: 1 }} />
                    <button
                      type="button"
                      className="btn btn-ghost btn-danger-ghost"
                      disabled={draft.groups.length <= 1}
                      onClick={() => {
                        if (
                          !confirm(
                            lang === 'zh' ? '删除这个词组？' : 'Delete this group?'
                          )
                        )
                          return
                        const next = draft.groups.filter((g) => g.id !== group.id)
                        const nextDef =
                          draft.default_group === group.id
                            ? next[0]?.id || null
                            : draft.default_group
                        touch({
                          ...draft,
                          groups: next,
                          default_group: nextDef
                        })
                        setSel(next[0]?.id || 'default')
                      }}
                    >
                      {s.deleteGroup}
                    </button>
                  </div>

                  <div className="hw-quick">
                    <input
                      className="text-input"
                      placeholder={s.addWordPh}
                      value={quick}
                      onChange={(e) => setQuick(e.target.value)}
                      onKeyDown={(e) => {
                        if (e.key === 'Enter') {
                          e.preventDefault()
                          addQuick()
                        }
                      }}
                    />
                    <button type="button" className="btn btn-primary" onClick={addQuick}>
                      {s.addWord}
                    </button>
                  </div>

                  <div className="hw-chips">
                    <AnimatePresence initial={false}>
                      {words.map((w) => (
                        <motion.span
                          key={w}
                          className="hw-chip"
                          layout
                          initial={{ opacity: 0, scale: 0.9 }}
                          animate={{ opacity: 1, scale: 1 }}
                          exit={{ opacity: 0, scale: 0.9 }}
                        >
                          {w}
                          <button type="button" aria-label="remove" onClick={() => removeWord(w)}>
                            ×
                          </button>
                        </motion.span>
                      ))}
                    </AnimatePresence>
                  </div>

                  <div className="field stack" style={{ flex: 1, display: 'flex', flexDirection: 'column' }}>
                    <label>
                      {s.words}
                      <span style={{ marginLeft: 8, fontWeight: 400, color: 'var(--muted)' }}>
                        {s.wordsHint}
                      </span>
                    </label>
                    <textarea
                      className="text-input"
                      placeholder={s.wordsPh}
                      value={group.words}
                      onChange={(e) => updateGroup(group.id, { words: e.target.value })}
                    />
                  </div>

                  <div>
                    <div
                      style={{
                        fontSize: '0.72rem',
                        fontWeight: 800,
                        letterSpacing: '0.06em',
                        textTransform: 'uppercase',
                        color: 'var(--accent)',
                        marginBottom: 8
                      }}
                    >
                      {s.params}
                    </div>
                    <div className="hw-params">
                      <div className="field stack">
                        <label>{s.defaultGroup}</label>
                        <button
                          type="button"
                          className={`toggle ${draft.default_group_enabled ? 'on' : ''}`}
                          role="switch"
                          aria-checked={draft.default_group_enabled}
                          onClick={() =>
                            touch({
                              ...draft,
                              default_group_enabled: !draft.default_group_enabled,
                              default_group: draft.default_group || group.id
                            })
                          }
                        />
                      </div>
                      <div className="field stack">
                        <label>{s.topK}</label>
                        <input
                          className="text-input"
                          type="number"
                          min={1}
                          max={200}
                          value={draft.top_k}
                          onChange={(e) =>
                            touch({ ...draft, top_k: Number(e.target.value) || 40 })
                          }
                        />
                      </div>
                      <div className="field stack">
                        <label>{s.threshold}</label>
                        <input
                          className="text-input"
                          type="number"
                          min={0}
                          max={1}
                          step={0.05}
                          value={draft.threshold}
                          onChange={(e) =>
                            touch({ ...draft, threshold: Number(e.target.value) || 0.7 })
                          }
                        />
                      </div>
                    </div>
                  </div>
                </>
              ) : (
                <p style={{ color: 'var(--muted)' }}>{s.emptyGroup}</p>
              )}
            </div>
          </div>
        </div>

        <div className="modal-footer">
          <span className="hint">{dirty ? s.unsaved : ''}</span>
          <button
            type="button"
            className="btn btn-ghost"
            disabled={!dirty}
            onClick={() => {
              setDraft(structuredClone(store))
              setSel(store.default_group || store.groups[0]?.id || 'default')
              setDirty(false)
              setQuick('')
            }}
          >
            {s.discard}
          </button>
          <button
            type="button"
            className="btn btn-primary"
            disabled={!dirty || saving}
            onClick={() => {
              setSaving(true)
              // normalize words before save
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
            }}
          >
            {s.save}
          </button>
        </div>
      </motion.div>
    </div>
  )
}

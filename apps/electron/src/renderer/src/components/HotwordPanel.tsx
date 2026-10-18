import { motion } from 'framer-motion'
import { useState } from 'react'
import { t, UiLang } from '../i18n'
import type { HotwordStore } from '../../../shared/types'

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
  const [draft, setDraft] = useState<HotwordStore>(store)
  const [sel, setSel] = useState(store.groups[0]?.id || 'default')
  const [dirty, setDirty] = useState(false)
  const group = draft.groups.find((g) => g.id === sel) || draft.groups[0]

  const touch = (next: HotwordStore) => {
    setDraft(next)
    setDirty(true)
  }

  return (
    <div className="overlay center" onClick={onClose}>
      <motion.div
        className="panel"
        onClick={(e) => e.stopPropagation()}
        initial={{ opacity: 0, y: 14 }}
        animate={{ opacity: 1, y: 0 }}
        exit={{ opacity: 0, y: 8 }}
      >
        <div className="modal-header">
          <h3>{s.hotwordTitle}</h3>
          <div style={{ flex: 1 }} />
          <button className="btn btn-ghost" onClick={onClose}>{s.close}</button>
        </div>
        <div className="body" style={{ whiteSpace: 'normal' }}>
          <div className="note">{s.hotwordHint}</div>
          <div className="chip-row" style={{ margin: '12px 0' }}>
            {draft.groups.map((g) => (
              <button
                key={g.id}
                type="button"
                className={`chip ${g.id === sel ? 'active' : ''}`}
                onClick={() => setSel(g.id)}
              >
                {g.name || g.id}
              </button>
            ))}
            <button
              type="button"
              className="btn btn-ghost"
              onClick={() => {
                const id = `g${Date.now()}`
                touch({ ...draft, groups: [...draft.groups, { id, name: '', words: '' }] })
                setSel(id)
              }}
            >
              {s.addGroup}
            </button>
          </div>
          {group && (
            <>
              <div className="field stack">
                <label>{s.groupName}</label>
                <input
                  className="text-input"
                  value={group.name}
                  onChange={(e) =>
                    touch({
                      ...draft,
                      groups: draft.groups.map((g) =>
                        g.id === group.id ? { ...g, name: e.target.value } : g
                      )
                    })
                  }
                />
              </div>
              <div className="field stack" style={{ marginTop: 10 }}>
                <label>{s.words}</label>
                <textarea
                  className="text-input"
                  value={group.words}
                  onChange={(e) =>
                    touch({
                      ...draft,
                      groups: draft.groups.map((g) =>
                        g.id === group.id ? { ...g, words: e.target.value } : g
                      )
                    })
                  }
                />
              </div>
            </>
          )}
          <div className="field" style={{ marginTop: 12 }}>
            <label>{s.defaultGroup}</label>
            <button
              type="button"
              className={`toggle ${draft.default_group_enabled ? 'on' : ''}`}
              onClick={() =>
                touch({
                  ...draft,
                  default_group_enabled: !draft.default_group_enabled,
                  default_group: draft.default_group || sel
                })
              }
            />
          </div>
          <div className="field">
            <label>{s.topK}</label>
            <input
              className="text-input"
              type="number"
              value={draft.top_k}
              onChange={(e) => touch({ ...draft, top_k: Number(e.target.value) || 40 })}
            />
          </div>
          <div className="field">
            <label>{s.threshold}</label>
            <input
              className="text-input"
              type="number"
              step="0.05"
              value={draft.threshold}
              onChange={(e) => touch({ ...draft, threshold: Number(e.target.value) || 0.7 })}
            />
          </div>
          <div className="modal actions" style={{ marginTop: 16, padding: 0 }}>
            <button
              type="button"
              className="btn btn-ghost btn-danger-ghost"
              disabled={draft.groups.length <= 1}
              onClick={() => {
                const next = draft.groups.filter((g) => g.id !== sel)
                touch({ ...draft, groups: next })
                setSel(next[0]?.id || 'default')
              }}
            >
              {s.deleteGroup}
            </button>
            <div style={{ flex: 1 }} />
            <span style={{ color: 'var(--warn)', fontSize: '0.82rem', marginRight: 8 }}>
              {dirty ? s.unsaved : ''}
            </span>
            <button
              type="button"
              className="btn btn-primary"
              disabled={!dirty}
              onClick={() =>
                void onSave(draft).then(() => {
                  setDirty(false)
                })
              }
            >
              {s.save}
            </button>
          </div>
        </div>
      </motion.div>
    </div>
  )
}

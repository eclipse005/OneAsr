import { AnimatePresence, motion, useReducedMotion } from 'framer-motion'
import { useEffect, useMemo, useRef, useState } from 'react'
import { t, UiLang } from '../i18n'
import { useLayerMotion } from '../motion'
import { Popover } from './Popover'
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

/** Controlled hotword editor — draft owned by SettingsDrawer / App. */
export function HotwordEditor({
  lang,
  value,
  onChange,
  onAskDeleteGroup,
  compact = false
}: {
  lang: UiLang
  value: HotwordStore
  onChange: (next: HotwordStore) => void
  /** 删组前先走统一的确认条，确认后才调用这里传出去的动作 */
  onAskDeleteGroup: (apply: () => void) => void
  compact?: boolean
}) {
  const s = t(lang)
  const chipMotion = useLayerMotion()
  const reduceMotion = useReducedMotion() === true
  const valueRef = useRef(value)
  const onChangeRef = useRef(onChange)
  valueRef.current = value
  onChangeRef.current = onChange
  const [sel, setSel] = useState(
    () => value.default_group || value.groups[0]?.id || 'default'
  )
  const [addRaw, setAddRaw] = useState('')
  const [lastAdded, setLastAdded] = useState<number | null>(null)
  const [groupAnchor, setGroupAnchor] = useState<DOMRect | null>(null)
  const groupTriggerRef = useRef<HTMLButtonElement>(null)

  useEffect(() => {
    if (!value.groups.some((g) => g.id === sel)) {
      setSel(value.default_group || value.groups[0]?.id || 'default')
    }
  }, [value, sel])

  const group = value.groups.find((g) => g.id === sel) || value.groups[0]
  const words = useMemo(() => parseWords(group?.words || ''), [group?.words])

  const touch = (next: HotwordStore) => onChange(next)

  const updateGroup = (id: string, patch: Partial<HotwordGroup>) => {
    touch({
      ...value,
      groups: value.groups.map((g) => (g.id === id ? { ...g, ...patch } : g))
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

  return (
    <div className={`hw-embed vertical ${compact ? 'compact' : ''}`}>
      <div className="hw-group-bar">
        <div className={`hw-group-menu${groupAnchor ? ' open' : ''}`}>
          <button
            ref={groupTriggerRef}
            type="button"
            className="hw-group-trigger"
            aria-haspopup="listbox"
            aria-expanded={groupAnchor != null}
            aria-label={s.groups}
            onClick={(e) =>
              setGroupAnchor((cur) => (cur ? null : e.currentTarget.getBoundingClientRect()))
            }
          >
            <span className="hw-group-trigger-label">
              {group?.name.trim() || s.emptyGroup}
              {group && value.default_group === group.id ? ` · ${s.isDefault}` : ''}
            </span>
            <span className="hw-group-caret" aria-hidden>
              ▾
            </span>
          </button>
          <Popover
            open={groupAnchor != null}
            anchor={groupAnchor}
            onClose={() => setGroupAnchor(null)}
            className="chip-menu hw-group-popover"
            label={s.groups}
            trigger={groupTriggerRef}
          >
            <ul className="hw-group-listbox" role="listbox">
              {value.groups.map((g) => {
                const isDef = value.default_group === g.id
                const label = g.name.trim() || s.emptyGroup
                const active = g.id === group?.id
                return (
                  <li
                    key={g.id}
                    className={`hw-group-option${active ? ' active' : ''}${isDef ? ' is-default' : ''}`}
                    role="option"
                    aria-selected={active}
                  >
                    <button
                      type="button"
                      className="hw-group-option-main"
                      onClick={() => {
                        setSel(g.id)
                        setGroupAnchor(null)
                      }}
                    >
                      <span className="hw-group-option-name">{label}</span>
                    </button>
                    {isDef ? (
                      <span className="hw-group-default-badge">{s.isDefault}</span>
                    ) : (
                      <button
                        type="button"
                        className="hw-group-set-default"
                        onClick={(e) => {
                          e.stopPropagation()
                          touch({
                            ...value,
                            default_group: g.id,
                            default_group_enabled: true
                          })
                        }}
                      >
                        {s.setDefault}
                      </button>
                    )}
                  </li>
                )
              })}
            </ul>
          </Popover>
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
              ...value,
              groups: [...value.groups, { id, name: '', words: '' }]
            })
            setSel(id)
          }}
        >
          + {s.addGroup}
        </button>
        <button
          type="button"
          className="btn btn-ghost btn-danger-ghost btn-sm"
          disabled={!group || value.groups.length <= 1}
          onClick={() => {
            if (!group) return
            const id = group.id
            onAskDeleteGroup(() => {
              const latest = valueRef.current
              const next = latest.groups.filter((g) => g.id !== id)
              const nextDef = latest.default_group === id ? next[0]?.id || null : latest.default_group
              onChangeRef.current({ ...latest, groups: next, default_group: nextDef })
              setSel(next[0]?.id || 'default')
            })
          }}
        >
          {s.deleteGroup}
        </button>
      </div>

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
                layout={!reduceMotion}
                {...chipMotion}
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
    </div>
  )
}


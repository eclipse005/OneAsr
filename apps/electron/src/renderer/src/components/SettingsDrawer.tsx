import { useEffect, useState } from 'react'
import { motion } from 'framer-motion'
import { LANG_OPTIONS, t, UiLang } from '../i18n'
import type { HotwordStore, ModelStatus, Settings } from '../../../shared/types'
import { HotwordEditor } from './HotwordPanel'

export type SettingsTab = 'general' | 'output' | 'models' | 'hotwords' | 'compute'

function Toggle({ on, onChange }: { on: boolean; onChange: (v: boolean) => void }) {
  return (
    <button
      type="button"
      className={`toggle ${on ? 'on' : ''}`}
      role="switch"
      aria-checked={on}
      onClick={() => onChange(!on)}
    />
  )
}

function Pill({ ok, label, ready, missing }: { ok: boolean; label: string; ready: string; missing: string }) {
  return (
    <span className={`status-pill ${ok ? 'ready' : 'missing'}`}>
      {label}: {ok ? ready : missing}
    </span>
  )
}

function Seg({
  options,
  value,
  onChange
}: {
  options: { id: string; label: string }[]
  value: string
  onChange: (id: string) => void
}) {
  return (
    <div className="seg" role="group">
      {options.map((o) => (
        <button
          key={o.id}
          type="button"
          className={`seg-item ${value === o.id ? 'active' : ''}`}
          aria-pressed={value === o.id}
          onClick={() => onChange(o.id)}
        >
          {o.label}
        </button>
      ))}
    </div>
  )
}

function CheckChip({
  checked,
  label,
  onChange
}: {
  checked: boolean
  label: string
  onChange: (v: boolean) => void
}) {
  return (
    <button
      type="button"
      className={`check-chip ${checked ? 'active' : ''}`}
      aria-pressed={checked}
      onClick={() => onChange(!checked)}
    >
      <span className="check-mark" aria-hidden>
        {checked ? '✓' : ''}
      </span>
      {label}
    </button>
  )
}

function asrSizeOf(model: string): '0.6B' | '1.7B' {
  return model.includes('1.7B') ? '1.7B' : '0.6B'
}
function asrQuantOf(model: string): 'fp16' | 'int8' {
  return model.includes('int8') ? 'int8' : 'fp16'
}
function asrId(size: '0.6B' | '1.7B', quant: 'fp16' | 'int8'): string {
  return `Qwen3-ASR-${size}-${quant === 'int8' ? 'int8' : 'hf'}`
}
function alignerKind(model: string): 'ctc' | 'qwen' {
  return model.toLowerCase().includes('qwen') ? 'qwen' : 'ctc'
}

export function SettingsDrawer({
  lang,
  draft,
  models,
  dirty,
  hotwordDraft,
  initialTab = 'general',
  onChange,
  onHotwordsChange,
  onSave,
  onReset,
  onClose,
  onPickDir,
  onPickModel,
  onOpenModels,
  onRefreshModels
}: {
  lang: UiLang
  draft: Settings
  models: ModelStatus | null
  dirty: boolean
  hotwordDraft: HotwordStore | null
  initialTab?: SettingsTab
  onChange: (next: Settings) => void
  onHotwordsChange: (next: HotwordStore) => void
  onSave: () => void
  onReset: () => void
  onClose: () => void
  onPickDir: () => void
  onPickModel: (field: 'asr_model_dir' | 'aligner_model_dir' | 'demucs_model_dir' | 'sensevoice_model_dir') => void
  onOpenModels: () => void
  onRefreshModels: () => void
}) {
  const s = t(lang)
  const [activeTab, setActiveTab] = useState<SettingsTab>(initialTab)
  useEffect(() => {
    setActiveTab(initialTab)
  }, [initialTab])
  const set = <K extends keyof Settings>(key: K, value: Settings[K]) =>
    onChange({ ...draft, [key]: value })

  const tryClose = () => {
    onClose()
  }

  const size = asrSizeOf(draft.asr_model)
  const quant = asrQuantOf(draft.asr_model)

  const setAsr = (nextSize: '0.6B' | '1.7B', nextQuant: 'fp16' | 'int8') => {
    const id = asrId(nextSize, nextQuant)
    const dir =
      draft.asr_dirs[id] ||
      draft.asr_model_dir.replace(/Qwen3-ASR-[^/]+$/, id) ||
      draft.asr_model_dir
    onChange({
      ...draft,
      asr_model: id,
      asr_model_dir: dir,
      asr_dirs: { ...draft.asr_dirs, [id]: dir }
    })
  }

  const setAligner = (kind: 'ctc' | 'qwen') => {
    const id = kind === 'qwen' ? 'Qwen3-ForcedAligner-0.6B-hf' : 'omniASR-CTC-300M-v2-hf'
    const dir = draft.aligner_dirs[id] || draft.aligner_model_dir.replace(/[^/]+$/, id)
    onChange({
      ...draft,
      aligner_model: id,
      aligner_model_dir: dir,
      aligner_dirs: { ...draft.aligner_dirs, [id]: dir }
    })
  }

  const tabs: { id: SettingsTab; label: string }[] = [
    { id: 'general', label: s.sectionGeneral },
    { id: 'output', label: s.sectionOutput },
    { id: 'models', label: s.sectionModels },
    { id: 'hotwords', label: s.sectionHotwords },
    { id: 'compute', label: s.sectionCompute }
  ]

  return (
    <div className="overlay" onClick={tryClose}>
      <motion.div
        className="modal-shell"
        role="dialog"
        aria-modal="true"
        onClick={(e) => e.stopPropagation()}
        initial={{ opacity: 0, y: 18, scale: 0.96 }}
        animate={{ opacity: 1, y: 0, scale: 1 }}
        exit={{ opacity: 0, y: 10, scale: 0.97 }}
        transition={{ type: 'spring', stiffness: 380, damping: 32 }}
      >
        <div className="modal-header">
          <h3>{s.settings}</h3>
          <div style={{ flex: 1 }} />
          <button type="button" data-shot="close-modal" className="btn btn-ghost" onClick={tryClose}>
            {s.close}
          </button>
        </div>

        <div className="settings-body">
          <nav className="settings-nav" aria-label={s.settings}>
            {tabs.map((tab) => (
              <button
                key={tab.id}
                type="button"
                data-shot={`settings-tab-${tab.id}`}
                className={`settings-nav-item ${activeTab === tab.id ? 'active' : ''}`}
                aria-current={activeTab === tab.id ? 'page' : undefined}
                onClick={() => setActiveTab(tab.id)}
              >
                {tab.label}
              </button>
            ))}
          </nav>

          <div className={`settings-pane${activeTab === 'hotwords' ? ' is-hotwords' : ''}`} data-shot={`settings-pane-${activeTab}`}>
            {activeTab === 'general' && (
              <>
                <div className="field">
                  <label>{s.uiLanguage}</label>
                  <select value={draft.ui_language} onChange={(e) => set('ui_language', e.target.value)}>
                    <option value="system">{s.system}</option>
                    <option value="zh">中文</option>
                    <option value="en">English</option>
                  </select>
                </div>
                <div className="field">
                  <label>{s.recognitionLanguage}</label>
                  <select value={draft.language} onChange={(e) => set('language', e.target.value)}>
                    {LANG_OPTIONS.map((o) => (
                      <option key={o.id} value={o.id}>
                        {o.label}
                      </option>
                    ))}
                  </select>
                </div>
                <div className="field">
                  <label>{s.subtitleLength}</label>
                  <Seg
                    value={draft.subtitle_length_preset}
                    onChange={(id) => set('subtitle_length_preset', id as Settings['subtitle_length_preset'])}
                    options={[
                      { id: 'short', label: s.short },
                      { id: 'standard', label: s.standard },
                      { id: 'loose', label: s.loose }
                    ]}
                  />
                </div>
                <div className="field">
                  <label>{s.chunkDuration}</label>
                  <select
                    value={String(draft.chunk_target_seconds)}
                    onChange={(e) => set('chunk_target_seconds', Number(e.target.value))}
                  >
                    {[30, 60, 90, 120, 180].map((n) => (
                      <option key={n} value={n}>
                        {n}
                      </option>
                    ))}
                  </select>
                </div>
              </>
            )}

            {activeTab === 'output' && (
              <>
                <div className="field">
                  <label>{s.outputs}</label>
                  <div className="check-chip-row">
                    <CheckChip checked={draft.output_srt} label="SRT" onChange={(v) => set('output_srt', v)} />
                    <CheckChip checked={draft.output_txt} label="TXT" onChange={(v) => set('output_txt', v)} />
                    <CheckChip checked={draft.output_ass} label="ASS" onChange={(v) => set('output_ass', v)} />
                  </div>
                </div>
                <div className="field">
                  <label>{s.chineseScript}</label>
                  <Seg
                    value={draft.text_script}
                    onChange={(id) => set('text_script', id as Settings['text_script'])}
                    options={[
                      { id: 'simplified', label: s.simplified },
                      { id: 'traditional', label: s.traditional },
                      { id: 'original', label: s.original }
                    ]}
                  />
                </div>
                <div className="pref-row">
                  <span className="pref-label">{s.subtitlePolish}</span>
                  <Toggle on={draft.subtitle_beautify} onChange={(v) => set('subtitle_beautify', v)} />
                </div>
                <div className="pref-row">
                  <span className="pref-label">{s.saveNextToSource}</span>
                  <Toggle on={draft.save_next_to_source} onChange={(v) => set('save_next_to_source', v)} />
                </div>
                <div className="field">
                  <label>{s.outputPath}</label>
                  <div className="path-input compact">
                    <input
                      className="text-input mono"
                      value={draft.output_dir}
                      onChange={(e) => set('output_dir', e.target.value)}
                    />
                    <button type="button" className="btn btn-ghost btn-sm" onClick={onPickDir}>
                      {s.browse}
                    </button>
                  </div>
                </div>
              </>
            )}

            {activeTab === 'models' && (
              <>
                <div className="models-toolbar">
                  <div className="models-status">
                    <Pill ok={!!models?.asrReady} label={s.asr} ready={s.ready} missing={s.missing} />
                    <Pill ok={!!models?.alignerReady} label={s.aligner} ready={s.ready} missing={s.missing} />
                    <Pill ok={!!models?.demucsReady} label={s.demucs} ready={s.ready} missing={s.missing} />
                  </div>
                  <div className="models-actions">
                    <button type="button" className="btn btn-ghost btn-sm" onClick={onOpenModels}>
                      {s.openModels}
                    </button>
                    <button type="button" className="btn btn-ghost btn-icon" onClick={onRefreshModels} aria-label="Refresh">
                      ↻
                    </button>
                  </div>
                </div>
                <div className="pane-divider" />
                <div className="field">
                  <label>{s.asrSize}</label>
                  <Seg
                    value={size}
                    onChange={(id) => setAsr(id as '0.6B' | '1.7B', quant)}
                    options={[
                      { id: '0.6B', label: '0.6B' },
                      { id: '1.7B', label: '1.7B' }
                    ]}
                  />
                </div>
                <div className="field">
                  <label>{s.quantization}</label>
                  <Seg
                    value={quant}
                    onChange={(id) => setAsr(size, id as 'fp16' | 'int8')}
                    options={[
                      { id: 'fp16', label: s.fp16 },
                      { id: 'int8', label: s.int8 }
                    ]}
                  />
                </div>
                <div className="field">
                  <label>{s.asrPath}</label>
                  <div className="path-input compact">
                    <input
                      className="text-input mono"
                      value={draft.asr_model_dir}
                      onChange={(e) => set('asr_model_dir', e.target.value)}
                    />
                    <button
                      type="button"
                      className="btn btn-ghost btn-sm"
                      onClick={() => onPickModel('asr_model_dir')}
                    >
                      {s.browse}
                    </button>
                  </div>
                </div>
                <div className="field">
                  <label>{s.alignerKind}</label>
                  <Seg
                    value={alignerKind(draft.aligner_model)}
                    onChange={(id) => setAligner(id as 'ctc' | 'qwen')}
                    options={[
                      { id: 'ctc', label: s.ctc },
                      { id: 'qwen', label: s.qwen }
                    ]}
                  />
                </div>
                <div className="field">
                  <label>{s.alignerPath}</label>
                  <div className="path-input compact">
                    <input
                      className="text-input mono"
                      value={draft.aligner_model_dir}
                      onChange={(e) => set('aligner_model_dir', e.target.value)}
                    />
                    <button
                      type="button"
                      className="btn btn-ghost btn-sm"
                      onClick={() => onPickModel('aligner_model_dir')}
                    >
                      {s.browse}
                    </button>
                  </div>
                </div>
                <div className="pref-row">
                  <span className="pref-label">{s.vocalSep}</span>
                  <Toggle on={draft.vocal_separation} onChange={(v) => set('vocal_separation', v)} />
                </div>
                <div className="field">
                  <label>{s.demucsPath}</label>
                  <div className="path-input compact">
                    <input
                      className="text-input mono"
                      value={draft.demucs_model_dir}
                      onChange={(e) => set('demucs_model_dir', e.target.value)}
                    />
                    <button
                      type="button"
                      className="btn btn-ghost btn-sm"
                      onClick={() => onPickModel('demucs_model_dir')}
                    >
                      {s.browse}
                    </button>
                  </div>
                </div>
                <div className="field">
                  <label>{s.sensevoicePath}</label>
                  <div className="path-input compact">
                    <input
                      className="text-input mono"
                      value={draft.sensevoice_model_dir || ''}
                      onChange={(e) => set('sensevoice_model_dir', e.target.value)}
                    />
                    <button
                      type="button"
                      className="btn btn-ghost btn-sm"
                      onClick={() => onPickModel('sensevoice_model_dir')}
                    >
                      {s.browse}
                    </button>
                  </div>
                </div>
              </>
            )}

            {activeTab === 'hotwords' && (
              <>
                {hotwordDraft ? (
                  <HotwordEditor
                    lang={lang}
                    value={hotwordDraft}
                    onChange={onHotwordsChange}
                    compact
                  />
                ) : (
                  <p className="hint" style={{ color: 'var(--muted)' }}>…</p>
                )}
              </>
            )}

            {activeTab === 'compute' && (
              <>
                <div className="field">
                  <label>{s.computeBackend}</label>
                  <Seg
                    value={draft.backend}
                    onChange={(id) => set('backend', id as Settings['backend'])}
                    options={[
                      { id: 'auto', label: s.auto },
                      { id: 'gpu', label: s.gpu },
                      { id: 'cpu', label: s.cpu }
                    ]}
                  />
                </div>
                <div className="pref-row">
                  <span className="pref-label">{s.sounds}</span>
                  <Toggle on={draft.sound} onChange={(v) => set('sound', v)} />
                </div>
              </>
            )}
          </div>
        </div>

        <div className="modal-footer">
          {activeTab === 'hotwords' && hotwordDraft ? (
            <div className="hw-default-toggle" title={s.defaultGroup}>
              <span className="pref-label">{s.defaultGroupShort}</span>
              <button
                type="button"
                className={`toggle ${hotwordDraft.default_group_enabled ? 'on' : ''}`}
                role="switch"
                aria-checked={hotwordDraft.default_group_enabled}
                aria-label={s.defaultGroup}
                onClick={() =>
                  onHotwordsChange({
                    ...hotwordDraft,
                    default_group_enabled: !hotwordDraft.default_group_enabled,
                    default_group:
                      hotwordDraft.default_group || hotwordDraft.groups[0]?.id || null
                  })
                }
              />
            </div>
          ) : null}
          <span className="hint">{dirty ? s.unsaved : ''}</span>
          <button type="button" className="btn btn-ghost" onClick={onReset}>
            {s.reset}
          </button>
          <button type="button" className="btn btn-primary" disabled={!dirty} onClick={onSave}>
            {s.save}
          </button>
        </div>
      </motion.div>
    </div>
  )
}

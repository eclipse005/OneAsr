import { useEffect, useState } from 'react'
import { AnimatePresence, motion } from 'framer-motion'
import { LANG_OPTIONS, t, UiLang } from '../i18n'
import type { DownloadState, HotwordStore, Settings, StatusSnapshot } from '../../../shared/types'
import { HotwordEditor } from './HotwordPanel'
import { ConfirmStrip } from './ConfirmStrip'
import { SelectMenu } from './Popover'
import { useEscClose } from '../esc'
import { usePaneMotion, useSurfaceMotion } from '../motion'
import { formatSize, gib } from '../lib/format'

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
  onChange,
  disabled = false,
  title
}: {
  checked: boolean
  label: string
  onChange: (v: boolean) => void
  disabled?: boolean
  title?: string
}) {
  return (
    <button
      type="button"
      className={`check-chip ${checked ? 'active' : ''}`}
      aria-pressed={checked}
      disabled={disabled}
      title={title}
      onClick={() => onChange(!checked)}
    >
      <span className="check-mark" aria-hidden>
        {checked ? '✓' : ''}
      </span>
      {label}
    </button>
  )
}

/** 系统页的诊断日志：大小是当前文件加上一份轮转，清理删掉这两份。 */
function DiagnosticsLog({ s }: { s: ReturnType<typeof t> }) {
  const [bytes, setBytes] = useState<number | null>(null)
  const [busy, setBusy] = useState(false)
  useEffect(() => {
    let alive = true
    void window.oneasr.logInfo().then((info) => {
      if (alive) setBytes(info.bytes)
    })
    return () => {
      alive = false
    }
  }, [])
  const empty = bytes == null || bytes === 0
  return (
    <div className="pref-block">
      <div className="pref-row">
        <span className="pref-copy">
          <span className="pref-label">{s.diagnostics}</span>
          {bytes != null ? (
            <span className="pref-note log-size">{formatSize(bytes) ?? s.logEmpty}</span>
          ) : null}
        </span>
        <span className="pref-actions">
          <button
            type="button"
            className="btn btn-ghost btn-sm"
            disabled={empty || busy}
            onClick={() => {
              setBusy(true)
              void window.oneasr
                .clearLog()
                .then((info) => setBytes(info.bytes))
                .finally(() => setBusy(false))
            }}
          >
            {s.clearLog}
          </button>
          <button type="button" className="btn btn-ghost btn-sm" onClick={() => void window.oneasr.openLog()}>
            {s.openLog}
          </button>
        </span>
      </div>
    </div>
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
  status,
  download,
  onDownloadStart,
  onDownloadCancel,
  dirty,
  savedAck = false,
  hotwordDraft,
  initialTab = 'general',
  onChange,
  onHotwordsChange,
  onSave,
  onReset,
  onRestoreDefaults,
  onClose,
  onPickDir,
  onPickModel,
  onOpenModels,
  onRefreshModels
}: {
  lang: UiLang
  draft: Settings
  /** CLI 的 status --json：就绪判据、门禁、可选模型清单都在里面 */
  status: StatusSnapshot | null
  /** 当前下载进度（null = 没在下） */
  download: DownloadState | null
  onDownloadStart: (id: string) => void
  onDownloadCancel: () => void
  dirty: boolean
  /** 刚保存成功：主按钮短暂保持主色并显示「已保存」 */
  savedAck?: boolean
  hotwordDraft: HotwordStore | null
  initialTab?: SettingsTab
  onChange: (next: Settings) => void
  onHotwordsChange: (next: HotwordStore) => void
  onSave: () => void
  /** 放弃未保存的改动（回到已保存的值） */
  onReset: () => void
  /** 恢复默认设置（会落盘） */
  onRestoreDefaults: () => void
  onClose: () => void
  onPickDir: () => void
  onPickModel: (field: 'asr_model_dir' | 'aligner_model_dir' | 'demucs_model_dir' | 'sensevoice_model_dir') => void
  onOpenModels: () => void
  onRefreshModels: () => void
}) {
  const s = t(lang)
  const surface = useSurfaceMotion()
  const pane = usePaneMotion()
  /** 没装的模型；下载中的那条也在里面（它本来就是缺的） */
  // 缺哪些模型由 CLI 的 catalog 说（目录名也来自 Rust，壳不再硬编码）
  const missingModels = (status?.catalog ?? []).filter((m) => !m.ready)
  // 只有"进行中"才算忙碌：终态（完成/失败/取消）不能让其它模型的按钮一直禁用
  const downloading = download?.state === 'downloading' || download?.state === 'verifying'
  const [activeTab, setActiveTab] = useState<SettingsTab>(initialTab)
  useEffect(() => {
    setActiveTab(initialTab)
  }, [initialTab])
  const set = <K extends keyof Settings>(key: K, value: Settings[K]) =>
    onChange({ ...draft, [key]: value })

  /** 有未保存改动时先确认，别让人手一滑丢掉刚调的参数 */
  const [confirmDiscard, setConfirmDiscard] = useState(false)
  /** 删词组：和放弃更改同一条确认条，不再走浏览器 confirm */
  const [deleteGroup, setDeleteGroup] = useState<(() => void) | null>(null)
  const tryClose = () => {
    if (deleteGroup) {
      setDeleteGroup(null)
      return
    }
    if (confirmDiscard) {
      setConfirmDiscard(false)
      return
    }
    if (dirty) {
      setConfirmDiscard(true)
      return
    }
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

  useEscClose(tryClose)

  return (
    <div className="overlay" onClick={tryClose}>
      <motion.div
        className="modal-shell"
        role="dialog"
        aria-modal="true"
        onClick={(e) => e.stopPropagation()}
        {...surface}
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
            <AnimatePresence mode="wait" initial={false}>
            <motion.div key={activeTab} className="settings-pane-inner" {...pane}>
            {activeTab === 'general' && (
              <>
                <div className="field">
                  <label>{s.uiLanguage}</label>
                  <Seg
                    value={draft.ui_language}
                    onChange={(id) => set('ui_language', id)}
                    options={[
                      { id: 'system', label: s.system },
                      { id: 'zh', label: s.uiLanguageZh },
                      { id: 'en', label: s.uiLanguageEn }
                    ]}
                  />
                </div>
                <div className="field">
                  <label>{s.recognitionLanguage}</label>
                  <SelectMenu
                    className="select-menu"
                    value={draft.language}
                    options={LANG_OPTIONS.map((o) => ({ id: o.id, label: o.label }))}
                    ariaLabel={s.recognitionLanguage}
                    onChange={(id) => set('language', id)}
                  />
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
                  <Seg
                    value={String(draft.chunk_target_seconds)}
                    onChange={(id) => set('chunk_target_seconds', Number(id))}
                    options={[30, 60, 90, 120, 180].map((n) => ({ id: String(n), label: String(n) }))}
                  />
                </div>
              </>
            )}

            {activeTab === 'output' && (
              <>
                <div className="field">
                  <label>{s.outputs}</label>
                  <div className="check-chip-row">
                    {(
                      [
                        ['output_srt', 'SRT'],
                        ['output_txt', 'TXT'],
                        ['output_ass', 'ASS']
                      ] as const
                    ).map(([key, label]) => {
                      const on = draft[key]
                      const alone = on && [draft.output_srt, draft.output_txt, draft.output_ass].filter(Boolean).length === 1
                      return (
                        <CheckChip
                          key={key}
                          checked={on}
                          label={label}
                          disabled={alone}
                          title={alone ? s.outputKeepOne : undefined}
                          onChange={(v) => {
                            if (!v && alone) return
                            set(key, v)
                          }}
                        />
                      )
                    })}
                  </div>
                </div>
                {/* 三种全关时，CLI 仍会保留 SRT（--no-srt 必须配合 --txt/--ass）。
                    界面不说这一句，用户就会以为「什么都不产出」。 */}
                {![draft.output_srt, draft.output_txt, draft.output_ass].some(Boolean) && (
                  <p className="field-hint warn">{s.outputNoneOn}</p>
                )}
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
                    <Pill ok={!!status?.models.asr.ready} label={s.asr} ready={s.ready} missing={s.missing} />
                    <Pill
                      ok={!!status?.models.aligner.ready}
                      label={s.aligner}
                      ready={s.ready}
                      missing={s.missing}
                    />
                    <Pill
                      ok={!!status?.models.demucs.ready}
                      label={s.demucs}
                      ready={s.ready}
                      missing={s.missing}
                    />
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

                {/* 模型下载：只列缺的（都齐了就用一行带过）——固定高度的面板不能被撑爆 */}
                <div className="dl-block">
                  <div className="dl-head">{s.downloadSection}</div>
                  {missingModels.length === 0 && !download ? (
                    <p className="dl-all">{s.allModelsReady}</p>
                  ) : (
                    <div className="dl-list">
                      {missingModels.map((m) => {
                        const active = download?.model === m.id
                        const pct = active ? Math.round(download?.pct ?? 0) : 0
                        return (
                          <div className="dl-row" key={m.id}>
                            <span className="dl-name">{m.id}</span>
                            {active && downloading && (
                              <span className="dl-progress">
                                <span className="dl-bar">
                                  <i style={{ width: `${pct}%` }} />
                                </span>
                                <span className="dl-meta">
                                  {download?.state === 'verifying'
                                    ? s.verifying
                                    : `${pct}% · ${gib(download?.doneBytes ?? 0)} / ${gib(download?.totalBytes ?? 0)}`}
                                </span>
                              </span>
                            )}
                            {active && downloading ? (
                              <button
                                type="button"
                                className="btn btn-ghost btn-sm"
                                onClick={onDownloadCancel}
                              >
                                {s.cancelDownload}
                              </button>
                            ) : (
                              <button
                                type="button"
                                className="btn btn-ghost btn-sm"
                                disabled={downloading}
                                onClick={() => onDownloadStart(m.id)}
                              >
                                {downloading ? s.downloading : s.download}
                              </button>
                            )}
                          </div>
                        )
                      })}
                    </div>
                  )}
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
                    onAskDeleteGroup={setDeleteGroup}
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
                <div className="pref-row">
                  <span className="pref-label">{s.closeToTray}</span>
                  <Toggle
                    on={!!draft.close_to_tray}
                    onChange={(v) => set('close_to_tray', v)}
                  />
                </div>
                <DiagnosticsLog s={s} />
              </>
            )}
            </motion.div>
            </AnimatePresence>
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
            {s.discardChanges}
          </button>
          <button type="button" className="btn btn-ghost" onClick={onRestoreDefaults}>
            {s.restoreDefaults}
          </button>
          <button
            type="button"
            className={`btn btn-primary ${savedAck ? 'is-acked' : ''}`}
            disabled={!dirty}
            onClick={onSave}
          >
            {savedAck ? s.saved : s.save}
          </button>
        </div>

        <AnimatePresence>
          {deleteGroup && (
            <ConfirmStrip
              key="delete-group"
              danger
              message={s.confirmDeleteGroup}
              cancelLabel={s.cancel}
              confirmLabel={s.deleteGroup}
              onCancel={() => setDeleteGroup(null)}
              onConfirm={() => {
                deleteGroup()
                setDeleteGroup(null)
              }}
            />
          )}
          {confirmDiscard && !deleteGroup && (
            <ConfirmStrip
              key="discard"
              message={s.confirmDiscard}
              cancelLabel={s.cancel}
              confirmLabel={s.discardChanges}
              onCancel={() => setConfirmDiscard(false)}
              onConfirm={() => {
                setConfirmDiscard(false)
                onClose()
              }}
            />
          )}
        </AnimatePresence>
      </motion.div>
    </div>
  )
}

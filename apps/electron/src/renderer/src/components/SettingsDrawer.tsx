import { useEffect, useState } from 'react'
import { AnimatePresence, motion } from 'framer-motion'
import { LANG_OPTIONS, t, UiLang } from '../i18n'
import type { DownloadState, HotwordStore, Settings, StatusSnapshot } from '../../../shared/types'
import { HotwordEditor } from './HotwordPanel'
import { ConfirmDialog } from './ConfirmDialog'
import { SelectMenu } from './Popover'
import { useEscClose } from '../esc'
import { usePaneMotion, useSurfaceMotion } from '../motion'
import { formatSize } from '../lib/format'

export type SettingsTab = 'general' | 'output' | 'models' | 'hotwords' | 'compute'

function Toggle({
  on,
  onChange,
  'aria-label': ariaLabel
}: {
  on: boolean
  onChange: (v: boolean) => void
  'aria-label'?: string
}) {
  return (
    <button
      type="button"
      className={`toggle ${on ? 'on' : ''}`}
      role="switch"
      aria-checked={on}
      aria-label={ariaLabel}
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

/** CLI 报的目录可能用 `/`，设置里可能用 `\`；Windows 路径还要忽略大小写。 */
function samePath(a: string, b: string): boolean {
  const trim = (p: string) => p.replace(/\/+$/, '')
  if (window.oneasr.platform === 'win32') {
    return trim(a).replace(/\\/g, '/').toLowerCase() === trim(b).replace(/\\/g, '/').toLowerCase()
  }
  return trim(a) === trim(b)
}

function modelSlotReady(
  status: StatusSnapshot | null,
  family: 'asr' | 'aligner' | 'demucs' | 'sensevoice',
  modelId: string,
  dir: string
): boolean {
  const slot = status?.models[family]
  return !!slot?.ready && slot.id === modelId && samePath(slot.dir, dir)
}

/** status 明确说“这个模型在这个目录”缺失时，旧的 done 下载态要作废。 */
function modelSlotMissing(
  status: StatusSnapshot | null,
  family: 'asr' | 'aligner' | 'demucs' | 'sensevoice',
  modelId: string,
  dir: string
): boolean {
  const slot = status?.models[family]
  return !!slot && slot.id === modelId && samePath(slot.dir, dir) && !slot.ready
}

/** 快速校验（已下载模型点「重新下载」）不该闪一下下载态；超过 delay 才显示。 */
function useDelayedBusy(active: boolean, delay = 400): boolean {
  const [visible, setVisible] = useState(false)
  useEffect(() => {
    if (!active) {
      setVisible(false)
      return
    }
    const timer = window.setTimeout(() => setVisible(true), delay)
    return () => window.clearTimeout(timer)
  }, [active, delay])
  return visible
}

function ModelPathField({
  label,
  value,
  onChange,
  onBrowse,
  modelId,
  ready,
  statusMissing,
  statusAt,
  download,
  browseLabel,
  downloadLabel,
  redownloadLabel,
  downloadingLabel,
  verifyingLabel,
  retryLabel,
  cancelLabel,
  onDownload,
  onCancel,
  dataTour
}: {
  label: string
  value: string
  onChange: (v: string) => void
  onBrowse: () => void
  modelId: string
  ready: boolean
  /** status 明确说这个模型缺失：不要让旧的 done 下载态把图标顶成“重新下载” */
  statusMissing: boolean
  statusAt: number
  download: DownloadState | null
  browseLabel: string
  downloadLabel: string
  redownloadLabel: string
  downloadingLabel: string
  verifyingLabel: string
  retryLabel: string
  cancelLabel: string
  onDownload: (id: string, targetDir?: string) => void
  onCancel: (id: string) => void
  dataTour?: string
}) {
  // 每个模型一份状态，可以同时下多个；同一行的快速校验不闪下载态。
  const busy = download?.state === 'downloading' || download?.state === 'verifying'
  const showBusy = useDelayedBusy(busy)
  const isDown = busy && showBusy
  const isErr = download?.state === 'error'
  const pct = Math.round(download?.pct ?? 0)
  // 下载到当前显示的目录且已成功：先显示「重新下载」，别让用户以为没下成。
  // status 说同一路径缺失时，只有 done 比这份 status 快照更晚才信 done；
  // 用户删掉模型再刷新后 status 更新，旧的 done 就不再顶图标。
  const doneAfterStatus = !!download?.finishedAt && download.finishedAt > statusAt
  const downloadedHere =
    download?.state === 'done' &&
    (!download.dir || samePath(download.dir, value)) &&
    (!statusMissing || doneAfterStatus)
  const readyNow = ready || downloadedHere
  let actionLabel = readyNow ? redownloadLabel : downloadLabel
  if (isErr) actionLabel = retryLabel
  const progressLabel =
    download?.state === 'verifying'
      ? verifyingLabel
      : pct > 0
        ? `${pct}%`
        : downloadingLabel
  const cancelTitle = isDown ? `${cancelLabel} · ${progressLabel}` : cancelLabel
  return (
    <div className="field">
      <label>{label}</label>
      <div className="model-path-stack">
        <div className="path-input compact">
          <div className={`path-field${isDown ? ' is-downloading' : ''}`}>
            {isDown ? (
              <span className="path-progress" style={{ width: `${pct}%` }} aria-hidden />
            ) : null}
            <input
              className="text-input mono"
              value={value}
              onChange={(e) => onChange(e.target.value)}
            />
            {isDown ? (
              <span className="path-progress-label" aria-hidden>
                {progressLabel}
              </span>
            ) : null}
          </div>
          <button
            type="button"
            className="icon-btn"
            title={browseLabel}
            aria-label={browseLabel}
            onClick={onBrowse}
          >
            <span className="icon-glyph i-folder" aria-hidden />
          </button>
          {isDown ? (
            <button
              type="button"
              className="icon-btn"
              data-tour={dataTour}
              /* title 不跟着 pct 改：原生 tooltip 会因 title 变化重新弹出，鼠标没动也闪。 */
              title={cancelLabel}
              aria-label={cancelTitle}
              onClick={() => onCancel(modelId)}
            >
              <span className="icon-glyph i-stop" aria-hidden />
            </button>
          ) : (
            <button
              type="button"
              className={`icon-btn ${readyNow && !isErr ? '' : 'primary'}`}
              data-tour={dataTour}
              disabled={busy}
              title={actionLabel}
              aria-label={actionLabel}
              onClick={() => onDownload(modelId, value)}
            >
              {/* 已就绪 → 重新下载；缺失 → 下载；失败 → 重试（同一枚循环箭头）。 */}
              <span className={`icon-glyph ${readyNow || isErr ? 'i-retry' : 'i-download'}`} aria-hidden />
            </button>
          )}
        </div>
      </div>
    </div>
  )
}

export function SettingsDrawer({
  lang,
  draft,
  status,
  statusAt,
  downloads,
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
  /** 上一次 status 快照的时间；和下载完成时间比谁更新 */
  statusAt: number
  /** 每个模型各自的下载状态；不同模型可以同时下 */
  downloads: Record<string, DownloadState>
  onDownloadStart: (id: string, targetDir?: string) => void
  onDownloadCancel: (id: string) => void
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
  onPickModel: (
    field: 'asr_model_dir' | 'aligner_model_dir' | 'demucs_model_dir' | 'sensevoice_model_dir',
    modelId?: string
  ) => void
  onOpenModels: () => void
  onRefreshModels: () => void
}) {
  const s = t(lang)
  const surface = useSurfaceMotion()
  const pane = usePaneMotion()
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
          <button type="button" data-shot="close-modal" data-tour="settings-close" className="btn btn-ghost" onClick={tryClose}>
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
                data-tour={`settings-tab-${tab.id}`}
                className={`settings-nav-item ${activeTab === tab.id ? 'active' : ''}`}
                aria-current={activeTab === tab.id ? 'page' : undefined}
                onClick={() => setActiveTab(tab.id)}
              >
                {tab.label}
              </button>
            ))}
          </nav>

          <div
            className={`settings-pane${activeTab === 'hotwords' ? ' is-hotwords' : ''}${activeTab === 'models' ? ' is-models' : ''}`}
            data-shot={`settings-pane-${activeTab}`}
            data-tour={`settings-pane-${activeTab}`}
          >
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
                  <div className="models-status" onClick={onRefreshModels} title={s.refreshModelsStatus}>
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
                    <button
                      type="button"
                      className="icon-btn models-refresh-btn"
                      onClick={onRefreshModels}
                      title={s.refreshModelsStatus}
                      aria-label={s.refreshModelsStatus}
                    >
                      <span className="models-refresh-glyph" aria-hidden>
                        ↻
                      </span>
                    </button>
                    <button type="button" className="btn btn-ghost btn-xs" onClick={onOpenModels}>
                      {s.openModels}
                    </button>
                  </div>
                </div>

                <div className="size-quant-row">
                  <div className="size-quant-half">
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
                  <div className="size-quant-half">
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
                </div>
                <ModelPathField
                  dataTour="model-download-asr"
                  label={s.asrPath}
                  value={draft.asr_model_dir}
                  onChange={(v) => {
                    const id = asrId(size, quant)
                    onChange({
                      ...draft,
                      asr_model_dir: v,
                      asr_dirs: { ...draft.asr_dirs, [id]: v }
                    })
                  }}
                  onBrowse={() => onPickModel('asr_model_dir', asrId(size, quant))}
                  modelId={asrId(size, quant)}
                  ready={modelSlotReady(status, 'asr', asrId(size, quant), draft.asr_model_dir)}
                  statusMissing={modelSlotMissing(status, 'asr', asrId(size, quant), draft.asr_model_dir)}
                  statusAt={statusAt}
                  download={downloads[asrId(size, quant)] ?? null}
                  browseLabel={s.browse}
                  downloadLabel={s.download}
                  redownloadLabel={s.redownload}
                  downloadingLabel={s.downloading}
                  verifyingLabel={s.verifying}
                  retryLabel={s.modelRetry}
                  cancelLabel={s.cancelDownload}
                  onDownload={onDownloadStart}
                  onCancel={onDownloadCancel}
                />

                <div className="pane-divider" />

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
                <ModelPathField
                  label={s.alignerPath}
                  value={draft.aligner_model_dir}
                  onChange={(v) => {
                    const id = draft.aligner_model
                    onChange({
                      ...draft,
                      aligner_model_dir: v,
                      aligner_dirs: { ...draft.aligner_dirs, [id]: v }
                    })
                  }}
                  onBrowse={() => onPickModel('aligner_model_dir', draft.aligner_model)}
                  modelId={draft.aligner_model}
                  ready={modelSlotReady(status, 'aligner', draft.aligner_model, draft.aligner_model_dir)}
                  statusMissing={modelSlotMissing(status, 'aligner', draft.aligner_model, draft.aligner_model_dir)}
                  statusAt={statusAt}
                  download={downloads[draft.aligner_model] ?? null}
                  browseLabel={s.browse}
                  downloadLabel={s.download}
                  redownloadLabel={s.redownload}
                  downloadingLabel={s.downloading}
                  verifyingLabel={s.verifying}
                  retryLabel={s.modelRetry}
                  cancelLabel={s.cancelDownload}
                  onDownload={onDownloadStart}
                  onCancel={onDownloadCancel}
                />

                <div className="pane-divider" />

                <div className="pref-row">
                  <span className="pref-label">{s.vocalSep}</span>
                  <div className="hw-default-toggle" title={s.vocalSepDefault}>
                    <span className="pref-label">{s.vocalSepDefaultShort}</span>
                    <Toggle
                      on={draft.vocal_separation}
                      onChange={(v) => set('vocal_separation', v)}
                      aria-label={s.vocalSepDefault}
                    />
                  </div>
                </div>
                <ModelPathField
                  label={s.demucsPath}
                  value={draft.demucs_model_dir}
                  onChange={(v) => set('demucs_model_dir', v)}
                  onBrowse={() => onPickModel('demucs_model_dir')}
                  modelId="htdemucs_ft"
                  ready={modelSlotReady(status, 'demucs', 'htdemucs_ft', draft.demucs_model_dir)}
                  statusMissing={modelSlotMissing(status, 'demucs', 'htdemucs_ft', draft.demucs_model_dir)}
                  statusAt={statusAt}
                  download={downloads.htdemucs_ft ?? null}
                  browseLabel={s.browse}
                  downloadLabel={s.download}
                  redownloadLabel={s.redownload}
                  downloadingLabel={s.downloading}
                  verifyingLabel={s.verifying}
                  retryLabel={s.modelRetry}
                  cancelLabel={s.cancelDownload}
                  onDownload={onDownloadStart}
                  onCancel={onDownloadCancel}
                />
                <ModelPathField
                  label={s.sensevoicePath}
                  value={draft.sensevoice_model_dir || ''}
                  onChange={(v) => set('sensevoice_model_dir', v)}
                  onBrowse={() => onPickModel('sensevoice_model_dir')}
                  modelId="SenseVoiceSmall"
                  ready={modelSlotReady(status, 'sensevoice', 'SenseVoiceSmall', draft.sensevoice_model_dir || '')}
                  statusMissing={modelSlotMissing(status, 'sensevoice', 'SenseVoiceSmall', draft.sensevoice_model_dir || '')}
                  statusAt={statusAt}
                  download={downloads.SenseVoiceSmall ?? null}
                  browseLabel={s.browse}
                  downloadLabel={s.download}
                  redownloadLabel={s.redownload}
                  downloadingLabel={s.downloading}
                  verifyingLabel={s.verifying}
                  retryLabel={s.modelRetry}
                  cancelLabel={s.cancelDownload}
                  onDownload={onDownloadStart}
                  onCancel={onDownloadCancel}
                />
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
            <ConfirmDialog
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
            <ConfirmDialog
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

import { motion } from 'framer-motion'
import { LANG_OPTIONS, t, UiLang } from '../i18n'
import type { Settings } from '../../../shared/types'

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
  dirty,
  onChange,
  onSave,
  onReset,
  onClose,
  onPickDir,
  onPickModel
}: {
  lang: UiLang
  draft: Settings
  dirty: boolean
  onChange: (next: Settings) => void
  onSave: () => void
  onReset: () => void
  onClose: () => void
  onPickDir: () => void
  onPickModel: (field: 'asr_model_dir' | 'aligner_model_dir' | 'demucs_model_dir') => void
}) {
  const s = t(lang)
  const set = <K extends keyof Settings>(key: K, value: Settings[K]) =>
    onChange({ ...draft, [key]: value })

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
    const dir =
      draft.aligner_dirs[id] ||
      draft.aligner_model_dir.replace(/[^/]+$/, id)
    onChange({
      ...draft,
      aligner_model: id,
      aligner_model_dir: dir,
      aligner_dirs: { ...draft.aligner_dirs, [id]: dir }
    })
  }

  return (
    <div className="overlay" onClick={onClose}>
      <motion.aside
        className="drawer"
        onClick={(e) => e.stopPropagation()}
        initial={{ x: 40, opacity: 0 }}
        animate={{ x: 0, opacity: 1 }}
        exit={{ x: 48, opacity: 0 }}
        transition={{ type: 'spring', stiffness: 380, damping: 34 }}
      >
        <div className="drawer-header">
          <h3>{s.settings}</h3>
          <div style={{ flex: 1 }} />
          <button className="btn btn-ghost" onClick={onClose}>
            {s.close}
          </button>
        </div>
        <div className="drawer-body">
          <div className="section">
            <h4>{s.sectionGeneral}</h4>
            <div className="field">
              <label>{s.uiLanguage}</label>
              <select
                value={draft.ui_language}
                onChange={(e) => set('ui_language', e.target.value)}
              >
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
              <div className="chip-row">
                {(['short', 'standard', 'loose'] as const).map((p) => (
                  <button
                    key={p}
                    className={`chip ${draft.subtitle_length_preset === p ? 'active' : ''}`}
                    onClick={() => set('subtitle_length_preset', p)}
                  >
                    {s[p]}
                  </button>
                ))}
              </div>
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
          </div>

          <div className="section">
            <h4>{s.sectionOutput}</h4>
            <div className="field">
              <label>{s.outputs}</label>
              <div className="chip-row">
                <button
                  className={`chip ${draft.output_srt ? 'active' : ''}`}
                  onClick={() => set('output_srt', !draft.output_srt)}
                >
                  SRT
                </button>
                <button
                  className={`chip ${draft.output_txt ? 'active' : ''}`}
                  onClick={() => set('output_txt', !draft.output_txt)}
                >
                  TXT
                </button>
                <button
                  className={`chip ${draft.output_ass ? 'active' : ''}`}
                  onClick={() => set('output_ass', !draft.output_ass)}
                >
                  ASS
                </button>
              </div>
            </div>
            <div className="field">
              <label>{s.chineseScript}</label>
              <div className="chip-row">
                {(['simplified', 'traditional', 'original'] as const).map((p) => (
                  <button
                    key={p}
                    className={`chip ${draft.text_script === p ? 'active' : ''}`}
                    onClick={() => set('text_script', p)}
                  >
                    {s[p]}
                  </button>
                ))}
              </div>
            </div>
            <div className="field">
              <label>{s.subtitlePolish}</label>
              <Toggle on={draft.subtitle_beautify} onChange={(v) => set('subtitle_beautify', v)} />
            </div>
            <div className="field">
              <label>{s.saveNextToSource}</label>
              <Toggle
                on={draft.save_next_to_source}
                onChange={(v) => set('save_next_to_source', v)}
              />
            </div>
            <div className="field stack">
              <label>{s.outputPath}</label>
              <div className="path-input">
                <input
                  className="text-input"
                  value={draft.output_dir}
                  onChange={(e) => set('output_dir', e.target.value)}
                />
                <button className="btn btn-ghost" onClick={onPickDir}>
                  {s.browse}
                </button>
              </div>
            </div>
          </div>

          <div className="section">
            <h4>{s.sectionModels}</h4>
            <div className="field">
              <label>{s.asrSize}</label>
              <div className="chip-row">
                <button
                  className={`chip ${size === '0.6B' ? 'active' : ''}`}
                  onClick={() => setAsr('0.6B', quant)}
                >
                  0.6B
                </button>
                <button
                  className={`chip ${size === '1.7B' ? 'active' : ''}`}
                  onClick={() => setAsr('1.7B', quant)}
                >
                  1.7B
                </button>
              </div>
            </div>
            <div className="field">
              <label>{s.quantization}</label>
              <div className="chip-row">
                <button
                  className={`chip ${quant === 'fp16' ? 'active' : ''}`}
                  onClick={() => setAsr(size, 'fp16')}
                >
                  {s.fp16}
                </button>
                <button
                  className={`chip ${quant === 'int8' ? 'active' : ''}`}
                  onClick={() => setAsr(size, 'int8')}
                >
                  {s.int8}
                </button>
              </div>
            </div>
            <div className="field stack">
              <label>{s.asrPath}</label>
              <div className="path-input">
                <input
                  className="text-input"
                  value={draft.asr_model_dir}
                  onChange={(e) => set('asr_model_dir', e.target.value)}
                />
                <button className="btn btn-ghost" onClick={() => onPickModel('asr_model_dir')}>
                  {s.browse}
                </button>
              </div>
            </div>
            <div className="field">
              <label>{s.alignerKind}</label>
              <div className="chip-row">
                <button
                  className={`chip ${alignerKind(draft.aligner_model) === 'ctc' ? 'active' : ''}`}
                  onClick={() => setAligner('ctc')}
                >
                  {s.ctc}
                </button>
                <button
                  className={`chip ${alignerKind(draft.aligner_model) === 'qwen' ? 'active' : ''}`}
                  onClick={() => setAligner('qwen')}
                >
                  {s.qwen}
                </button>
              </div>
            </div>
            <div className="field stack">
              <label>{s.alignerPath}</label>
              <div className="path-input">
                <input
                  className="text-input"
                  value={draft.aligner_model_dir}
                  onChange={(e) => set('aligner_model_dir', e.target.value)}
                />
                <button className="btn btn-ghost" onClick={() => onPickModel('aligner_model_dir')}>
                  {s.browse}
                </button>
              </div>
            </div>
            <div className="field">
              <label>{s.vocalSep}</label>
              <Toggle
                on={draft.vocal_separation}
                onChange={(v) => set('vocal_separation', v)}
              />
            </div>
            <div className="field stack">
              <label>{s.demucsPath}</label>
              <div className="path-input">
                <input
                  className="text-input"
                  value={draft.demucs_model_dir}
                  onChange={(e) => set('demucs_model_dir', e.target.value)}
                />
                <button className="btn btn-ghost" onClick={() => onPickModel('demucs_model_dir')}>
                  {s.browse}
                </button>
              </div>
            </div>
          </div>

          <div className="section">
            <h4>{s.sectionCompute}</h4>
            <div className="field">
              <label>{s.computeBackend}</label>
              <div className="chip-row">
                {(['auto', 'gpu', 'cpu'] as const).map((b) => (
                  <button
                    key={b}
                    className={`chip ${draft.backend === b ? 'active' : ''}`}
                    onClick={() => set('backend', b)}
                  >
                    {s[b]}
                  </button>
                ))}
              </div>
            </div>
            <div className="field">
              <label>{s.sounds}</label>
              <Toggle on={draft.sound} onChange={(v) => set('sound', v)} />
            </div>
          </div>
        </div>
        <div className="drawer-footer">
          <span className="hint">{dirty ? s.unsaved : ''}</span>
          <button className="btn btn-ghost" onClick={onReset}>
            {s.reset}
          </button>
          <button className="btn btn-primary" disabled={!dirty} onClick={onSave}>
            {s.save}
          </button>
        </div>
      </motion.aside>
    </div>
  )
}

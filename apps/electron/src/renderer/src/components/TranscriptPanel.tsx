import { AnimatePresence, motion } from 'framer-motion'
import { useEffect, useRef, useState } from 'react'
import { ConfirmStrip } from './ConfirmStrip'
import { useEscClose } from '../esc'
import { useSurfaceMotion } from '../motion'
import { t, transcriptRate, transcriptSummary, type UiLang } from '../i18n'
import type { TranscriptInfo } from '../../../shared/types'

interface TranscriptPanelProps {
  lang: UiLang
  info: TranscriptInfo
  /** 主进程里当前暂存的正文；面板的草稿与它不同就是 dirty */
  text: string
  locked: boolean
  onClose: () => void
  /** 只保存。打轴由这一行外面的开始按钮发起。 */
  onSave: (text: string) => Promise<boolean>
  onSplit: (text: string) => Promise<void>
  onReread: () => Promise<void>
  onRemove: () => void
}

/**
 * 文稿面板：左边摘要（行数 / 字数 / 语速 / 原时间轴提示），右边可编辑正文。
 *
 * 比旧版多出来的是**编辑器**：Electron 有原生 textarea，不需要像 gpui 那样
 * 自己实现光标、选区、撤销与 IME。保存只改主进程里的暂存副本，原文件永远不写。
 */
export function TranscriptPanel({
  lang,
  info,
  text,
  locked,
  onClose,
  onSave,
  onSplit,
  onReread,
  onRemove
}: TranscriptPanelProps) {
  const s = t(lang)
  const surface = useSurfaceMotion()
  const [draft, setDraft] = useState(text)
  const [busy, setBusy] = useState(false)
  const [confirmClose, setConfirmClose] = useState(false)
  const [savedAck, setSavedAck] = useState(false)
  const ackTimer = useRef<number | undefined>(undefined)
  const dirty = draft !== text

  // 保存 / 断句 / 重读都会让父层换掉暂存正文；换一次就重置草稿。
  useEffect(() => {
    setDraft(text)
    setConfirmClose(false)
  }, [text])

  useEffect(() => {
    return () => {
      if (ackTimer.current !== undefined) window.clearTimeout(ackTimer.current)
    }
  }, [])

  // 又改了正文，「已保存」就不再成立。
  useEffect(() => {
    if (!dirty) return
    setSavedAck(false)
    if (ackTimer.current !== undefined) {
      window.clearTimeout(ackTimer.current)
      ackTimer.current = undefined
    }
  }, [dirty])

  const requestClose = (): void => {
    if (confirmClose) {
      setConfirmClose(false)
      return
    }
    if (dirty) setConfirmClose(true)
    else onClose()
  }
  useEscClose(requestClose)

  const run = async (fn: () => Promise<unknown>): Promise<void> => {
    if (busy) return
    setBusy(true)
    try {
      await fn()
    } finally {
      setBusy(false)
    }
  }

  const lines = draft.split(/\r?\n/).filter((line) => line.trim()).length
  const chars = Array.from(draft.replace(/\s/g, '')).length
  const editable = !locked && !info.error

  return (
    <div className="overlay" onClick={requestClose}>
      <motion.div
        className="modal-shell transcript-shell"
        role="dialog"
        aria-modal="true"
        onClick={(e) => e.stopPropagation()}
        {...surface}
      >
        <div className="modal-header">
          <h3>{s.transcriptPanelTitle}</h3>
          <div style={{ flex: 1 }} />
          <button type="button" className="btn btn-ghost" onClick={requestClose}>
            {s.close}
          </button>
        </div>

        <div className="transcript-body">
          <aside className="transcript-summary">
            <div>
              <div className="transcript-file">{info.name}</div>
              {info.path && (
                <div className="transcript-path" title={info.path}>
                  {info.path}
                </div>
              )}
            </div>
            <div className="transcript-meta">{transcriptSummary(lang, lines, chars)}</div>
            {info.droppedTimecodes && (
              <div className="transcript-note">{s.transcriptDroppedTimecodes}</div>
            )}
            {info.rate && (
              <div className={`transcript-rate ${info.rate.plausible ? 'ok' : 'bad'}`}>
                {transcriptRate(lang, info.rate)}
                {dirty ? ` · ${s.transcriptDensityStale}` : ''}
              </div>
            )}
            {info.needsRealign && (
              <div className="transcript-warn">{s.transcriptNeedsRealign}</div>
            )}
            {info.error && <div className="transcript-error">{info.error}</div>}
            {locked && <div className="transcript-note">{s.transcriptLocked}</div>}

            <div className="transcript-actions">
              <button
                type="button"
                className="btn btn-ghost btn-sm"
                disabled={!editable || busy || !draft.trim()}
                onClick={() => void run(() => onSplit(draft))}
              >
                {s.transcriptSmartSplit}
              </button>
              {info.path && (
                <button
                  type="button"
                  className="btn btn-ghost btn-sm"
                  disabled={!editable || busy}
                  onClick={() =>
                    void run(async () => {
                      // 重读是显式丢弃草稿；父层返回的正文若与基线相同，
                      // React 不会重渲染，所以这里先把 draft 拉回基线。
                      setDraft(text)
                      await onReread()
                    })
                  }
                >
                  {s.transcriptReread}
                </button>
              )}
              <button
                type="button"
                className="btn btn-ghost btn-danger-ghost btn-sm"
                disabled={!editable || busy}
                onClick={onRemove}
              >
                {s.transcriptRemove}
              </button>
            </div>
            <div className="transcript-promise">{s.transcriptEditorHint}</div>
          </aside>

          <div className="transcript-editor-wrap">
            <textarea
              className="transcript-editor"
              value={draft}
              readOnly={!editable}
              spellCheck={false}
              placeholder={s.transcriptEditorHint}
              onChange={(e) => setDraft(e.target.value)}
            />
            <div className="transcript-editor-foot">
              <span>{transcriptSummary(lang, lines, chars)}</span>
              <span className={dirty ? 'is-dirty' : ''}>
                {dirty ? s.transcriptUnsaved : ''}
              </span>
            </div>
          </div>
        </div>

        <AnimatePresence>
          {confirmClose && (
            <ConfirmStrip
              key="discard-transcript"
              message={s.transcriptConfirmDiscard}
              cancelLabel={s.cancel}
              confirmLabel={s.discardChanges}
              onCancel={() => setConfirmClose(false)}
              onConfirm={onClose}
            />
          )}
        </AnimatePresence>

        <div className="modal-footer">
          <span className="hint">{locked ? s.transcriptLocked : s.transcriptUseRowStart}</span>
          <button type="button" className="btn btn-ghost" onClick={requestClose}>
            {s.cancel}
          </button>
          <button
            type="button"
            className={`btn btn-primary ${savedAck && !dirty ? 'is-acked' : ''}`}
            disabled={!dirty || !editable || busy}
            onClick={() =>
              void run(async () => {
                const ok = await onSave(draft)
                if (!ok) return
                setSavedAck(true)
                if (ackTimer.current !== undefined) window.clearTimeout(ackTimer.current)
                ackTimer.current = window.setTimeout(() => setSavedAck(false), 1400)
              })
            }
          >
            {savedAck && !dirty ? s.saved : s.transcriptSave}
          </button>
        </div>
      </motion.div>
    </div>
  )
}

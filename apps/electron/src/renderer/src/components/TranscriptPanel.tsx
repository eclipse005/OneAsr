import { AnimatePresence, motion } from 'framer-motion'
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { ConfirmStrip } from './ConfirmStrip'
import { useEscClose } from '../esc'
import { useSurfaceMotion } from '../motion'
import { t, transcriptRate, transcriptSummary, type UiLang } from '../i18n'
import { transcriptLineMarks, transcriptRows } from '../../../shared/transcript-lines'
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

  // ── 行号槽 ──────────────────────────────────────────────────────────
  //
  // 为什么要有它：textarea 软换行之后，一条 80 字的字幕在屏幕上占两行，
  // 人分不清它是「一条超长的字幕」还是「两条字幕」—— 而这恰恰是文稿匹配
  // 最要紧的事（一行 = 一条字幕）。号只画在**逻辑行的第一行**上，续行留白，
  // 一眼可辨；号就是字幕号，跟最终 SRT 的序号对得上。
  //
  // 怎么定位每一行：一份和 textarea 同字体、同宽度、同样换行规则的隐形镜像，
  // 每个物理行一个块；排完版量每个块的位置，那就是号该在的 y。textarea
  // 滚动时整列跟着挪。
  const editorRef = useRef<HTMLTextAreaElement>(null)
  const mirrorRef = useRef<HTMLDivElement>(null)
  const gutterRef = useRef<HTMLDivElement>(null)
  /** 每个有号的字幕行：第一行在内容区里的 y（相对内容区顶） */
  const [marks, setMarks] = useState<{ no: number; top: number }[]>([])
  /** 光标在哪一行（物理行下标）：给当前行的号加个高亮 */
  const [caretRow, setCaretRow] = useState<number | null>(null)

  const rows = useMemo(() => transcriptRows(draft), [draft])
  const lineMarks = useMemo(() => transcriptLineMarks(draft), [draft])
  /** 物理行下标 → 字幕号 */
  const markByRow = useMemo(
    () => new Map(lineMarks.map((m) => [m.row, m.no] as const)),
    [lineMarks]
  )
  /** 字幕号 → 物理行下标（给当前行高亮用） */
  const rowByNo = useMemo(
    () => new Map(lineMarks.map((m) => [m.no, m.row] as const)),
    [lineMarks]
  )

  /**
   * 量一遍号该在哪儿。
   *
   * 隐形镜像摆在 textarea 的**内容盒**上：同字体、同宽度 ⇒ 同样的换行点，
   * 镜像里每个块的位置就是 textarea 里它的位置。
   *
   * 用 `offsetTop`（布局坐标）而不是 `getBoundingClientRect()`（视觉坐标）：
   * 面板入场是带缩放动画的，视觉坐标会被那个 scale 缩掉一截 —— 拿它当偏移
   * 写进样式，号就会一行比一行偏（0.986 的缩放 ⇒ 6 行偏 2px）。布局坐标不
   * 受变换影响，量到什么就是什么。
   */
  const measure = useCallback(() => {
    const ta = editorRef.current
    const mirror = mirrorRef.current
    const gutter = gutterRef.current
    if (!ta || !mirror || !gutter) return
    const cs = getComputedStyle(ta)
    const padTop = parseFloat(cs.paddingTop) || 0
    const padLeft = parseFloat(cs.paddingLeft) || 0
    const padRight = parseFloat(cs.paddingRight) || 0
    const width = Math.max(0, ta.clientWidth - padLeft - padRight)
    // 镜像与行号槽摆在同一个原点上（内容盒的顶），两边的坐标才可比
    mirror.style.left = `${padLeft}px`
    mirror.style.top = `${padTop}px`
    mirror.style.width = `${width}px`
    gutter.style.top = `${padTop}px`
    const next: { no: number; top: number }[] = []
    mirror.querySelectorAll<HTMLElement>('[data-row]').forEach((el) => {
      if (el.dataset.empty === '1') return // 空行不是一条字幕
      // offsetParent 是镜像（它是 absolute），offsetTop 就是它相对镜像的顶
      next.push({ no: Number(el.dataset.no), top: el.offsetTop })
    })
    setMarks(next)
  }, [])

  // 正文变了要重量（上面的行一变，下面全挪）。合并到一帧里，别在每次按键
  // 都同步量一遍 DOM。
  useLayoutEffect(() => {
    const raf = requestAnimationFrame(measure)
    return () => cancelAnimationFrame(raf)
  }, [draft, measure])

  // 尺寸变了也要重量：换行点会变（窗口拉伸、面板改大小）。
  useEffect(() => {
    const ta = editorRef.current
    if (!ta || typeof ResizeObserver === 'undefined') return
    const ro = new ResizeObserver(() => measure())
    ro.observe(ta)
    return () => ro.disconnect()
  }, [measure])

  /** 滚动时行号整列跟着挪，不然号会停在原地。 */
  const onScroll = useCallback(() => {
    const ta = editorRef.current
    const gutter = gutterRef.current
    if (!ta || !gutter) return
    gutter.style.transform = `translateY(${-ta.scrollTop}px)`
  }, [])

  /**
   * 光标落在哪一行：给那一行的号加个高亮，让人知道自己在哪。
   *
   * 三个入口都挂上：`select` 管拖选，`keyup` 管键盘移动，`click` 管点击落点
   * ——只挂 `select` 的话，键盘移动光标在有些情况下不冒泡，高亮会不跟手。
   */
  const syncCaret = useCallback(() => {
    const ta = editorRef.current
    if (!ta) return
    const upto = ta.value.slice(0, ta.selectionStart ?? 0)
    setCaretRow(upto.split('\n').length - 1)
  }, [])

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
              <div className="transcript-file" title={info.name}>
                {info.name}
              </div>
              {info.path && (
                <div className="transcript-path" title={info.path}>
                  {info.path}
                </div>
              )}
            </div>
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
          </aside>

          <div className="transcript-editor-wrap">
            <div className="transcript-editor-area">
              {/* 行号槽：号只画在字幕行的第一行上，续行留白 —— 一眼分清
                  「一条超长的字幕」和「两条字幕」。号就是字幕号。 */}
              <div className="transcript-gutter" aria-hidden="true">
                <div className="transcript-gutter-inner" ref={gutterRef}>
                  {marks.map((m) => (
                    <span
                      key={m.no}
                      className={`transcript-gutter-no ${
                        rowByNo.get(m.no) === caretRow ? 'is-caret' : ''
                      }`}
                      style={{ top: `${m.top}px` }}
                    >
                      {m.no}
                    </span>
                  ))}
                </div>
              </div>
              {/* 隐形镜像：和 textarea 同字体同宽度，用来量每一行在哪儿。 */}
              <div className="transcript-mirror" ref={mirrorRef} aria-hidden="true">
                {rows.map((row, i) => {
                  const no = markByRow.get(i)
                  return (
                    <span
                      key={i}
                      className="transcript-mirror-row"
                      data-row={i}
                      data-no={no ?? ''}
                      data-empty={no === undefined ? '1' : '0'}
                    >
                      {row}
                    </span>
                  )
                })}
              </div>
              <textarea
                ref={editorRef}
                className="transcript-editor"
                value={draft}
                readOnly={!editable}
                spellCheck={false}
                onChange={(e) => setDraft(e.target.value)}
                onScroll={onScroll}
                onSelect={syncCaret}
                onKeyUp={syncCaret}
                onClick={syncCaret}
              />
            </div>
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

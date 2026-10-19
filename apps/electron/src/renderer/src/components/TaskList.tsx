import { AnimatePresence, motion } from 'framer-motion'
import { useState } from 'react'
import { createPortal } from 'react-dom'
import {
  LANG_OPTIONS,
  formatDuration,
  queueRank,
  realtimeFactorLabel,
  stageLabel,
  stageWithChunk,
  t,
  UiLang
} from '../i18n'
import { useRowMotion } from '../motion'
import { SelectMenu } from './Popover'
import { formatSize } from '../lib/format'
import { isTranscriptPath } from '../lib/pairing'
import { rowChipState, rowStartIcon, rowStartKind, type RowStartKind } from '../../../shared/task-row'
import type { HotwordGroup, TaskSnapshot } from '../../../shared/types'

/**
 * 行内图标按钮：32×32 等宽，所以「开始 / 打开输出 / 重新转录」怎么变，右侧都是齐的。
 * 图标来自仓库 assets/icons（与旧版同一份资产），用 CSS mask 上色 → 跟随主题。
 * 纯图标必须配 title + aria-label，否则就成了猜谜。
 */
function IconButton({
  icon,
  label,
  tone = 'ghost',
  disabled,
  onClick
}: {
  icon: 'play' | 'stop' | 'folder' | 'retry' | 'trash'
  label: string
  tone?: 'ghost' | 'primary' | 'danger'
  disabled?: boolean
  onClick: () => void
}) {
  return (
    <button
      type="button"
      className={`icon-btn ${tone}`}
      title={label}
      aria-label={label}
      disabled={disabled}
      onClick={onClick}
    >
      <span className={`icon-glyph i-${icon}`} aria-hidden />
    </button>
  )
}

/**
 * 悬停弹卡（阶段耗时明细）。
 *
 * **必须 portal 到 body**：列表容器是 `.task-list{overflow:auto}`，卡片内部的绝对定位弹卡
 * 会被它裁掉——阶段名那一列就是这么丢的（行内菜单当初修过同一类问题，这个漏了）。
 * 位置由鼠标进入时的 rect 现算，底部放不下就翻到上方。
 */
function HoverCard({ trigger, children }: { trigger: React.ReactNode; children: React.ReactNode }) {
  const [rect, setRect] = useState<DOMRect | null>(null)
  const flipUp = rect ? rect.bottom + 260 > window.innerHeight : false
  const style: React.CSSProperties | undefined = rect
    ? flipUp
      ? { position: 'fixed', left: rect.left, bottom: window.innerHeight - rect.top + 6 }
      : { position: 'fixed', left: rect.left, top: rect.bottom + 6 }
    : undefined
  return (
    <span
      className="timing-chip"
      onMouseEnter={(e) => setRect(e.currentTarget.getBoundingClientRect())}
      onMouseLeave={() => setRect(null)}
    >
      {trigger}
      {rect &&
        style &&
        createPortal(
          <div className="hover-card" style={style}>
            {children}
          </div>,
          document.body
        )}
    </span>
  )
}

function startKindLabel(s: ReturnType<typeof t>, kind: RowStartKind): string {
  switch (kind) {
    case 'blocked':
      return s.transcriptRequiresTimedOutput
    case 'align':
      return s.transcriptStart
    case 'realign':
      return s.transcriptRealign
    case 'retry-align':
      return s.transcriptRetryAlign
    case 'retry':
      return s.retry
    case 'retranscribe':
      return s.retranscribe
    case 'start':
      return s.start
  }
}

export function TaskList({
  lang,
  tasks,
  hotwordGroups,
  defaultLanguage,
  transcriptExts,
  timedOutputEnabled,
  onStart,
  onCancel,
  onRemove,
  onOpen,
  onSetLanguage,
  onSetVocalSeparation,
  onApplyToAll,
  onSetHotwordGroup,
  onPickTranscript,
  onOpenTranscript,
  onAttachTranscript,
  onCopyError
}: {
  lang: UiLang
  tasks: TaskSnapshot[]
  hotwordGroups: HotwordGroup[]
  /** 设置里的识别语言：行内没单独选过时用它 */
  defaultLanguage: string
  /** CLI 的文稿扩展名表；行内拖放只在命中它时才接管 */
  transcriptExts: string[]
  /** 文稿匹配需要 SRT 或 ASS；没有时禁用开始并说明原因 */
  timedOutputEnabled: boolean
  onStart: (id: string) => void
  onCancel: (id: string) => void
  onRemove: (id: string) => void
  onOpen: (path: string) => void
  onSetLanguage: (id: string, language: string) => void
  onSetVocalSeparation: (id: string, on: boolean) => void
  /** 把这一行的语言/词库/人声分离推给其它行 */
  onApplyToAll: (id: string) => void
  onSetHotwordGroup: (id: string, groupId: string | null) => void
  onPickTranscript: (id: string) => void
  onOpenTranscript: (id: string) => void
  onAttachTranscript: (id: string, path: string) => void
  onCopyError: (text: string) => void
}) {
  const s = t(lang)
  const rowMotion = useRowMotion()
  /**
   * 正被拖拽经过的任务行。
   *
   * dragover 阶段拿不到文件名（Chromium 里 `getAsFile()` 返回 null），
   * 所以这一行**不知道**拖的是文稿还是媒体 —— 高亮只说「这一行是落点」，
   * 到底挂文稿还是加媒体，松手时按扩展名定。文稿那一条写在文稿 chip 的
   * 提示里（`transcriptDropHint`），不靠这个高亮说。
   */
  const [dropTargetId, setDropTargetId] = useState<string | null>(null)

  const label = (st: string): string =>
    st === 'running'
      ? s.running
      : st === 'done'
        ? s.done
        : st === 'error'
          ? s.error
          : st === 'cancelled'
            ? s.cancelled
            : s.queued

  return (
    <div className="task-list">
      <AnimatePresence initial={false}>
        {tasks.map((task) => {
          // 运行中锁定行内操作（与旧版一致）
          const locked = task.status === 'running'
          const doneWithOutput = task.status === 'done' && !!task.outputPath
          const language = task.language || defaultLanguage
          const boundGroup = hotwordGroups.find((g) => g.id === task.hotwordGroupId)
          const size = formatSize(task.sizeBytes)
          const duration = task.durationSec != null ? formatDuration(task.durationSec) : undefined
          const meta = [size, duration].filter(Boolean).join('  ·  ')
          const timingMs =
            task.totalMs ?? (task.stageTimings?.reduce((a, b) => a + b.ms, 0) || undefined)
          // 实时倍速：音视频时长 ÷ 本次处理耗时。算不出来就不显示（绝不显示 ∞ / NaN）。
          const rtfx = realtimeFactorLabel(lang, task.durationSec, timingMs)
          // 阶段文案按 CLI 的稳定 key 出（label 只是兜底）。分段计数接在后面（转写 2/5）。
          const stageText = stageWithChunk(
            stageLabel(lang, task.stageKey, task.stage),
            task.stageChunk,
            task.stageChunkTotal
          )
          const duplicateName = tasks.some((x) => x.id !== task.id && x.name === task.name)
          const hasTranscript = !!task.transcript
          const needsRealign = !!task.transcript?.needsRealign
          // 只有**真的进了队列**（有排队位次）或正在跑才锁；刚添加的待处理行还允许改稿
          const transcriptLocked =
            task.status === 'running' || (task.status === 'queued' && task.queueRank != null)
          const chips = rowChipState(hasTranscript)
          const startKind = rowStartKind({
            hasTranscript,
            status: task.status,
            hasOutput: !!task.outputPath,
            needsRealign,
            timedOutputEnabled
          })
          const startLabel = startKindLabel(s, startKind)
          // 文案固定，宽度才不会跟着行数变。有没有文稿、要不要重打，靠颜色区分。
          const transcriptChipText = hasTranscript ? s.transcript : s.transcriptAdd
          // 还没挂文稿时，把「可以拖文稿到这一行」说清楚 —— 这个功能靠拖，
          // 不说出来没人知道。挂上之后提示改成状态（打开 / 要重打 / 锁定）。
          const transcriptChipTitle = transcriptLocked
            ? s.transcriptLocked
            : task.transcript?.error
              ? s.transcriptReadFailed
              : needsRealign
                ? s.transcriptNeedsRealign
                : hasTranscript
                  ? s.transcriptOpen
                  : `${s.transcriptAdd} — ${s.transcriptDropHint}`
          return (
            <motion.div
              className={`task-card status-${task.status} ${
                dropTargetId === task.id ? 'row-drop' : ''
              }`}
              key={task.id}
              onDragOver={(e) => {
                if (!transcriptExts.length) return
                e.preventDefault()
                setDropTargetId(task.id)
              }}
              onDragLeave={(e) => {
                if (!e.currentTarget.contains(e.relatedTarget as Node)) {
                  setDropTargetId((cur) => (cur === task.id ? null : cur))
                }
              }}
              onDrop={(e) => {
                const paths = Array.from(e.dataTransfer.files || [])
                  .map((f) => window.oneasr.getPathForFile?.(f) || '')
                  .filter((p): p is string => !!p)
                const transcript = paths.find((p) => isTranscriptPath(p, transcriptExts))
                if (!transcript) return // 媒体：让窗口级 drop 走正常添加
                e.preventDefault()
                e.stopPropagation()
                setDropTargetId(null)
                onAttachTranscript(task.id, transcript)
              }}
              {...rowMotion}
            >
              {/* 左：文件名一行，下面是 大小 · 时长 · 耗时（旧版就是这个形状） */}
              <div className="card-info">
                <div className="name">
                  {/* 媒体类型由 CLI 的 probe 判定（壳不按扩展名猜）；还没探到就先留空位 */}
                  {/* 图标位**永远占 15px**：后端报 other（如 .txt）或还没探到时只是不画图形，
                      但宽度必须留着 —— 空 <span> 在 flex 里会塌成 0 宽，整行文件名就左移 15px
                      （首行从 grid 换成 flex 时踩过这个坑，实测 .txt 行 nameLeft 43 vs 58） */}
                  <span
                    className={`media-icon ${task.mediaKind ?? 'unknown'}`}
                    aria-hidden
                  />
                  {/* 完整路径放 title：文件名放不下或者需要确认位置时，悬停就能看到 */}
                  <span className="task-name" title={`${task.name}\n${task.input}`}>
                    {task.name}
                  </span>
                </div>
                <div className="meta-line">
                  {meta && <span className="name-meta">{meta}</span>}
                  {task.stageTimings && task.stageTimings.length > 0 && (
                    <HoverCard
                      trigger={
                        <>
                          {s.timing}
                          {timingMs != null ? ` ${formatDuration(timingMs / 1000)}` : ''}
                        </>
                      }
                    >
                      {/* 头部：处理耗时 + 倍速读数 + 总耗时。
                          倍速 = 音视频时长 ÷ 本次处理耗时，跟右边的总耗时互为倒数
                          （master 的耗时弹卡就是这么写的，见 e56299b）。 */}
                      <span className="timing-head">
                        <i>{s.timingTitle}</i>
                        <span className="timing-head-right">
                          {rtfx && <span className="timing-rtfx">{rtfx}</span>}
                          {timingMs != null && <b>{formatDuration(timingMs / 1000)}</b>}
                        </span>
                      </span>
                      <span className="timing-sep" />
                      {task.stageTimings.map((t2, i) => (
                        <span className="timing-line" key={`${t2.key ?? t2.stage}-${i}`}>
                          <i>{stageLabel(lang, t2.key, t2.stage)}</i>
                          <b>{formatDuration(t2.ms / 1000)}</b>
                        </span>
                      ))}
                    </HoverCard>
                  )}
                  {/* 状态就在文件名下面这一行（跟「大小·时长」「耗时」排在一起）：
                      跑着的时候显示阶段名，跑完/失败/取消显示对应状态。
                      之前它占着右列一个 88px 的定宽槽，右边空一大块；
                      挪到左列之后右列只剩参数 chip 和动作按钮，四个 chip 整体右移
                      （用户 2026-10-19 指出：状态该在文件名下面，右边别再空一块）。 */}
                  {task.status === 'running' ? (
                    <span className="badge running" title={stageText || undefined}>
                      <span>{stageText || s.running}</span>
                    </span>
                  ) : task.status === 'queued' && task.queueRank != null ? (
                    <span className="badge queued" title={queueRank(lang, task.queueRank)}>
                      <span>{queueRank(lang, task.queueRank)}</span>
                    </span>
                  ) : (
                    <span className={`badge ${task.status}`} title={label(task.status)}>
                      <span>{task.status === 'queued' ? s.pending : label(task.status)}</span>
                    </span>
                  )}
                </div>
                {/* 路径默认不占行（文件名已经在上面）；同名不同目录时才显示 */}
                {duplicateName && <div className="meta">{task.input}</div>}
              </div>
              {/* 右：术语 chips + 状态 + 动作（与旧版一致：术语贴着右侧，动作在最右） */}
              <div className="controls">
                <div className="row-chips">
                  {chips.showLanguage && (
                    <SelectMenu
                      className="row-chip lang-chip"
                      value={language}
                      options={LANG_OPTIONS.map((o) => ({ id: o.id, label: o.label }))}
                      ariaLabel={s.recognitionLanguage}
                      disabled={locked || !chips.languageEnabled}
                      onChange={(id) => onSetLanguage(task.id, id)}
                      footer={(close) => (
                        <>
                          <div className="chip-menu-sep" />
                          <button
                            type="button"
                            className="chip-menu-item"
                            onClick={() => {
                              onApplyToAll(task.id)
                              close()
                            }}
                          >
                            {s.applyToAll}
                          </button>
                        </>
                      )}
                    />
                  )}

                  {chips.showHotwords && (
                    <SelectMenu
                      className={`row-chip group-chip ${chips.hotwordsEnabled && !boundGroup ? 'muted' : ''}`}
                      value={task.hotwordGroupId ?? ''}
                      options={[
                        { id: '', label: s.hotwordNone },
                        ...hotwordGroups.map((g) => ({ id: g.id, label: g.name }))
                      ]}
                      ariaLabel={boundGroup ? boundGroup.name : s.hotwordNone}
                      title={chips.hotwordsEnabled ? undefined : s.hotwordIgnoredOnAlign}
                      disabled={locked || !chips.hotwordsEnabled}
                      onChange={(id) => onSetHotwordGroup(task.id, id || null)}
                      footer={(close) => (
                        <>
                          <div className="chip-menu-sep" />
                          <button
                            type="button"
                            className="chip-menu-item"
                            onClick={() => {
                              onApplyToAll(task.id)
                              close()
                            }}
                          >
                            {s.applyToAll}
                          </button>
                        </>
                      )}
                    />
                  )}

                  <button
                    type="button"
                    // 类名别叫 toggle：全局 .toggle 是开关组件的样式（带 ::after 旋钮），会撞车
                    className={`row-chip sep-chip ${task.vocalSeparation ? 'is-on' : ''}`}
                    disabled={locked}
                    title={s.vocalSep}
                    // 开/关同时靠颜色和圆点形状：空心是关，实心是开。aria-pressed 再给读屏。
                    aria-pressed={!!task.vocalSeparation}
                    onClick={() => onSetVocalSeparation(task.id, !task.vocalSeparation)}
                  >
                    <span className="sep-mark" aria-hidden />
                    <span className="ellipsis">{s.vocalSep}</span>
                  </button>

                  {/* 文稿：一个入口两种状态。没挂时点它选文件；挂了点它开面板。
                      拖一份文稿到这一行也会走同一条 setTranscript 路径。 */}
                  <button
                    type="button"
                    className={`row-chip transcript-chip ${hasTranscript ? 'has' : ''} ${
                      task.transcript?.needsRealign ? 'needs-realign' : ''
                    } ${task.transcript?.error ? 'has-error' : ''}`}
                    disabled={transcriptLocked}
                    title={transcriptChipTitle}
                    onClick={() => {
                      if (task.transcript?.error) {
                        onPickTranscript(task.id)
                      } else if (hasTranscript) {
                        onOpenTranscript(task.id)
                      } else {
                        onPickTranscript(task.id)
                      }
                    }}
                  >
                    <span className="transcript-glyph" aria-hidden />
                    <span className="ellipsis">{transcriptChipText}</span>
                  </button>
                </div>

              {/* 动作区用图标按钮：等宽 → 各行控件严格对齐；文字说明交给 title / aria-label
                  （图标是仓库里那 5 个 SVG，与旧版共享，mask 上色跟着主题走） */}
              {/* 三槽位始终都在：开始（按这一行自己的流程跑）、打开输出、移除。
                  用不上就禁用，不把按钮拿掉。 */}
              <div className="actions">
                {task.status === 'running' || (task.status === 'queued' && task.queueRank != null) ? (
                  <IconButton
                    icon="stop"
                    label={task.status === 'running' ? s.cancel : s.cancelQueued}
                    tone="danger"
                    onClick={() => onCancel(task.id)}
                  />
                ) : (
                  <IconButton
                    // 「再来一次」这一族用圆圈箭头（重新转录/重试/重新打轴），
                    // 「从零开始」才用播放三角——图形自己把区别说清楚
                    icon={rowStartIcon(startKind)}
                    label={startLabel}
                    tone="primary"
                    disabled={startKind === 'blocked'}
                    onClick={() => onStart(task.id)}
                  />
                )}
                <IconButton
                  icon="folder"
                  // 没有输出就禁用，按钮还在。禁用时别写「打开输出」。
                  label={doneWithOutput ? s.openOutput : s.noOutputYet}
                  disabled={!doneWithOutput}
                  onClick={() => onOpen(task.outputPath!)}
                />
                <IconButton
                  icon="trash"
                  label={s.remove}
                  tone="danger"
                  disabled={locked}
                  onClick={() => onRemove(task.id)}
                />
              </div>
              </div>

              {/* 错误：单独占最下面一行，点一下即复制全文（不留「复制错误」按钮） */}
              {task.error && (
                <div
                  className="row-error"
                  title={task.errorDetail ?? task.error}
                  onClick={() => onCopyError(task.errorDetail ?? task.error ?? '')}
                >
                  {/* 失败在哪一步是**结构上确定**的（CLI 的阶段事件），照实标出来；
                      错误原文一个字不解释、不翻译——ffmpeg 报什么就是什么，
                      点一下复制的是完整 stderr，拿去搜就行 */}
                  {stageText && <span className="err-stage">{stageText}</span>}
                  <span className="err-text">{task.error}</span>
                  {/* 复制图标按钮（不是文字按钮）；整行本身也能点，所以这里停一下冒泡 */}
                  <button
                    type="button"
                    className="icon-btn danger err-copy"
                    title={s.copyError}
                    aria-label={s.copyError}
                    onClick={(e) => {
                      e.stopPropagation()
                      onCopyError(task.errorDetail ?? task.error ?? '')
                    }}
                  >
                    <span className="icon-glyph i-copy" aria-hidden />
                  </button>
                </div>
              )}

            </motion.div>
          )
        })}
      </AnimatePresence>
    </div>
  )
}

import { AnimatePresence, motion } from 'framer-motion'
import { useEffect, useState } from 'react'
import { createPortal } from 'react-dom'
import { LANG_OPTIONS, formatDuration, queueRank, stageLabel, t, UiLang } from '../i18n'
import { useEscClose } from '../esc'
import { rowMotion } from '../motion'
import { formatSize } from '../lib/format'
import type { HotwordGroup, TaskSnapshot } from '../../../shared/types'

/**
 * 行内 chip 的下拉菜单：**渲染到 body**（portal），不用行内绝对定位。
 * 理由：`.task-list` 是滚动容器，行内菜单会被它裁掉——列表最后几行点开就只剩半截。
 * 固定在触发按钮下方；下面放不下就翻到上方；滚动/缩放/点外部/Esc 都关掉。
 */
function ChipMenu({
  anchor,
  onClose,
  children
}: {
  anchor: DOMRect
  onClose: () => void
  children: React.ReactNode
}) {
  useEscClose(onClose)

  useEffect(() => {
    const onDown = (e: MouseEvent): void => {
      if ((e.target as HTMLElement).closest('.chip-menu')) return
      onClose()
    }
    const onMove = (): void => onClose()
    window.addEventListener('mousedown', onDown, true)
    window.addEventListener('scroll', onMove, true)
    window.addEventListener('resize', onMove)
    return () => {
      window.removeEventListener('mousedown', onDown, true)
      window.removeEventListener('scroll', onMove, true)
      window.removeEventListener('resize', onMove)
    }
  }, [onClose])

  const flipUp = anchor.bottom + 264 > window.innerHeight
  const style: React.CSSProperties = flipUp
    ? { position: 'fixed', left: anchor.left, bottom: window.innerHeight - anchor.top + 6 }
    : { position: 'fixed', left: anchor.left, top: anchor.bottom + 6 }

  return createPortal(
    <div className="chip-menu" style={style} onClick={(e) => e.stopPropagation()}>
      {children}
    </div>,
    document.body
  )
}

/**
 * 行内图标按钮：32×32 等宽，所以「开始 / 打开输出 / 重新转录」怎么变，右侧都是齐的。
 * 图标来自仓库 assets/icons（与 GPUI 版同一份资产），用 CSS mask 上色 → 跟随主题。
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

export function TaskList({
  lang,
  tasks,
  hotwordGroups,
  defaultLanguage,
  onStart,
  onCancel,
  onRemove,
  onOpen,
  onSetLanguage,
  onSetVocalSeparation,
  onApplyToAll,
  onSetHotwordGroup,
  onCopyError
}: {
  lang: UiLang
  tasks: TaskSnapshot[]
  hotwordGroups: HotwordGroup[]
  /** 设置里的识别语言：行内没单独选过时用它 */
  defaultLanguage: string
  onStart: (id: string) => void
  onCancel: (id: string) => void
  onRemove: (id: string) => void
  onOpen: (path: string) => void
  onSetLanguage: (id: string, language: string) => void
  onSetVocalSeparation: (id: string, on: boolean) => void
  /** 把这一行的语言/词库/人声分离推给其它行 */
  onApplyToAll: (id: string) => void
  onSetHotwordGroup: (id: string, groupId: string | null) => void
  onCopyError: (text: string) => void
}) {
  const s = t(lang)
  /** 同时只开一个菜单：记 key 与触发按钮的位置（菜单用 fixed 定位） */
  const [menu, setMenu] = useState<{ key: string; rect: DOMRect } | null>(null)

  const toggleMenu = (key: string, el: HTMLElement): void => {
    setMenu((cur) => (cur?.key === key ? null : { key, rect: el.getBoundingClientRect() }))
  }
  const closeMenu = (): void => setMenu(null)

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
          // 运行中锁定行内操作（与 GPUI 的 locks_row_actions 一致）
          const locked = task.status === 'running'
          const doneWithOutput = task.status === 'done' && !!task.outputPath
          const language = task.language || defaultLanguage
          const langLabel = LANG_OPTIONS.find((o) => o.id === language)?.label ?? language
          const boundGroup = hotwordGroups.find((g) => g.id === task.hotwordGroupId)
          const size = formatSize(task.sizeBytes)
          const duration = task.durationSec != null ? formatDuration(task.durationSec) : undefined
          const meta = [size, duration].filter(Boolean).join('  ·  ')
          const timingMs =
            task.totalMs ?? (task.stageTimings?.reduce((a, b) => a + b.ms, 0) || undefined)
          // 阶段文案按 CLI 的稳定 key 出（label 只是兜底），药丸与耗时明细共用
          const stageText = stageLabel(lang, task.stageKey, task.stage)
          const duplicateName = tasks.some((x) => x.id !== task.id && x.name === task.name)
          const langKey = `${task.id}:lang`
          const hwKey = `${task.id}:hw`

          return (
            <motion.div
              className={`task-card status-${task.status}`}
              key={task.id}
              {...rowMotion}
            >
              {/* 左：文件名一行，下面是 大小 · 时长 · 耗时（GPUI 版就是这个形状） */}
              <div className="card-info">
                <div className="name">
                  {/* 媒体类型由 CLI 的 probe 判定（壳不按扩展名猜）；还没探到就先留空位 */}
                  {task.mediaKind === 'audio' || task.mediaKind === 'video' ? (
                    <span className={`media-icon ${task.mediaKind}`} aria-hidden />
                  ) : (
                    // 后端报 other（如 .txt）或还没探到：留空位不画图标
                    <span aria-hidden />
                  )}
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
                      {task.stageTimings.map((t2, i) => (
                        <span className="timing-line" key={`${t2.key ?? t2.stage}-${i}`}>
                          <i>{stageLabel(lang, t2.key, t2.stage)}</i>
                          <b>{formatDuration(t2.ms / 1000)}</b>
                        </span>
                      ))}
                    </HoverCard>
                  )}
                </div>
                {/* 路径默认不占行（文件名已经在上面）；同名不同目录时才显示 */}
                {duplicateName && <div className="meta">{task.input}</div>}
              </div>
              {/* 右：术语 chips + 状态 + 动作（与 GPUI 一致：术语贴着右侧，动作在最右） */}
              <div className="controls">
                <div className="row-chips">
                  <button
                    type="button"
                    className={`row-chip ${menu?.key === langKey ? 'open' : ''}`}
                    disabled={locked}
                    title={s.recognitionLanguage}
                    onClick={(e) => toggleMenu(langKey, e.currentTarget)}
                  >
                    {langLabel}
                    <span className="caret">▾</span>
                  </button>

                  <button
                    type="button"
                    className={`row-chip ${menu?.key === hwKey ? 'open' : ''} ${
                      boundGroup ? '' : 'muted'
                    }`}
                    disabled={locked}
                    title={s.hotwords}
                    onClick={(e) => toggleMenu(hwKey, e.currentTarget)}
                  >
                    <span className="ellipsis">{boundGroup ? boundGroup.name : s.hotwordNone}</span>
                    <span className="caret">▾</span>
                  </button>

                  <button
                    type="button"
                    // 类名别叫 toggle：全局 .toggle 是开关组件的样式（带 ::after 旋钮），会撞车
                    className={`row-chip sep-chip ${task.vocalSeparation ? 'is-on' : ''}`}
                    disabled={locked}
                    title={s.vocalSep}
                    onClick={() => onSetVocalSeparation(task.id, !task.vocalSeparation)}
                  >
                    {task.vocalSeparation ? `✓ ${s.vocalSep}` : s.vocalSep}
                  </button>
                </div>

                {/* 状态就一个药丸：排队位次并进它（原来是「排队第 1 位」+「排队」两个胶囊） */}
                {task.status === 'running' ? (
                  <span className="badge running" title={stageText}>
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

              {/* 动作区用图标按钮：等宽 → 各行控件严格对齐；文字说明交给 title / aria-label
                  （图标是仓库里那 5 个 SVG，GPUI 版同一份资产，mask 上色跟着主题走） */}
              {/* 固定三槽位，**永远都是满的**（没有"为了对齐留空位"那种掉牙感）：
                    槽位1 生命周期控制（开始/取消/重试/重新转录）、槽位2 查看结果、
                    槽位3 移除。完成后高光从槽位1 转到槽位2（打开输出点亮、重跑退成灰），
                    所以"下一步该点哪个"一眼可见，而不是靠多出一个按钮来暗示。 */}
              <div className="actions">
                {task.status === 'running' || (task.status === 'queued' && task.queueRank != null) ? (
                  <IconButton
                    icon="stop"
                    label={task.status === 'running' ? s.cancel : s.cancelQueued}
                    tone="danger"
                    onClick={() => onCancel(task.id)}
                  />
                ) : doneWithOutput ? (
                  <IconButton icon="retry" label={s.retranscribe} onClick={() => onStart(task.id)} />
                ) : (
                  <IconButton
                    icon={task.status === 'error' ? 'retry' : 'play'}
                    label={task.status === 'error' ? s.retry : s.start}
                    tone="primary"
                    onClick={() => onStart(task.id)}
                  />
                )}
                <IconButton
                  icon="folder"
                  label={s.openOutput}
                  tone="primary"
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

              {menu?.key === langKey && (
                <ChipMenu anchor={menu.rect} onClose={closeMenu}>
                  {LANG_OPTIONS.map((o) => (
                    <button
                      type="button"
                      key={o.id}
                      className={`chip-menu-item ${o.id === language ? 'active' : ''}`}
                      onClick={() => {
                        onSetLanguage(task.id, o.id)
                        closeMenu()
                      }}
                    >
                      {o.label}
                    </button>
                  ))}
                  <div className="chip-menu-sep" />
                  <button
                    type="button"
                    className="chip-menu-item"
                    onClick={() => {
                      onApplyToAll(task.id)
                      closeMenu()
                    }}
                  >
                    {s.applyToAll}
                  </button>
                </ChipMenu>
              )}

              {menu?.key === hwKey && (
                <ChipMenu anchor={menu.rect} onClose={closeMenu}>
                  <button
                    type="button"
                    className={`chip-menu-item ${task.hotwordGroupId ? '' : 'active'}`}
                    onClick={() => {
                      onSetHotwordGroup(task.id, null)
                      closeMenu()
                    }}
                  >
                    {s.hotwordNone}
                  </button>
                  {hotwordGroups.map((g) => (
                    <button
                      type="button"
                      key={g.id}
                      className={`chip-menu-item ${g.id === task.hotwordGroupId ? 'active' : ''}`}
                      onClick={() => {
                        onSetHotwordGroup(task.id, g.id)
                        closeMenu()
                      }}
                    >
                      {g.name}
                    </button>
                  ))}
                  <div className="chip-menu-sep" />
                  <button
                    type="button"
                    className="chip-menu-item"
                    onClick={() => {
                      onApplyToAll(task.id)
                      closeMenu()
                    }}
                  >
                    {s.applyToAll}
                  </button>
                </ChipMenu>
              )}
            </motion.div>
          )
        })}
      </AnimatePresence>
    </div>
  )
}

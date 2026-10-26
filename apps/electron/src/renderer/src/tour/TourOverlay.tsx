import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { AnimatePresence, motion } from 'framer-motion'
import { EASE_OUT, useSurfaceMotion } from '../motion'
import { readTargetRect } from './wait'
import type { TourAdvance } from './types'
import { attachTourTargetClickListener } from './tourTargetClick'

const PAD = 8
/** 与 CSS --titlebar-h / main TITLEBAR_HEIGHT 一致。遮罩不得盖住标题栏：
 *  系统 Window Controls Overlay 画在网页之上，压暗标题栏会留下一块刺眼的白按钮区。 */
const TITLEBAR_H = 44

type Props = {
  active: boolean
  selector: string | null
  title: string
  body: string
  stepIndex: number
  stepTotal: number
  advance: TourAdvance
  nextLabel: string
  /** click-or-next 时「下一步」可改成「跳过此步」 */
  skipStepLabel?: string
  skipLabel: string
  doneLabel: string
  clickHint: string
  clickOrNextHint?: string
  skipWaitLabel?: string
  showSkipWait?: boolean
  isLast: boolean
  onNext: () => void
  onSkip: () => void
  onSkipWait?: () => void
  /** 目标被点了（capture+defer）；由引擎决定是否推进 */
  onTargetClick: () => void
}

export function TourOverlay({
  active,
  selector,
  title,
  body,
  stepIndex,
  stepTotal,
  advance,
  nextLabel,
  skipStepLabel,
  skipLabel,
  doneLabel,
  clickHint,
  clickOrNextHint,
  skipWaitLabel,
  showSkipWait,
  isLast,
  onNext,
  onSkip,
  onSkipWait,
  onTargetClick
}: Props) {
  const [rect, setRect] = useState<DOMRect | null>(null)
  const surface = useSurfaceMotion()

  // 跟随目标：resize / scroll / 布局变化
  useLayoutEffect(() => {
    if (!active || !selector) {
      setRect(null)
      return
    }
    const update = (): void => setRect(readTargetRect(selector))
    update()
    const ro = new ResizeObserver(update)
    const el = document.querySelector(selector)
    if (el) ro.observe(el)
    window.addEventListener('resize', update)
    window.addEventListener('scroll', update, true)
    const timer = window.setInterval(update, 200)
    return () => {
      ro.disconnect()
      window.removeEventListener('resize', update)
      window.removeEventListener('scroll', update, true)
      window.clearInterval(timer)
    }
  }, [active, selector])

  // 点高亮目标推进：capture + 延后回调。
  // SettingsDrawer modal-shell stopPropagation 会挡住 document 冒泡监听；
  // capture 才能收到标签点击。延后到 setTimeout(0) 让目标自己的 onClick
  // （如 setActiveTab）先跑完，再 advance。不 preventDefault。
  const onTargetClickRef = useRef(onTargetClick)
  onTargetClickRef.current = onTargetClick
  useEffect(() => {
    if (!active || !selector) return
    if (advance !== 'click' && advance !== 'click-or-next') return
    return attachTourTargetClickListener({
      selector,
      onTargetClick: () => onTargetClickRef.current()
    })
  }, [active, selector, advance])

  const hole = useMemo(() => {
    if (!rect) return null
    // 洞可以贴到标题栏下沿，但不要伸进系统按钮区（避免洞边缘画在 overlay 上）
    const top = Math.max(TITLEBAR_H, rect.top - PAD)
    const left = Math.max(0, rect.left - PAD)
    const bottom = rect.bottom + PAD
    const right = rect.right + PAD
    return {
      top,
      left,
      width: Math.max(0, right - left),
      height: Math.max(0, bottom - top)
    }
  }, [rect])

  // click：只点目标，不显示下一步；click-or-next / next：显示
  const showNext = advance === 'next' || advance === 'click-or-next' || isLast
  const showClickHint = (advance === 'click' || advance === 'click-or-next') && !isLast
  const hintText =
    advance === 'click-or-next' && clickOrNextHint ? clickOrNextHint : clickHint
  const primaryLabel = isLast
    ? doneLabel
    : advance === 'click-or-next' && skipStepLabel
      ? skipStepLabel
      : nextLabel

  return (
    <AnimatePresence>
      {active && (
        <motion.div
          className="tour-root"
          aria-live="polite"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0, transition: { duration: 0.12, ease: EASE_OUT } }}
          transition={{ duration: 0.18, ease: EASE_OUT }}
        >
          {/* 四块遮罩围出洞；一律从标题栏下方开始，避免压暗后与系统 WCO 白块割裂 */}
          {hole ? (
            <>
              <div
                className="tour-mask"
                style={{
                  top: TITLEBAR_H,
                  left: 0,
                  right: 0,
                  height: Math.max(0, hole.top - TITLEBAR_H)
                }}
              />
              <div
                className="tour-mask"
                style={{
                  top: hole.top,
                  left: 0,
                  width: hole.left,
                  height: hole.height
                }}
              />
              <div
                className="tour-mask"
                style={{
                  top: hole.top,
                  left: hole.left + hole.width,
                  right: 0,
                  height: hole.height
                }}
              />
              <div
                className="tour-mask"
                style={{
                  top: hole.top + hole.height,
                  left: 0,
                  right: 0,
                  bottom: 0
                }}
              />
              <motion.div
                className="tour-spotlight"
                initial={false}
                animate={{
                  top: hole.top,
                  left: hole.left,
                  width: hole.width,
                  height: hole.height
                }}
                transition={{ type: 'spring', stiffness: 380, damping: 32, mass: 0.7 }}
              />
            </>
          ) : (
            // 无目标（done）或尚未量到：遮罩仍避开标题栏
            <div
              className="tour-mask"
              style={{ top: TITLEBAR_H, left: 0, right: 0, bottom: 0 }}
            />
          )}

          {/* 讲解卡固定右下角，只有聚光环跟着目标走 */}
          <motion.div
            className="tour-bubble tour-bubble-dock"
            role="dialog"
            aria-modal="false"
            aria-label={title}
            key={`${title}-${stepIndex}`}
            {...surface}
          >
            <div className="tour-bubble-head">
              <strong>{title}</strong>
              <span className="tour-step-count">
                {stepIndex + 1}/{stepTotal}
              </span>
            </div>
            <p className="tour-bubble-body">{body}</p>
            {showClickHint && <p className="tour-click-hint">{hintText}</p>}
            <div className="tour-bubble-actions">
              {!isLast && (
                <button type="button" className="btn btn-ghost btn-sm" onClick={onSkip}>
                  {skipLabel}
                </button>
              )}
              <div className="grow" />
              {showSkipWait && skipWaitLabel && onSkipWait && (
                <button type="button" className="btn btn-ghost btn-sm" onClick={onSkipWait}>
                  {skipWaitLabel}
                </button>
              )}
              {showNext && (
                <button type="button" className="btn btn-primary btn-sm" onClick={onNext}>
                  {primaryLabel}
                </button>
              )}
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  )
}

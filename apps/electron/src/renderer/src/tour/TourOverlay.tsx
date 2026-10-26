import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { AnimatePresence, motion } from 'framer-motion'
import { EASE_OUT, useSurfaceMotion } from '../motion'
import { placeBubble } from './place'
import { readTargetRect } from './wait'
import type { TourAdvance, TourPlacement } from './types'

const PAD = 8
const DEFAULT_BUBBLE_W = 320
const DEFAULT_BUBBLE_H = 168

type Props = {
  active: boolean
  selector: string | null
  title: string
  body: string
  stepIndex: number
  stepTotal: number
  advance: TourAdvance
  placement?: TourPlacement
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
  /** 目标被点了（capture）；由引擎决定是否推进 */
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
  placement = 'auto',
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
  const [bubbleSize, setBubbleSize] = useState({ w: DEFAULT_BUBBLE_W, h: DEFAULT_BUBBLE_H })
  const bubbleRef = useRef<HTMLDivElement | null>(null)
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

  // 量真实气泡尺寸，避免估算偏小导致盖住目标
  useLayoutEffect(() => {
    if (!active) return
    const el = bubbleRef.current
    if (!el) return
    const measure = (): void => {
      const r = el.getBoundingClientRect()
      if (r.width > 0 && r.height > 0) {
        setBubbleSize({ w: Math.ceil(r.width), h: Math.ceil(r.height) })
      }
    }
    measure()
    const ro = new ResizeObserver(measure)
    ro.observe(el)
    return () => ro.disconnect()
  }, [active, title, body, stepIndex])

  // 点高亮目标推进（冒泡阶段：先让目标自己的 onClick 跑完，例如切 settings tab，
  // 再 advance；不 preventDefault → 不挡 ASR / 不取消任务）
  useEffect(() => {
    if (!active || !selector) return
    if (advance !== 'click' && advance !== 'click-or-next') return
    const onClick = (event: MouseEvent): void => {
      const target = event.target
      if (!(target instanceof Element)) return
      const hit = target.closest(selector)
      if (!hit) return
      onTargetClick()
    }
    document.addEventListener('click', onClick, false)
    return () => document.removeEventListener('click', onClick, false)
  }, [active, selector, advance, onTargetClick])

  const hole = useMemo(() => {
    if (!rect) return null
    return {
      top: Math.max(0, rect.top - PAD),
      left: Math.max(0, rect.left - PAD),
      width: rect.width + PAD * 2,
      height: rect.height + PAD * 2
    }
  }, [rect])

  const bubblePos = useMemo(() => {
    if (!rect) {
      return {
        top: Math.max(12, window.innerHeight / 2 - bubbleSize.h / 2),
        left: Math.max(12, window.innerWidth / 2 - bubbleSize.w / 2),
        side: 'bottom' as const
      }
    }
    return placeBubble(rect, placement, bubbleSize.w, bubbleSize.h)
  }, [rect, placement, bubbleSize.w, bubbleSize.h])

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
          {/* 四块遮罩围出洞：洞里 pointer-events:none，点得穿到真控件 */}
          {hole ? (
            <>
              <div
                className="tour-mask"
                style={{ top: 0, left: 0, right: 0, height: hole.top }}
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
            // 无目标（done）或尚未量到：全遮罩，不留假洞
            <div className="tour-mask tour-mask-full" />
          )}

          <motion.div
            ref={bubbleRef}
            className={`tour-bubble side-${bubblePos.side}`}
            role="dialog"
            aria-modal="false"
            aria-label={title}
            style={{ top: bubblePos.top, left: bubblePos.left }}
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

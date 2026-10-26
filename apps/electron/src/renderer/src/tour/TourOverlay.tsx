import { useEffect, useLayoutEffect, useMemo, useState } from 'react'
import { AnimatePresence, motion } from 'framer-motion'
import { EASE_OUT, useSurfaceMotion } from '../motion'
import { readTargetRect } from './wait'
import type { TourAdvance, TourPlacement } from './types'

const PAD = 8
const BUBBLE_GAP = 12

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
  skipLabel: string
  doneLabel: string
  clickHint: string
  skipWaitLabel?: string
  showSkipWait?: boolean
  isLast: boolean
  onNext: () => void
  onSkip: () => void
  onSkipWait?: () => void
  /** 目标被点了（capture）；由引擎决定是否推进 */
  onTargetClick: () => void
}

function placeBubble(
  rect: DOMRect,
  placement: TourPlacement,
  bubbleW: number,
  bubbleH: number
): { top: number; left: number; side: Exclude<TourPlacement, 'auto'> } {
  const vw = window.innerWidth
  const vh = window.innerHeight
  const candidates: Exclude<TourPlacement, 'auto'>[] =
    placement === 'auto'
      ? ['bottom', 'top', 'right', 'left']
      : [placement, 'bottom', 'top', 'right', 'left']

  for (const side of candidates) {
    let top = 0
    let left = 0
    if (side === 'bottom') {
      top = rect.bottom + BUBBLE_GAP
      left = rect.left + rect.width / 2 - bubbleW / 2
    } else if (side === 'top') {
      top = rect.top - bubbleH - BUBBLE_GAP
      left = rect.left + rect.width / 2 - bubbleW / 2
    } else if (side === 'right') {
      top = rect.top + rect.height / 2 - bubbleH / 2
      left = rect.right + BUBBLE_GAP
    } else {
      top = rect.top + rect.height / 2 - bubbleH / 2
      left = rect.left - bubbleW - BUBBLE_GAP
    }
    left = Math.max(12, Math.min(left, vw - bubbleW - 12))
    top = Math.max(12, Math.min(top, vh - bubbleH - 12))
    const fits =
      (side === 'bottom' && rect.bottom + BUBBLE_GAP + bubbleH < vh - 8) ||
      (side === 'top' && rect.top - BUBBLE_GAP - bubbleH > 8) ||
      (side === 'right' && rect.right + BUBBLE_GAP + bubbleW < vw - 8) ||
      (side === 'left' && rect.left - BUBBLE_GAP - bubbleW > 8)
    if (fits || placement !== 'auto') return { top, left, side }
  }
  return {
    top: Math.min(rect.bottom + BUBBLE_GAP, vh - 180),
    left: Math.max(12, Math.min(rect.left, vw - bubbleW - 12)),
    side: 'bottom'
  }
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
  skipLabel,
  doneLabel,
  clickHint,
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

  // 点高亮目标推进（capture，不阻止原事件 → 不挡 ASR / 不取消任务）
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
    document.addEventListener('click', onClick, true)
    return () => document.removeEventListener('click', onClick, true)
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
    if (!rect) return { top: window.innerHeight / 2 - 80, left: window.innerWidth / 2 - 160, side: 'bottom' as const }
    return placeBubble(rect, placement, 320, 160)
  }, [rect, placement])

  const showNext = advance === 'next' || advance === 'click-or-next' || isLast
  const showClickHint = advance === 'click' || advance === 'click-or-next'

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
            <div className="tour-mask tour-mask-full" />
          )}

          <motion.div
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
            {showClickHint && !isLast && (
              <p className="tour-click-hint">{clickHint}</p>
            )}
            <div className="tour-bubble-actions">
              <button type="button" className="btn btn-ghost btn-sm" onClick={onSkip}>
                {skipLabel}
              </button>
              <div className="grow" />
              {showSkipWait && skipWaitLabel && onSkipWait && (
                <button type="button" className="btn btn-ghost btn-sm" onClick={onSkipWait}>
                  {skipWaitLabel}
                </button>
              )}
              {showNext && (
                <button type="button" className="btn btn-primary btn-sm" onClick={onNext}>
                  {isLast ? doneLabel : nextLabel}
                </button>
              )}
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  )
}

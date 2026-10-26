import type { TourPlacement } from './types'

const BUBBLE_GAP = 18

export type PlaceSide = Exclude<TourPlacement, 'auto'>

function viewport(): { vw: number; vh: number } {
  // 浏览器里 globalThis === window；单测可 stub globalThis.window
  const g = globalThis as typeof globalThis & {
    window?: { innerWidth: number; innerHeight: number }
    innerWidth?: number
    innerHeight?: number
  }
  const w = g.window ?? g
  return { vw: w.innerWidth ?? 1280, vh: w.innerHeight ?? 800 }
}

function rectsOverlap(
  a: { top: number; left: number; width: number; height: number },
  b: { top: number; left: number; right: number; bottom: number },
  pad = 4
): boolean {
  const ar = a.left + a.width
  const ab = a.top + a.height
  return !(
    ar + pad < b.left ||
    a.left - pad > b.right ||
    ab + pad < b.top ||
    a.top - pad > b.bottom
  )
}

function computeSide(
  rect: DOMRect,
  side: PlaceSide,
  bubbleW: number,
  bubbleH: number
): { top: number; left: number } {
  if (side === 'bottom') {
    return {
      top: rect.bottom + BUBBLE_GAP,
      left: rect.left + rect.width / 2 - bubbleW / 2
    }
  }
  if (side === 'top') {
    return {
      top: rect.top - bubbleH - BUBBLE_GAP,
      left: rect.left + rect.width / 2 - bubbleW / 2
    }
  }
  if (side === 'right') {
    return {
      top: rect.top + rect.height / 2 - bubbleH / 2,
      left: rect.right + BUBBLE_GAP
    }
  }
  return {
    top: rect.top + rect.height / 2 - bubbleH / 2,
    left: rect.left - bubbleW - BUBBLE_GAP
  }
}

function clampPos(
  top: number,
  left: number,
  bubbleW: number,
  bubbleH: number
): { top: number; left: number } {
  const { vw, vh } = viewport()
  return {
    left: Math.max(20, Math.min(left, vw - bubbleW - 20)),
    top: Math.max(20, Math.min(top, vh - bubbleH - 20))
  }
}

/** 候选顺序：首选 → 视口对侧优先（目标在下半就先试上）→ 其余 */
function candidateSides(placement: TourPlacement, rect: DOMRect): PlaceSide[] {
  const { vw, vh } = viewport()
  const preferTop = rect.top + rect.height / 2 > vh / 2
  const preferLeft = rect.left + rect.width / 2 > vw / 2
  const oppositeFirst: PlaceSide[] = preferTop
    ? preferLeft
      ? ['top', 'left', 'bottom', 'right']
      : ['top', 'right', 'bottom', 'left']
    : preferLeft
      ? ['bottom', 'left', 'top', 'right']
      : ['bottom', 'right', 'top', 'left']

  if (placement === 'auto') return oppositeFirst
  return [placement, ...oppositeFirst.filter((s) => s !== placement)]
}

/**
 * 气泡永远不要紧贴/盖住目标矩形；放不下就翻到视口对侧。
 */
export function placeBubble(
  rect: DOMRect,
  placement: TourPlacement,
  bubbleW: number,
  bubbleH: number
): { top: number; left: number; side: PlaceSide } {
  const { vw, vh } = viewport()
  const target = {
    top: rect.top,
    left: rect.left,
    right: rect.right,
    bottom: rect.bottom
  }
  const sides = candidateSides(placement, rect)
  let best: { top: number; left: number; side: PlaceSide; score: number } | null = null

  for (const side of sides) {
    const raw = computeSide(rect, side, bubbleW, bubbleH)
    const pos = clampPos(raw.top, raw.left, bubbleW, bubbleH)
    const box = { top: pos.top, left: pos.left, width: bubbleW, height: bubbleH }
    const overlaps = rectsOverlap(box, target)
    const naturalFit =
      (side === 'bottom' && raw.top + bubbleH <= vh - 8) ||
      (side === 'top' && raw.top >= 8) ||
      (side === 'right' && raw.left + bubbleW <= vw - 8) ||
      (side === 'left' && raw.left >= 8)
    const score =
      (overlaps ? -1000 : 0) +
      (naturalFit ? 100 : 0) +
      (side === sides[0] ? 10 : 0) -
      Math.abs(pos.top - raw.top) -
      Math.abs(pos.left - raw.left)
    if (!best || score > best.score) {
      best = { ...pos, side, score }
    }
    if (!overlaps && naturalFit) {
      return { top: pos.top, left: pos.left, side }
    }
  }

  if (best && best.score > -500) {
    return { top: best.top, left: best.left, side: best.side }
  }

  const cx = rect.left + rect.width / 2
  const cy = rect.top + rect.height / 2
  const corners = [
    { top: 20, left: 20, side: 'bottom' as PlaceSide },
    { top: 20, left: vw - bubbleW - 20, side: 'bottom' as PlaceSide },
    { top: vh - bubbleH - 20, left: 20, side: 'top' as PlaceSide },
    { top: vh - bubbleH - 20, left: vw - bubbleW - 20, side: 'top' as PlaceSide }
  ]
  corners.sort((a, b) => {
    const da = Math.hypot(a.left + bubbleW / 2 - cx, a.top + bubbleH / 2 - cy)
    const db = Math.hypot(b.left + bubbleW / 2 - cx, b.top + bubbleH / 2 - cy)
    return db - da
  })
  const pick = corners[0]!
  return { top: pick.top, left: pick.left, side: pick.side }
}

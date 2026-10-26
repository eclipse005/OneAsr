/** 轮询等选择器出现；超时返回 null。不抛错。 */
export function waitForSelector(
  selector: string,
  timeoutMs = 8000,
  intervalMs = 50
): Promise<Element | null> {
  const existing = document.querySelector(selector)
  if (existing) return Promise.resolve(existing)

  return new Promise((resolve) => {
    const started = Date.now()
    const tick = (): void => {
      const el = document.querySelector(selector)
      if (el) {
        resolve(el)
        return
      }
      if (Date.now() - started >= timeoutMs) {
        resolve(null)
        return
      }
      window.setTimeout(tick, intervalMs)
    }
    tick()
  })
}

/** 读目标矩形；目标消失时返回 null */
export function readTargetRect(selector: string): DOMRect | null {
  const el = document.querySelector(selector)
  if (!el || !(el instanceof HTMLElement)) return null
  const rect = el.getBoundingClientRect()
  if (rect.width <= 0 && rect.height <= 0) return null
  return rect
}

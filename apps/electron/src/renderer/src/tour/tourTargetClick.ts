/**
 * Tour click-to-advance helper.
 *
 * SettingsDrawer’s modal-shell calls stopPropagation so overlay clicks don’t
 * close the drawer. A document bubble listener therefore never sees tab
 * clicks. Capture phase is required; we defer the advance callback so the
 * target’s own onClick (e.g. setActiveTab) still runs in the same native event.
 */

type Closestable = { closest: (selector: string) => unknown }

function asClosestable(target: EventTarget | null): Closestable | null {
  if (!target || typeof (target as unknown as Closestable).closest !== 'function') {
    return null
  }
  return target as unknown as Closestable
}

export function matchesTourTarget(
  target: EventTarget | null,
  selector: string
): boolean {
  const el = asClosestable(target)
  return !!el?.closest(selector)
}

export type AttachTourTargetClickOptions = {
  selector: string
  onTargetClick: () => void
  /** Defaults to document. Injectable for tests. */
  root?: {
    addEventListener: (
      type: string,
      listener: EventListenerOrEventListenerObject,
      options?: boolean | AddEventListenerOptions
    ) => void
    removeEventListener: (
      type: string,
      listener: EventListenerOrEventListenerObject,
      options?: boolean | EventListenerOptions
    ) => void
  }
  /** Defaults to setTimeout(0). Injectable for sync tests. Returns cancel. */
  defer?: (cb: () => void) => () => void
}

/**
 * Listen for clicks on `selector` in capture phase and invoke onTargetClick
 * after deferring (so React/onClick on the target runs first).
 * Returns an unsubscribe that also cancels a pending deferred call.
 */
export function attachTourTargetClickListener(
  options: AttachTourTargetClickOptions
): () => void {
  const {
    selector,
    onTargetClick,
    root = document,
    defer = (cb) => {
      const id = window.setTimeout(cb, 0)
      return () => window.clearTimeout(id)
    }
  } = options

  let cancelPending: (() => void) | null = null

  const onClick = (event: Event): void => {
    if (!matchesTourTarget(event.target, selector)) return
    cancelPending?.()
    cancelPending = defer(() => {
      cancelPending = null
      onTargetClick()
    })
  }

  // capture: true — see file header
  root.addEventListener('click', onClick, true)
  return () => {
    root.removeEventListener('click', onClick, true)
    cancelPending?.()
    cancelPending = null
  }
}

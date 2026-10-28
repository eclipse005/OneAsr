import type { ReactNode, RefObject } from 'react'
import { useEffect, useRef } from 'react'
import { motion } from 'framer-motion'
import { useEscClose } from '../../esc'
import { useOverlayMotion, useSurfaceMotion } from '../../motion'

const FOCUSABLE_SELECTOR = [
  'a[href]',
  'button:not([disabled])',
  'textarea:not([disabled])',
  'input:not([disabled]):not([type="hidden"])',
  'select:not([disabled])',
  '[tabindex]:not([tabindex="-1"])'
].join(',')

function focusableIn(root: HTMLElement): HTMLElement[] {
  return Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)).filter((el) => {
    if (el.closest('[aria-hidden="true"]')) return false
    return el.getClientRects().length > 0
  })
}

/**
 * Trap Tab/Shift+Tab inside `containerRef`; focus first focusable on mount;
 * restore previously focused element on unmount. Esc stays on useEscClose.
 */
function useFocusTrap(containerRef: RefObject<HTMLElement | null>): void {
  useEffect(() => {
    const container = containerRef.current
    if (!container) return

    const previouslyFocused =
      document.activeElement instanceof HTMLElement ? document.activeElement : null

    const focusInitial = (): void => {
      const nodes = focusableIn(container)
      ;(nodes[0] ?? container).focus({ preventScroll: true })
    }

    // Double rAF: wait for Framer Motion mount + layout before focusing.
    let raf2 = 0
    const raf1 = requestAnimationFrame(() => {
      raf2 = requestAnimationFrame(focusInitial)
    })

    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key !== 'Tab') return
      const nodes = focusableIn(container)
      if (nodes.length === 0) {
        event.preventDefault()
        container.focus({ preventScroll: true })
        return
      }
      const first = nodes[0]!
      const last = nodes[nodes.length - 1]!
      const activeEl = document.activeElement
      if (event.shiftKey) {
        if (activeEl === first || !container.contains(activeEl)) {
          event.preventDefault()
          last.focus()
        }
      } else if (activeEl === last || !container.contains(activeEl)) {
        event.preventDefault()
        first.focus()
      }
    }

    // Capture so Tab cycling wins over nested controls; tour dig-hole is
    // pointer-based and unaffected (tour overlay is not ModalShell).
    document.addEventListener('keydown', onKeyDown, true)

    return () => {
      cancelAnimationFrame(raf1)
      cancelAnimationFrame(raf2)
      document.removeEventListener('keydown', onKeyDown, true)
      if (previouslyFocused && document.contains(previouslyFocused)) {
        previouslyFocused.focus({ preventScroll: true })
      }
    }
  }, [containerRef])
}

/**
 * Shared modal chrome: overlay + shell + header + close.
 * Root is motion so AnimatePresence can run overlay exit; shell uses surface.
 * Focus trap + Esc + overlay dismiss. Tour coach-mark dig-hole is separate.
 */
export function ModalShell({
  title,
  onClose,
  closeLabel,
  children,
  shellClassName,
  headerDataTour,
  closeDataShot,
  closeDataTour
}: {
  title: string
  onClose: () => void
  closeLabel: string
  children: ReactNode
  /** Extra classes on `.modal-shell` (e.g. `guide-shell`, `transcript-shell`). */
  shellClassName?: string
  headerDataTour?: string
  closeDataShot?: string
  closeDataTour?: string
}) {
  const overlay = useOverlayMotion()
  const surface = useSurfaceMotion()
  const shellRef = useRef<HTMLDivElement>(null)
  useEscClose(onClose)
  useFocusTrap(shellRef)

  return (
    <motion.div className="overlay" onClick={onClose} {...overlay}>
      <motion.div
        ref={shellRef}
        className={shellClassName ? `modal-shell ${shellClassName}` : 'modal-shell'}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        tabIndex={-1}
        onClick={(e) => e.stopPropagation()}
        {...surface}
      >
        <div className="modal-header" data-tour={headerDataTour}>
          <h3>{title}</h3>
          <div style={{ flex: 1 }} />
          <button
            type="button"
            className="btn btn-ghost"
            data-shot={closeDataShot}
            data-tour={closeDataTour}
            onClick={onClose}
          >
            {closeLabel}
          </button>
        </div>
        {children}
      </motion.div>
    </motion.div>
  )
}

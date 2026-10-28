import type { ReactNode } from 'react'
import { motion } from 'framer-motion'
import { useEscClose } from '../../esc'
import { useSurfaceMotion } from '../../motion'

/**
 * Shared modal chrome: overlay + shell + header + close.
 * Focus trap intentionally stubbed (follow-up); Esc + overlay dismiss wired here.
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
  const surface = useSurfaceMotion()
  useEscClose(onClose)
  // TODO(a11y): focus trap inside dialog — stub for workstream 1

  return (
    <div className="overlay" onClick={onClose}>
      <motion.div
        className={shellClassName ? `modal-shell ${shellClassName}` : 'modal-shell'}
        role="dialog"
        aria-modal="true"
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
    </div>
  )
}

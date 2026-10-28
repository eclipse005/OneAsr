import { motion } from 'framer-motion'
import { useEscClose } from '../esc'
import { useOverlayMotion, useSurfaceMotion } from '../motion'

export function WelcomeModal({
  title,
  body,
  startLabel,
  skipLabel,
  onStart,
  onSkip
}: {
  title: string
  body: string
  startLabel: string
  skipLabel: string
  onStart: () => void
  onSkip: () => void
}) {
  const overlay = useOverlayMotion()
  const surface = useSurfaceMotion()
  useEscClose(onSkip)

  return (
    <motion.div className="tour-welcome-backdrop" onClick={onSkip} {...overlay}>
      <motion.div
        className="tour-welcome"
        role="dialog"
        aria-modal="true"
        aria-label={title}
        onClick={(e) => e.stopPropagation()}
        {...surface}
      >
        <div className="tour-welcome-mark" aria-hidden>
          <span className="tour-welcome-mark-icon" />
        </div>
        <h2>{title}</h2>
        <p>{body}</p>
        <div className="tour-welcome-actions">
          <button type="button" className="btn btn-ghost" onClick={onSkip}>
            {skipLabel}
          </button>
          <button type="button" className="btn btn-primary" onClick={onStart} data-tour="welcome-start">
            {startLabel}
          </button>
        </div>
      </motion.div>
    </motion.div>
  )
}

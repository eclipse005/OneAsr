import { motion } from 'framer-motion'
import { t, UiLang } from '../i18n'

export function GuidePanel({ lang, onClose }: { lang: UiLang; onClose: () => void }) {
  const s = t(lang)
  return (
    <div className="overlay" onClick={onClose}>
      <motion.div
        className="modal-shell"
        role="dialog"
        aria-modal="true"
        onClick={(e) => e.stopPropagation()}
        initial={{ opacity: 0, y: 18, scale: 0.96 }}
        animate={{ opacity: 1, y: 0, scale: 1 }}
        exit={{ opacity: 0, y: 10, scale: 0.97 }}
        transition={{ type: 'spring', stiffness: 380, damping: 32 }}
      >
        <div className="modal-header">
          <h3>{s.guideTitle}</h3>
          <div style={{ flex: 1 }} />
          <button type="button" className="btn btn-ghost" onClick={onClose}>
            {s.close}
          </button>
        </div>
        <div className="modal-body">
          <div className="section" style={{ whiteSpace: 'pre-wrap', color: 'var(--muted)', lineHeight: 1.65 }}>
            {s.guideBody}
          </div>
        </div>
      </motion.div>
    </div>
  )
}

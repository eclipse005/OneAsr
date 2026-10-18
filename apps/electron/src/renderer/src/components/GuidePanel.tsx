import { motion } from 'framer-motion'
import { t, UiLang } from '../i18n'

export function GuidePanel({ lang, onClose }: { lang: UiLang; onClose: () => void }) {
  const s = t(lang)
  return (
    <div className="overlay center" onClick={onClose}>
      <motion.div
        className="panel"
        onClick={(e) => e.stopPropagation()}
        initial={{ opacity: 0, y: 12 }}
        animate={{ opacity: 1, y: 0 }}
        exit={{ opacity: 0, y: 8 }}
      >
        <div className="modal-header">
          <h3>{s.guideTitle}</h3>
          <div style={{ flex: 1 }} />
          <button className="btn btn-ghost" onClick={onClose}>
            {s.close}
          </button>
        </div>
        <div className="body">{s.guideBody}</div>
      </motion.div>
    </div>
  )
}

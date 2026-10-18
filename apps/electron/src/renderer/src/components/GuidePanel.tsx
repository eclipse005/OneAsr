import { motion } from 'framer-motion'
import { t, UiLang } from '../i18n'

export function GuidePanel({ lang, onClose }: { lang: UiLang; onClose: () => void }) {
  const s = t(lang)
  // guideBody 是「1. …」逐行的步骤文本：拆成一条条卡片，在统一的 900×640 框里居中排布
  const steps = s.guideBody
    .split('\n')
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => line.replace(/^\d+[.、]\s*/, ''))

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
          <button type="button" data-shot="close-modal" className="btn btn-ghost" onClick={onClose}>
            {s.close}
          </button>
        </div>
        <div className="modal-body guide-body">
          {steps.map((step, i) => (
            <div className="guide-step" key={i}>
              <span className="guide-index">{i + 1}</span>
              <p>{step}</p>
            </div>
          ))}
        </div>
      </motion.div>
    </div>
  )
}

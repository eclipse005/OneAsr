import { motion } from 'framer-motion'
import { t, UiLang } from '../i18n'
import { useEscClose } from '../esc'
import { panelMotion } from '../motion'

export function GuidePanel({ lang, onClose }: { lang: UiLang; onClose: () => void }) {
  const s = t(lang)
  // guideBody 是「1. …」逐行的步骤文本：拆成一条条卡片，在统一的 900×640 框里居中排布
  const steps = s.guideBody
    .split('\n')
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => line.replace(/^\d+[.、]\s*/, ''))

  useEscClose(onClose)

  return (
    <div className="overlay" onClick={onClose}>
      <motion.div
        className="modal-shell"
        role="dialog"
        aria-modal="true"
        onClick={(e) => e.stopPropagation()}
        {...panelMotion}
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

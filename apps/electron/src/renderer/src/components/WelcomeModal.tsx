import { motion } from 'framer-motion'
import { t, UiLang } from '../i18n'
import { panelMotion } from '../motion'

export function WelcomeModal({
  lang,
  onGotIt,
  onOpenSettings
}: {
  lang: UiLang
  onGotIt: () => void
  onOpenSettings: () => void
}) {
  const s = t(lang)
  return (
    <div className="overlay">
      <motion.div
        className="modal"
        {...panelMotion}
      >
        <div className="modal-header">
          <div
            className="logo"
            style={{
              width: 22,
              height: 22,
              borderRadius: 8,
              background: 'linear-gradient(135deg,#14b8a6,#0d9488)'
            }}
          />
          <h3>{s.welcomeTitle}</h3>
        </div>
        <div className="body">{s.welcomeBody}</div>
        <div className="actions">
          <button className="btn btn-ghost" onClick={onOpenSettings}>
            {s.openSettings}
          </button>
          <button className="btn btn-primary" onClick={onGotIt}>
            {s.gotIt}
          </button>
        </div>
      </motion.div>
    </div>
  )
}

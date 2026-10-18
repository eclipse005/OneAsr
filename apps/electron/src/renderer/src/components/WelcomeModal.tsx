import { motion } from 'framer-motion'
import { t, UiLang } from '../i18n'

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
    <div className="overlay center">
      <motion.div
        className="modal"
        initial={{ opacity: 0, y: 16, scale: 0.98 }}
        animate={{ opacity: 1, y: 0, scale: 1 }}
        exit={{ opacity: 0, y: 8, scale: 0.98 }}
        transition={{ duration: 0.22 }}
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

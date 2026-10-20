import { motion } from 'framer-motion'
import { useLayerMotion } from '../motion'

/**
 * 浮窗里的确认卡：放弃未保存、删除词组。
 * 遮罩只盖住当前浮窗（.modal-shell 是定位上下文），不再从底部挤出一条独立横栏。
 */
export function ConfirmDialog({
  message,
  cancelLabel,
  confirmLabel,
  onCancel,
  onConfirm,
  danger = false
}: {
  message: string
  cancelLabel: string
  confirmLabel: string
  onCancel: () => void
  onConfirm: () => void
  danger?: boolean
}) {
  const layer = useLayerMotion()
  return (
    <div className="modal-confirm-backdrop" onClick={onCancel}>
      <motion.div
        className={`modal-confirm ${danger ? 'is-danger' : ''}`}
        role="alertdialog"
        aria-modal="true"
        onClick={(e) => e.stopPropagation()}
        {...layer}
      >
        <p>{message}</p>
        <div className="modal-confirm-actions">
          <button type="button" className="btn btn-ghost" onClick={onCancel}>
            {cancelLabel}
          </button>
          <button
            type="button"
            className={`btn ${danger ? 'btn-danger' : 'btn-primary'}`}
            onClick={onConfirm}
          >
            {confirmLabel}
          </button>
        </div>
      </motion.div>
    </div>
  )
}

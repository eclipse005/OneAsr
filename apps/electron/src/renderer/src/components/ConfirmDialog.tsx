import { motion } from 'framer-motion'
import { useLayerMotion, useOverlayMotion } from '../motion'

/**
 * 浮窗里的确认卡：放弃未保存、删除词组。
 * 遮罩只盖住当前浮窗（.modal-shell 是定位上下文），不再从底部挤出一条独立横栏。
 * 根节点是 motion，App 的 AnimatePresence 才能收到退出。
 */
export function ConfirmDialog({
  message,
  cancelLabel,
  confirmLabel,
  onCancel,
  onConfirm,
  danger = false,
  fixed = false
}: {
  message: string
  cancelLabel: string
  confirmLabel: string
  onCancel: () => void
  onConfirm: () => void
  danger?: boolean
  /** 主窗口没有 .modal-shell 作定位上下文时用固定遮罩（标题栏下方）。 */
  fixed?: boolean
}) {
  const overlay = useOverlayMotion()
  const layer = useLayerMotion()
  return (
    <motion.div
      className={`modal-confirm-backdrop${fixed ? ' is-fixed' : ''}`}
      onClick={onCancel}
      {...overlay}
    >
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
    </motion.div>
  )
}

import { motion } from 'framer-motion'
import { useLayerMotion } from '../motion'

/**
 * 浮窗里的确认条：放弃未保存、删除词组。清空列表走工具栏上的小卡片。
 * 淡入并上移 4px，退出沿原方向收回。父级要包一层 AnimatePresence，退出才走得完。
 */
export function ConfirmStrip({
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
    <motion.div className="confirm-clip" {...layer}>
      <div className={`confirm-strip ${danger ? 'is-danger' : ''}`}>
        <span>{message}</span>
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
  )
}

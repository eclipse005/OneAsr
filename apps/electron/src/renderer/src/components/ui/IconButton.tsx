/**
 * 行内图标按钮：32×32 等宽，所以「开始 / 打开输出 / 重新转录」怎么变，右侧都是齐的。
 * 图标来自仓库 assets/icons（与旧版同一份资产），用 CSS mask 上色 → 跟随主题。
 * 纯图标必须配 title + aria-label，否则就成了猜谜。
 */
export function IconButton({
  icon,
  label,
  tone = 'ghost',
  disabled,
  onClick,
  dataTour
}: {
  icon: 'play' | 'stop' | 'folder' | 'retry' | 'trash'
  label: string
  tone?: 'ghost' | 'primary' | 'danger'
  disabled?: boolean
  onClick: () => void
  dataTour?: string
}) {
  return (
    <button
      type="button"
      className={`icon-btn ${tone}`}
      title={label}
      aria-label={label}
      disabled={disabled}
      data-tour={dataTour}
      onClick={onClick}
    >
      <span className={`icon-glyph i-${icon}`} aria-hidden />
    </button>
  )
}

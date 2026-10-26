import { useRef, useState } from 'react'
import { Popover } from '../components/Popover'
import { t, type UiLang } from '../i18n'

/**
 * 工具栏「指南」：拆成「交互引导」和「使用说明」。
 * 交互引导走 TourProvider；使用说明仍开原来的 GuidePanel。
 */
export function GuideMenu({
  lang,
  onStartTour,
  onOpenManual,
  tourActive
}: {
  lang: UiLang
  onStartTour: () => void
  onOpenManual: () => void
  tourActive?: boolean
}) {
  const s = t(lang)
  const [anchor, setAnchor] = useState<DOMRect | null>(null)
  const triggerRef = useRef<HTMLButtonElement>(null)
  const close = (): void => setAnchor(null)

  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        className={`btn btn-ghost ${anchor ? 'open' : ''}`}
        data-shot="guide"
        data-tour="toolbar-guide"
        aria-haspopup="menu"
        aria-expanded={anchor != null}
        disabled={tourActive}
        onClick={(e) =>
          setAnchor((cur) => (cur ? null : e.currentTarget.getBoundingClientRect()))
        }
      >
        <span className="btn-glyph i-help" aria-hidden />
        {s.guide}
      </button>
      <Popover
        open={anchor != null}
        anchor={anchor}
        onClose={close}
        label={s.guide}
        trigger={triggerRef}
        className="chip-menu guide-menu"
      >
        <button
          type="button"
          className="chip-menu-item"
          role="menuitem"
          onClick={() => {
            close()
            onStartTour()
          }}
        >
          {s.tourInteractive}
        </button>
        <button
          type="button"
          className="chip-menu-item"
          role="menuitem"
          onClick={() => {
            close()
            onOpenManual()
          }}
        >
          {s.tourManual}
        </button>
      </Popover>
    </>
  )
}

import { AnimatePresence, motion } from 'framer-motion'
import { useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { useEscClose } from '../esc'
import { useLayerMotion } from '../motion'

/** 同时只留一个弹出层：后打开的把前一个关掉。 */
let closeCurrent: (() => void) | null = null

function claimPopover(close: () => void): void {
  if (closeCurrent && closeCurrent !== close) closeCurrent()
  closeCurrent = close
}

function releasePopover(close: () => void): void {
  if (closeCurrent === close) closeCurrent = null
}

/**
 * 贴着锚点的弹出层：菜单、词库列表共用。
 * 渲染到 body，避开任务列表的滚动裁剪；放不下就翻到上方。
 * Esc、点外面、滚动、缩放都会关。
 */
export function Popover({
  open,
  anchor,
  onClose,
  children,
  className = 'chip-menu',
  label,
  trigger
}: {
  open: boolean
  anchor: DOMRect | null
  onClose: () => void
  children: React.ReactNode
  className?: string
  label?: string
  /** 触发按钮；点它自己不算“点外面”，否则菜单会被关掉又立刻重开。 */
  trigger?: React.RefObject<HTMLElement | null>
}) {
  const layer = useLayerMotion()
  return (
    <AnimatePresence>
      {open && anchor && (
        <PopoverBody
          anchor={anchor}
          onClose={onClose}
          className={className}
          label={label}
          motion={layer}
          trigger={trigger}
        >
          {children}
        </PopoverBody>
      )}
    </AnimatePresence>
  )
}

function PopoverBody({
  anchor,
  onClose,
  children,
  className,
  label,
  trigger,
  motion: motionProps
}: {
  anchor: DOMRect
  onClose: () => void
  children: React.ReactNode
  className: string
  label?: string
  trigger?: React.RefObject<HTMLElement | null>
  motion: ReturnType<typeof useLayerMotion>
}) {
  const ref = useRef<HTMLDivElement>(null)
  useEscClose(onClose)

  useEffect(() => {
    claimPopover(onClose)
    return () => releasePopover(onClose)
  }, [onClose, trigger])

  useEffect(() => {
    const onDown = (e: MouseEvent): void => {
      const target = e.target as Node
      if (ref.current?.contains(target)) return
      if (trigger?.current?.contains(target)) return
      onClose()
    }
    const onMove = (e: Event): void => {
      const target = e.target as Node | null
      if (target && ref.current?.contains(target)) return
      onClose()
    }
    window.addEventListener('mousedown', onDown, true)
    window.addEventListener('scroll', onMove, true)
    window.addEventListener('resize', onMove)
    return () => {
      window.removeEventListener('mousedown', onDown, true)
      window.removeEventListener('scroll', onMove, true)
      window.removeEventListener('resize', onMove)
    }
  }, [onClose, trigger])

  useEffect(() => {
    ref.current?.querySelector<HTMLElement>('button')?.focus()
  }, [])

  const onKeyDown = (e: React.KeyboardEvent): void => {
    const items = Array.from(ref.current?.querySelectorAll<HTMLButtonElement>('button') ?? [])
    if (!items.length) return
    const i = items.indexOf(document.activeElement as HTMLButtonElement)
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      items[(i + 1) % items.length]?.focus()
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      items[(i - 1 + items.length) % items.length]?.focus()
    } else if (e.key === 'Home') {
      e.preventDefault()
      items[0]?.focus()
    } else if (e.key === 'End') {
      e.preventDefault()
      items[items.length - 1]?.focus()
    }
  }

  const flipUp = anchor.bottom + 264 > window.innerHeight
  const style: React.CSSProperties = flipUp
    ? { position: 'fixed', left: anchor.left, bottom: window.innerHeight - anchor.top + 6 }
    : { position: 'fixed', left: anchor.left, top: anchor.bottom + 6 }

  return createPortal(
    <motion.div
      ref={ref}
      className={className}
      style={style}
      role="group"
      aria-label={label}
      onClick={(e) => e.stopPropagation()}
      onKeyDown={onKeyDown}
      {...motionProps}
    >
      {children}
    </motion.div>,
    document.body
  )
}

/** 语言、词库这类「从一份列表里选一个」的菜单。任务行和设置用同一套。 */
export function SelectMenu({
  value,
  options,
  onChange,
  ariaLabel,
  className,
  disabled,
  title,
  footer
}: {
  value: string
  options: { id: string; label: string }[]
  onChange: (id: string) => void
  ariaLabel: string
  className?: string
  disabled?: boolean
  title?: string
  /** 菜单底部的额外动作（例如「应用到全部行」），点完会关菜单 */
  footer?: (close: () => void) => React.ReactNode
}) {
  const [anchor, setAnchor] = useState<DOMRect | null>(null)
  const triggerRef = useRef<HTMLButtonElement>(null)
  const current = options.find((o) => o.id === value)
  const close = (): void => setAnchor(null)

  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        className={`${className ?? ''} ${anchor ? 'open' : ''}`}
        disabled={disabled}
        title={title}
        aria-haspopup="listbox"
        aria-expanded={anchor != null}
        aria-label={ariaLabel}
        onClick={(e) =>
          setAnchor((cur) => (cur ? null : e.currentTarget.getBoundingClientRect()))
        }
      >
        <span className="ellipsis">{current?.label ?? value}</span>
        <span className="caret" aria-hidden>
          ▾
        </span>
      </button>
      <Popover
        open={anchor != null}
        anchor={anchor}
        onClose={close}
        label={ariaLabel}
        trigger={triggerRef}
      >
        <div role="listbox" aria-label={ariaLabel}>
          {options.map((o) => (
            <button
              type="button"
              key={o.id}
              role="option"
              aria-selected={o.id === value}
              className={`chip-menu-item ${o.id === value ? 'active' : ''}`}
              onClick={() => {
                onChange(o.id)
                close()
              }}
            >
              {o.label}
            </button>
          ))}
          {footer?.(close)}
        </div>
      </Popover>
    </>
  )
}

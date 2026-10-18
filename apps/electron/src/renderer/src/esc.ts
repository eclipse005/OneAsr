import { useEffect } from 'react'

/**
 * Esc 关闭「最上层」浮层/菜单。
 *
 * 用一个后进先出的栈：谁最后打开谁先被 Esc 关掉（菜单 → 面板 → …），
 * 只装一个全局 keydown 监听，各组件只管注册自己。
 */
const stack: Array<() => void> = []
let installed = false

function onKeyDown(event: KeyboardEvent): void {
  if (event.key !== 'Escape') return
  const top = stack[stack.length - 1]
  if (!top) return
  event.preventDefault()
  event.stopPropagation()
  top()
}

export function useEscClose(onClose: () => void, active = true): void {
  useEffect(() => {
    if (!active) return
    if (!installed) {
      window.addEventListener('keydown', onKeyDown, true)
      installed = true
    }
    const entry = (): void => onClose()
    stack.push(entry)
    return () => {
      const index = stack.lastIndexOf(entry)
      if (index >= 0) stack.splice(index, 1)
    }
  }, [onClose, active])
}

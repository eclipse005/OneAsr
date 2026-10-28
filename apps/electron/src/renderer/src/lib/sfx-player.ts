/**
 * 渲染进程播放器：主进程通过 `sfx:play` 下发 kind，这里只负责出声。
 *
 * - click：连点时停掉上一段再播（替换，不叠）。
 * - reminder：独立实例；主进程已在提醒期间 mute 点击，这里不主动掐提醒。
 * 窗口藏到托盘时 BrowserWindow 仍在，HTMLAudioElement 照样响。
 */

import clickUrl from '@assets/sounds/click.wav?url'
import reminderUrl from '@assets/sounds/reminder.wav?url'

const URLS = {
  click: clickUrl,
  reminder: reminderUrl
} as const

export type SfxPlayKind = keyof typeof URLS

let clickAudio: HTMLAudioElement | null = null
let reminderAudio: HTMLAudioElement | null = null

function playClip(kind: SfxPlayKind): void {
  const url = URLS[kind]
  if (kind === 'click') {
    if (!clickAudio) clickAudio = new Audio(url)
    const a = clickAudio
    try {
      a.pause()
      a.currentTime = 0
      void a.play().catch(() => {
        /* autoplay / 解码失败：静默 */
      })
    } catch {
      /* ignore */
    }
    return
  }
  if (!reminderAudio) reminderAudio = new Audio(url)
  const a = reminderAudio
  try {
    a.pause()
    a.currentTime = 0
    void a.play().catch(() => {
      /* ignore */
    })
  } catch {
    /* ignore */
  }
}

/** 挂上 preload 的 onSfxPlay；返回卸载函数。 */
export function installSfxPlayer(
  onSfxPlay: (cb: (kind: SfxPlayKind) => void) => () => void
): () => void {
  return onSfxPlay((kind) => {
    if (kind === 'click' || kind === 'reminder') playClip(kind)
  })
}

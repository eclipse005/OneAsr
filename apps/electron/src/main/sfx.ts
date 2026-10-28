/**
 * UI 提示音（主进程决策、渲染进程播放）。
 *
 * 两个片段、两个角色 —— 与旧版 GPUI `sfx.rs` 同语义：
 *
 * | Clip | Role |
 * |------|------|
 * | click | 交互轻触：加文件 / 删行 / 清空 / 开始 / 显式保存设置 |
 * | reminder | 整批跑完：成功与失败共用；结果由提示条颜色与文案区分 |
 *
 * **什么值得出声。** 判据是「有没有改队列，或有没有提交到磁盘」——不是「点没点」：
 * - 会响：加/删文件、删任务、清空、开始一批、显式保存。
 * - 不响：设置抽屉里的即时开关、hover、滚动、抽屉关闭时的自动行为。
 *
 * 一次操作一声，从不按文件数连打：拖入 20 个文件或清空 20 行只响一次。
 * 提醒音同理 —— 只在忙→闲时响一次（见 JobManager 的 batch 收尾），不按行。
 *
 * 音量写在 WAV 里，代码不调增益；不要加运行时音量旋钮。
 *
 * 播放走渲染进程的 HTMLAudioElement（跨平台；窗口藏到托盘时 webContents 仍在，照样响）。
 * 提醒音播放期间点击音自动让位（mute-until = WAV 时长 + 150ms），避免把长提醒掐断。
 */

import fs from 'fs'
import path from 'path'
import type { BrowserWindow } from 'electron'
import { loadSettings } from './settings'

export type SfxKind = 'click' | 'reminder'

/** Unix 毫秒截止：在此之前点击音静默（提醒音占着「通道」）。 */
let tapMuteUntilMs = 0
let soundEnabled = true
let reminderDurationMs: number | null = null
let getWindow: (() => BrowserWindow | null) | null = null

function nowMs(): number {
  return Date.now()
}

/** 把播放通道绑到主窗口（托盘藏窗时窗口对象仍在）。 */
export function bindSfxWindow(getter: () => BrowserWindow | null): void {
  getWindow = getter
}

/** 设置显式保存 / 重置后刷新开关。 */
export function setSoundEnabled(on: boolean): void {
  soundEnabled = !!on
}

/** 从当前 settings.json 读一次 `sound`。 */
export function refreshSoundFromSettings(): void {
  try {
    soundEnabled = !!loadSettings().sound
  } catch {
    soundEnabled = true
  }
}

export function playClick(): void {
  play('click')
}

export function playReminder(): void {
  play('reminder')
}

export function play(kind: SfxKind): void {
  if (!soundEnabled) return
  if (kind === 'click' && nowMs() < tapMuteUntilMs) return
  if (kind === 'reminder') {
    // 占住通道：整段时长 + 一小段尾，避免衰减被尾部落点掐掉。
    tapMuteUntilMs = nowMs() + reminderDurationCached() + 150
  }
  const win = getWindow?.()
  if (!win || win.isDestroyed()) return
  try {
    win.webContents.send('sfx:play', kind)
  } catch {
    /* 窗口正在关：静默丢弃 */
  }
}

/**
 * 从 RIFF 头读 PCM WAV 时长（毫秒）。
 * 硬编码会在重新切音后悄悄过期；头里已经有答案。
 * 认不出就返回 0 —— 「不要占通道」，从不抛、从不瞎 mute。
 */
export function wavDurationMs(bytes: Uint8Array | Buffer): number {
  if (bytes.length < 12) return 0
  if (
    bytes[0] !== 0x52 ||
    bytes[1] !== 0x49 ||
    bytes[2] !== 0x46 ||
    bytes[3] !== 0x46 ||
    bytes[8] !== 0x57 ||
    bytes[9] !== 0x41 ||
    bytes[10] !== 0x56 ||
    bytes[11] !== 0x45
  ) {
    return 0
  }
  const u32 = (i: number): number =>
    bytes[i]! | (bytes[i + 1]! << 8) | (bytes[i + 2]! << 16) | (bytes[i + 3]! << 24)

  let pos = 12
  let byteRate = 0
  while (pos + 8 <= bytes.length) {
    const size = u32(pos + 4) >>> 0
    const body = pos + 8
    const tag0 = bytes[pos]!
    const tag1 = bytes[pos + 1]!
    const tag2 = bytes[pos + 2]!
    const tag3 = bytes[pos + 3]!
    const isFmt = tag0 === 0x66 && tag1 === 0x6d && tag2 === 0x74 && tag3 === 0x20
    const isData = tag0 === 0x64 && tag1 === 0x61 && tag2 === 0x74 && tag3 === 0x61
    if (isFmt && size >= 16 && body + 12 <= bytes.length) {
      // fmt: format(0..2) channels(2..4) sample_rate(4..8) byte_rate(8..12)
      byteRate = u32(body + 8) >>> 0
    } else if (isData) {
      if (byteRate === 0) return 0
      const payload = Math.min(size, bytes.length - body)
      return Math.floor((payload * 1000) / byteRate)
    }
    // 块按字对齐；奇数 size 多一个 pad 字节。
    pos = body + size + (size & 1)
  }
  return 0
}

/** 开发期源码树 / 打包后 resources/sounds 都能找到 reminder.wav。 */
export function resolveReminderWavPath(): string | undefined {
  const name = 'reminder.wav'
  const candidates = [
    // electron-vite：out/main → 仓库根 assets/sounds
    path.join(__dirname, '../../../../assets/sounds', name),
    // electron-builder extraResources → resources/sounds
    ...(process.resourcesPath ? [path.join(process.resourcesPath, 'sounds', name)] : []),
    path.join(__dirname, '../../assets/sounds', name)
  ]
  return candidates.find((p) => fs.existsSync(p))
}

function reminderDurationCached(): number {
  if (reminderDurationMs != null) return reminderDurationMs
  const p = resolveReminderWavPath()
  if (!p) {
    reminderDurationMs = 0
    return 0
  }
  try {
    reminderDurationMs = wavDurationMs(fs.readFileSync(p))
  } catch {
    reminderDurationMs = 0
  }
  return reminderDurationMs
}

/** 测试用：重置 mute / 时长缓存。 */
export function _resetSfxForTests(): void {
  tapMuteUntilMs = 0
  reminderDurationMs = null
  soundEnabled = true
  getWindow = null
}

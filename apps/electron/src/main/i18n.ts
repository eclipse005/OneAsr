import { app } from 'electron'
import { loadSettings } from './settings'

/**
 * 主进程也要说人话：托盘菜单之类的系统级文案在这里成对写（中/英），
 * 语言跟随设置里的「界面语言」，选「跟随系统」时看系统语言。
 * 渲染层的表在 src/renderer/src/i18n/index.ts，两边各管自己那一层。
 */
export type MainLang = 'zh' | 'en'

const zh = {
  trayTooltip: 'OneAsr — 本地离线转字幕',
  trayOpen: '打开 OneAsr',
  trayQuit: '退出 OneAsr'
}

const en: typeof zh = {
  trayTooltip: 'OneAsr — offline subtitles',
  trayOpen: 'Open OneAsr',
  trayQuit: 'Quit OneAsr'
}

function systemLooksChinese(): boolean {
  try {
    return app.getLocale().toLowerCase().startsWith('zh')
  } catch {
    return false
  }
}

export function mainLang(): MainLang {
  const setting = loadSettings().ui_language
  if (setting === 'zh' || setting === 'en') return setting
  return systemLooksChinese() ? 'zh' : 'en'
}

export function m(): typeof zh {
  return mainLang() === 'zh' ? zh : en
}

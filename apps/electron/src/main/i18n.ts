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
  trayQuit: '退出 OneAsr',
  /**
   * CLI 说跑成了（退出码 0），却没报产物路径。
   *
   * 这时壳**不去猜**文件名（core 的命名规则会变，猜了就可能把上一轮遗留的
   * 同名文件当成本次的产物），于是这一次没有可说清的产物 —— 报失败，
   * 不报假绿。
   */
  taskNoOutput: 'CLI 报告成功但没有给出产物路径，无法确认这次的结果',
  transcriptUnavailable: '这一行的文稿暂不可用',
  noTranscript: '这一行没有文稿',
  cliStatusFailed: '读取 CLI 状态失败'
}

const en: typeof zh = {
  trayTooltip: 'OneAsr — offline subtitles',
  trayOpen: 'Open OneAsr',
  trayQuit: 'Quit OneAsr',
  taskNoOutput: 'The CLI reported success but gave no output path, so this run cannot be confirmed',
  transcriptUnavailable: 'The transcript for this row is not available',
  noTranscript: 'This row has no transcript',
  cliStatusFailed: 'Could not read CLI status'
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

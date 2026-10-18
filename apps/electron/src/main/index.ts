import {
  app,
  BrowserWindow,
  clipboard,
  dialog,
  ipcMain,
  Menu,
  nativeImage,
  shell,
  Tray
} from 'electron'
import { join } from 'path'
import fs from 'fs'
import { DEFAULT_APP_ROOT, defaultSettings, loadSettings, resolveCli, saveSettings } from './settings'
import { runCliJson } from './cli'
import { JobManager } from './jobs'
import { DownloadManager } from './downloads'
import { addSessionContext, initLog, installProcessHandlers, logPathname } from './log'
import { m } from './i18n'
import { loadHotwords, saveHotwords } from './hotword'
import type { HotwordStore, Settings, StatusSnapshot } from '../shared/types'

app.disableHardwareAcceleration()
app.commandLine.appendSwitch('disable-gpu')
app.commandLine.appendSwitch('no-sandbox')

// 任务栏 / 窗口图标：与 GPUI 版同一份 assets/icons/app-icon.ico。
// GPUI 是用 winresource 把图标嵌进 exe，Electron 必须在运行时显式交给窗口。
// AppUserModelID 让任务栏把窗口归到 OneAsr 名下，而不是并进 electron.exe。
app.setAppUserModelId('com.eclipse005.oneasr')

// 诊断日志：`{app_root}/oneasr-error.log`（会话上下文先缓存，出问题才落盘）
initLog(DEFAULT_APP_ROOT)
installProcessHandlers()

/** 定位仓库里那份图标：源码树里跑与将来打包后的 resources 两种落点都试。 */
function resolveAppIcon(): string | undefined {
  const name = process.platform === 'win32' ? 'app-icon.ico' : 'app-icon.png'
  const candidates = [join(__dirname, '../../../../assets/icons', name)] // 源码树：out/main → 仓库根
  if (process.resourcesPath) candidates.push(join(process.resourcesPath, 'icons', name))
  candidates.push(join(__dirname, '../../assets/icons', name))
  return candidates.find((p) => fs.existsSync(p))
}

let mainWindow: BrowserWindow | null = null
let tray: Tray | null = null
/** 区分「用户点退出」与「点了窗口的关闭键」：前者要真退，后者看 close_to_tray。 */
let quitting = false
const jobs = new JobManager()
const downloads = new DownloadManager()

/** 托盘：左键恢复窗口，菜单里「打开 / 退出」。关窗到托盘的行为在 window 的 close 里。 */
function createTray(icon: string | undefined): void {
  const image = icon ? nativeImage.createFromPath(icon) : nativeImage.createEmpty()
  // Windows 托盘要小图；macOS 用 18pt 比较合适
  const sized =
    process.platform === 'win32' ? image.resize({ width: 16, height: 16 }) : image.resize({ width: 18, height: 18 })
  tray = new Tray(sized.isEmpty() ? nativeImage.createEmpty() : sized)
  const t = m()
  tray.setToolTip(t.trayTooltip)
  const showWindow = (): void => {
    if (!mainWindow) {
      createWindow()
      return
    }
    if (mainWindow.isMinimized()) mainWindow.restore()
    mainWindow.show()
    mainWindow.focus()
  }
  tray.setContextMenu(
    Menu.buildFromTemplate([
      { label: t.trayOpen, click: showWindow },
      { type: 'separator' },
      {
        label: t.trayQuit,
        click: () => {
          quitting = true
          app.quit()
        }
      }
    ])
  )
  tray.on('click', showWindow)
}


async function runAutoShots(win: BrowserWindow): Promise<void> {
  const outDir = process.env.ONEASR_SHOT_DIR || join(process.cwd(), 'shots')
  fs.mkdirSync(outDir, { recursive: true })
  const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))
  const waitApi = async () => {
    for (let i = 0; i < 50; i++) {
      const ok = await win.webContents.executeJavaScript(
        '!!window.__oneasrShotApi'
      )
      if (ok) return
      await sleep(200)
    }
    throw new Error('shot api not ready')
  }
  const capture = async (name: string) => {
    await sleep(450)
    const img = await win.webContents.capturePage()
    const file = join(outDir, `${name}.png`)
    fs.writeFileSync(file, img.toPNG())
    console.log('SHOT', file)
  }
  await waitApi()
  await win.webContents.executeJavaScript('window.__oneasrShotApi.dismissWelcome()')
  for (const lang of ['zh', 'en'] as const) {
    await win.webContents.executeJavaScript(
      `window.__oneasrShotApi.setUiLang(${JSON.stringify(lang)})`
    )
    await win.webContents.executeJavaScript(`window.__oneasrShotApi.open('none')`)
    await capture(`${lang}-01-main-empty`)
    for (const [panel, file] of [
      ['settings', '02-settings'],
      ['hotwords', '03-hotwords'],
      ['stats', '04-stats'],
      ['guide', '05-guide']
    ] as const) {
      await win.webContents.executeJavaScript(
        `window.__oneasrShotApi.open(${JSON.stringify(panel)})`
      )
      await capture(`${lang}-${file}`)
      if (panel === 'settings') {
        await win.webContents.executeJavaScript(
          `document.querySelector('[data-shot="settings-tab-models"]')?.click()`
        )
        await capture(`${lang}-02b-settings-models`)
        await win.webContents.executeJavaScript(
          `document.querySelector('[data-shot="settings-tab-hotwords"]')?.click()`
        )
        await sleep(300)
        await win.webContents.executeJavaScript(
          `document.querySelector('.hw-group-trigger')?.click()`
        )
        await capture(`${lang}-02d-settings-hotwords`)
        await win.webContents.executeJavaScript(
          `document.querySelector('.hw-group-trigger')?.click()`
        )
        await win.webContents.executeJavaScript(
          `document.querySelector('[data-shot="settings-tab-compute"]')?.click()`
        )
        await capture(`${lang}-02c-settings-system`)
      }
      await win.webContents.executeJavaScript(`window.__oneasrShotApi.open('none')`)
      await sleep(250)
    }
  }
  console.log('SHOTS_DONE', outDir)
  if (process.env.ONEASR_AUTO_SHOT_EXIT === '1') {
    app.quit()
  }
}

/** 标题栏高度。CSS 里 .titlebar 的 44px 必须跟它一致（overlay 高度、红绿灯对中都读它）。 */
const TITLEBAR_HEIGHT = 44

/**
 * 窗口按钮一律交给系统，不自绘：
 * - Windows / Linux：titleBarStyle hidden + Window Controls Overlay →
 *   min/max/close 归系统（Windows 由 DWM 画；Linux 由 Electron 按桌面主题画，
 *   symbolColor 在 Linux 不传，Electron 自动取满足对比度的颜色）。
 *   Windows 上命中测试是真的 HTMINBUTTON/HTMAXBUTTON/HTCLOSE —— Win11 贴靠布局因此可用。
 * - macOS：titleBarStyle hidden + trafficLightPosition → 左上角红绿灯，
 *   位置对齐 44px 标题栏（系统默认位置是按 ~28px 标题栏给的，不设会偏上）。
 */
function windowChrome(): Partial<Electron.BrowserWindowConstructorOptions> {
  if (process.platform === 'darwin') {
    return {
      titleBarStyle: 'hidden',
      trafficLightPosition: { x: 16, y: Math.round((TITLEBAR_HEIGHT - 14) / 2) }
    }
  }
  const overlay: Electron.TitleBarOverlay = { color: '#ffffff', height: TITLEBAR_HEIGHT }
  if (process.platform === 'win32') overlay.symbolColor = '#475569'
  return { titleBarStyle: 'hidden', titleBarOverlay: overlay }
}

function createWindow(): void {
  const icon = resolveAppIcon()
  // 一行启动诊断：窗口装饰归谁、图标从哪来，出问题时先看这条（Electron 33 没有 getTitleBarOverlay 可查）。
  const chrome = windowChrome()
  console.log(
    `[oneasr] platform=${process.platform} appRoot=${DEFAULT_APP_ROOT} cli=${resolveCli()}\n` +
      `[oneasr] icon=${icon ?? '(none)'} chrome=${JSON.stringify(chrome)}`
  )
  // 这些信息平时只留在内存里；一旦有 WARN/ERROR/PANIC，会连同第一条日志一起写入
  addSessionContext('window', `chrome=${JSON.stringify(chrome)}\nicon=${icon ?? '(none)'}\ncli=${resolveCli()}`)
  // 模型状态也问 CLI（判据在 Rust），随手把结果塞进诊断日志的会话上下文
  void runCliJson<StatusSnapshot>(['status', '--json'], 20000)
    .then((r) => {
      if (!r.ok || !r.data) return
      const m = r.data.models
      addSessionContext(
        'models (oneasr-cli status)',
        `asr=${m.asr.id} ready=${m.asr.ready} path=${m.asr.dir}\n` +
          `aligner=${m.aligner.id} ready=${m.aligner.ready} path=${m.aligner.dir}\n` +
          `demucs=${m.demucs.ready} sensevoice=${m.sensevoice.ready} backend=${r.data.backend}\n` +
          `can_start=${r.data.can_start.ok ? 'ok' : (r.data.can_start.reason ?? 'no')}`
      )
    })
    .catch(() => undefined)
  void app
    .getGPUInfo('basic')
    .then((info) => addSessionContext('gpu', JSON.stringify(info).slice(0, 800)))
    .catch(() => undefined)
  mainWindow = new BrowserWindow({
    width: 1200,
    height: 800,
    minWidth: 900,
    minHeight: 620,
    ...windowChrome(),
    backgroundColor: '#f4f6f8',
    title: 'OneAsr',
    icon,
    webPreferences: {
      preload: join(__dirname, '../preload/index.js'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: false
    }
  })

  // 关窗：开了「关闭时隐藏到托盘」就藏起来（任务继续跑），否则按真关闭走
  mainWindow.on('close', (event) => {
    if (quitting) return
    if (loadSettings().close_to_tray) {
      event.preventDefault()
      mainWindow?.hide()
    }
  })

  if (process.env.ELECTRON_RENDERER_URL) {
    void mainWindow.loadURL(process.env.ELECTRON_RENDERER_URL)
  } else {
    void mainWindow.loadFile(join(__dirname, '../renderer/index.html'))
  }

  if (process.env.ONEASR_AUTO_SHOT === '1') {
    mainWindow.webContents.once('did-finish-load', () => {
      void runAutoShots(mainWindow!).catch((e) => {
        console.error('AUTO_SHOT_FAIL', e)
        if (process.env.ONEASR_AUTO_SHOT_EXIT === '1') app.quit()
      })
    })
  }

  jobs.on('tasks', (tasks) => mainWindow?.webContents.send('tasks:updated', tasks))
  jobs.on('progress', (payload) => mainWindow?.webContents.send('task:progress', payload))
  jobs.on('stats', (stats) => mainWindow?.webContents.send('stats:updated', stats))
}

function registerIpc(): void {
  ipcMain.handle('app:info', () => ({
    appRoot: DEFAULT_APP_ROOT,
    cliPath: resolveCli(),
    display: process.env.DISPLAY,
    version: '0.2.0-electron'
  }))

  ipcMain.handle('settings:get', () => loadSettings())
  ipcMain.handle('settings:save', (_e, settings: Settings) => saveSettings(settings))
  ipcMain.handle('settings:reset', () => saveSettings(defaultSettings()))
  /**
   * 模型状态、就绪判据、开跑门禁、可选模型清单（catalog）——全部来自
   * `oneasr-cli status --json`。壳只转发，不再自己判断目录够不够。
   */
  ipcMain.handle('models:status', async () => {
    const r = await runCliJson<StatusSnapshot>(['status', '--json'], 20000)
    return r.ok ? r.data : null
  })
  // 下载：一次一个，进度从 CLI 的 stdout 机器行来
  ipcMain.handle('download:start', (_e, model: string) => downloads.start(model))
  ipcMain.handle('download:cancel', () => {
    downloads.cancel()
  })
  ipcMain.handle('download:state', () => downloads.state())
  ipcMain.handle('models:openFolder', async () => {
    const dir = join(DEFAULT_APP_ROOT, 'models')
    await shell.openPath(dir)
  })

  ipcMain.handle('dialog:pickFiles', async () => {
    try {
      const r = await dialog.showOpenDialog(mainWindow!, {
        properties: ['openFile', 'multiSelections'],
        filters: [
          {
            name: 'Media',
            extensions: ['wav', 'mp3', 'flac', 'm4a', 'aac', 'ogg', 'mp4', 'mkv', 'mov', 'webm', 'avi']
          },
          { name: 'All', extensions: ['*'] }
        ]
      })
      return { ok: true as const, paths: r.canceled ? [] : r.filePaths }
    } catch (e) {
      return { ok: false as const, paths: [] as string[], error: String(e) }
    }
  })

  ipcMain.handle('dialog:pickDirectory', async (_e, defaultPath?: string) => {
    try {
      const r = await dialog.showOpenDialog(mainWindow!, {
        properties: ['openDirectory', 'createDirectory'],
        defaultPath
      })
      return { ok: true as const, path: r.canceled ? null : r.filePaths[0] ?? null }
    } catch (e) {
      return { ok: false as const, path: null, error: String(e) }
    }
  })

  ipcMain.handle('dialog:pickModelDir', async (_e, defaultPath?: string) => {
    try {
      const r = await dialog.showOpenDialog(mainWindow!, {
        properties: ['openDirectory'],
        defaultPath
      })
      return { ok: true as const, path: r.canceled ? null : r.filePaths[0] ?? null }
    } catch (e) {
      return { ok: false as const, path: null, error: String(e) }
    }
  })

  ipcMain.handle('shell:openPath', async (_e, p: string) => {
    if (p) await shell.openPath(p)
  })
  // 复制走主进程的 clipboard：渲染层 file:// 下 navigator.clipboard 不保证可用
  ipcMain.handle('shell:copyText', (_e, text: string) => {
    clipboard.writeText(String(text ?? ''))
  })
  /** 出问题时一键把日志拿出来（还没写过就只打开所在目录）。 */
  ipcMain.handle('shell:openLog', async () => {
    const file = logPathname()
    if (file && fs.existsSync(file)) await shell.openPath(file)
    else await shell.openPath(DEFAULT_APP_ROOT)
  })
  ipcMain.handle('shell:openOutputFolder', async (_e, filePath: string) => {
    if (filePath) shell.showItemInFolder(filePath)
  })

  ipcMain.handle('tasks:list', () => jobs.list())
  ipcMain.handle('tasks:add', (_e, paths: string[]) => jobs.add(paths))
  ipcMain.handle('tasks:remove', (_e, id: string) => jobs.remove(id))
  ipcMain.handle('tasks:clear', () => jobs.clear())
  // 门禁结果要回给渲染层（翻文案 / 翻设置页），所以这里必须把值传回去
  ipcMain.handle('tasks:start', (_e, id: string) => jobs.startOne(id))
  ipcMain.handle('tasks:startAll', () => jobs.startAll())
  ipcMain.handle('tasks:cancel', (_e, id: string) => {
    jobs.cancel(id)
  })
  // 行内改（运行中会被 JobManager.patch 挡掉）
  ipcMain.handle('tasks:setLanguage', (_e, id: string, language: string) =>
    jobs.setLanguage(id, language)
  )
  ipcMain.handle('tasks:setVocalSeparation', (_e, id: string, on: boolean) =>
    jobs.setVocalSeparation(id, on)
  )
  ipcMain.handle('tasks:setHotwordGroup', (_e, id: string, groupId: string | null) =>
    jobs.setHotwordGroup(id, groupId)
  )
  ipcMain.handle('stats:get', () => jobs.getStats())

  ipcMain.handle('hotword:get', () => loadHotwords())
  ipcMain.handle('hotword:save', (_e, store: HotwordStore) => saveHotwords(store))

  ipcMain.handle('window:minimize', () => mainWindow?.minimize())
  ipcMain.handle('window:maximize', () => {
    if (!mainWindow) return
    if (mainWindow.isMaximized()) mainWindow.unmaximize()
    else mainWindow.maximize()
  })
  ipcMain.handle('window:close', () => mainWindow?.close())
}

app.whenReady().then(() => {
  registerIpc()
  // macOS 的 Dock 图标不认 BrowserWindow.icon：打包后由 .app 里的 AppIcon.icns 决定
  // （scripts/pack-unix.sh 用 iconutil 生成），未打包开发期这里先顶上同一份图标。
  if (process.platform === 'darwin') {
    const dockIcon = resolveAppIcon()
    if (dockIcon) app.dock?.setIcon(dockIcon)
  }
  createWindow()
  createTray(resolveAppIcon())
  // 托盘 tooltip 跟着当前阶段走（GPUI 的 tray.rs 同款）：长任务挂着的时候一眼能看到进展
  // 下载进度转发给渲染层（设置 → 模型 里的下载区）
  downloads.on('updated', (state) => mainWindow?.webContents.send('download:updated', state))
  jobs.on('tasks', (list: { status: string; stage?: string }[]) => {
    if (!tray) return
    const running = list.find((t) => t.status === 'running')
    const base = m().trayTooltip
    tray.setToolTip(running?.stage ? `${base} · ${running.stage}` : base)
  })
  console.log('[oneasr] tray ready')
})

// 退出前把在跑的 CLI 收掉（否则子进程会变孤儿），并让 close 处理器知道这是真退出
app.on('before-quit', () => {
  quitting = true
  jobs.shutdown()
  downloads.shutdown()
})

// macOS：点 Dock 图标 / 关掉唯一窗口后想再打开
app.on('activate', () => {
  if (!mainWindow) {
    createWindow()
    return
  }
  if (mainWindow.isMinimized()) mainWindow.restore()
  mainWindow.show()
})

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') app.quit()
})

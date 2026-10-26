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
import {
  DEFAULT_APP_ROOT,
  dataRoot,
  defaultSettings,
  loadSettings,
  resolveCli,
  saveSettings,
  setUserDataDir
} from './settings'
import { probeCliCapabilities, runCliJson } from './cli'
import { JobManager } from './jobs'
import { DownloadManager } from './downloads'
import {
  addSessionContext,
  clearDiagnosticLog,
  initLog,
  installProcessHandlers,
  logFileToOpen,
  logInfo
} from './log'
import { m } from './i18n'
import { loadHotwords, saveHotwords } from './hotword'
import type { AddTaskInput, HotwordStore, Settings, StatusSnapshot } from '../shared/types'

app.disableHardwareAcceleration()
app.commandLine.appendSwitch('disable-gpu')
app.commandLine.appendSwitch('no-sandbox')

// 任务栏 / 窗口图标：与旧版共享 assets/icons/app-icon.ico。
// 旧版用 winresource 把图标嵌进 exe，Electron 必须在运行时显式交给窗口。
// AppUserModelID 让任务栏把窗口归到 OneAsr 名下，而不是并进 electron.exe。
app.setName('OneAsr')
app.setAppUserModelId('com.eclipse005.oneasr')

// 诊断日志在 app ready 后初始化到 data root（打包后可能不是安装目录）。
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
  const shotOnly = process.env.ONEASR_SHOT_ONLY || ''
  if (shotOnly === 'models') {
    for (const lang of ['zh', 'en'] as const) {
      await win.webContents.executeJavaScript(
        `window.__oneasrShotApi.setUiLang(${JSON.stringify(lang)})`
      )
      await win.webContents.executeJavaScript(`window.__oneasrShotApi.open('settings')`)
      await win.webContents.executeJavaScript(
        `window.__oneasrShotApi.setUiLang(${JSON.stringify(lang)})`
      )
      await sleep(200)
      await win.webContents.executeJavaScript(
        `document.querySelector('[data-shot="settings-tab-models"]')?.click()`
      )
      await sleep(350)
      await capture(`${lang}-02b-settings-models`)
      const scrollInfo = await win.webContents.executeJavaScript(`(() => {
        const pane = document.querySelector('[data-shot="settings-pane-models"]');
        if (!pane) return null;
        const cs = getComputedStyle(pane);
        const inner = pane.querySelector('.settings-pane-inner');
        const ics = inner ? getComputedStyle(inner) : null;
        const kids = [...(inner?.children || [])].map((el) => ({
          tag: el.className?.toString?.().slice(0, 48) || el.tagName,
          h: Math.round(el.getBoundingClientRect().height)
        }));
        return {
          pane: { sh: pane.scrollHeight, ch: pane.clientHeight, overflow: cs.overflowY },
          inner: inner ? { sh: inner.scrollHeight, ch: inner.clientHeight, overflow: ics.overflowY } : null,
          fits: pane.scrollHeight <= pane.clientHeight + 1,
          kids
        };
      })()`)
      console.log('MODELS_PANE_SCROLL', lang, JSON.stringify(scrollInfo))
      await win.webContents.executeJavaScript(`window.__oneasrShotApi.open('none')`)
      await sleep(200)
    }
    if (process.env.ONEASR_AUTO_SHOT_EXIT === '1') {
      app.quit()
    }
    return
  }
  if (shotOnly !== 'transcript' && shotOnly !== 'tasks') {
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
      // open() 会从 settings 克隆 draft；再钉一次语言，避免 en 截图串成中文
      await win.webContents.executeJavaScript(
        `window.__oneasrShotApi.setUiLang(${JSON.stringify(lang)})`
      )
      await sleep(200)
      await capture(`${lang}-${file}`)
      if (panel === 'settings') {
        await capture(`${lang}-02a-settings-general`)
        await win.webContents.executeJavaScript(
          `document.querySelector('[data-shot="settings-tab-output"]')?.click()`
        )
        await capture(`${lang}-02e-settings-output`)
        await win.webContents.executeJavaScript(
          `document.querySelector('[data-shot="settings-tab-models"]')?.click()`
        )
        await sleep(350)
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
  } // end !transcript-only panels
  // 有任务 / 文稿面板：用 review-fixtures（demo.wav + 同名 demo.txt）
  for (const lang of ['zh', 'en'] as const) {
    await win.webContents.executeJavaScript(
      `window.__oneasrShotApi.setUiLang(${JSON.stringify(lang)})`
    )
    await win.webContents.executeJavaScript(`window.__oneasrShotApi.open('none')`)
    // 本地截图夹具：相对主进程产物 out/main → apps/electron/shots/…
    // 勿写死 /workspace/…（Windows / 其它机器路径不同）
    const demo = join(DEFAULT_APP_ROOT, 'shots-demo.wav')
    const demoAlt = join(__dirname, '..', '..', 'shots', 'review-fixtures', 'demo.wav')
    const media = fs.existsSync(demoAlt) ? demoAlt : demo
    const transcript = media.replace(/\.wav$/i, '.txt')
    if (fs.existsSync(media)) {
      await win.webContents.executeJavaScript(
        `window.oneasr.addTasks([${JSON.stringify(media)}]).then(() => true)`
      )
      await sleep(800)
      await capture(`${lang}-06-main-with-tasks`)
      // 挂上同名文稿后再点芯片，才能真正打开文稿面板（否则会走选文件对话框）
      if (fs.existsSync(transcript)) {
        const attached = await win.webContents.executeJavaScript(`(async () => {
          const tasks = await window.oneasr.listTasks();
          const id = tasks[0]?.id;
          if (!id) return { ok: false, error: 'no task' };
          await window.oneasr.setTaskTranscript(id, ${JSON.stringify(transcript)});
          // 等 React 收到 tasks:updated，芯片出现 .has
          for (let i = 0; i < 40; i++) {
            const has = !!document.querySelector('.row-chip.transcript-chip.has');
            if (has) break;
            await new Promise((r) => setTimeout(r, 100));
          }
          document.querySelector('.row-chip.transcript-chip.has')?.click();
          // 等文稿面板挂上（TranscriptPanel → .transcript-shell / .transcript-editor）
          for (let i = 0; i < 50; i++) {
            if (document.querySelector('.transcript-shell, .transcript-editor')) return { ok: true, id };
            await new Promise((r) => setTimeout(r, 100));
          }
          return {
            ok: false,
            id,
            hasChip: !!document.querySelector('.row-chip.transcript-chip.has'),
            error: 'transcript panel not visible'
          };
        })()`)
        console.log('SHOT_TRANSCRIPT_ATTACH', lang, attached)
        await sleep(600)
      } else {
        console.warn('SHOT_NO_TRANSCRIPT_FIXTURE', transcript)
        await win.webContents.executeJavaScript(
          `document.querySelector('.row-chip.transcript-chip')?.click()`
        )
        await sleep(500)
      }
      await capture(`${lang}-07-transcript-panel`)
      await win.webContents.executeJavaScript(`window.__oneasrShotApi.open('none')`)
      // 关文稿面板：清 transcriptId
      await win.webContents.executeJavaScript(`(async () => {
        // 点面板关闭按钮（若有）
        document.querySelector('[data-shot="transcript-close"], .transcript-close, .tp-close')?.click();
        await window.oneasr.clearTasks();
        return true;
      })()`)
      await sleep(300)
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
 *   min/max/close 归系统（Windows 由 DWM 画；Linux 由 Electron 画）。
 *   overlay.color / symbolColor 必须跟渲染进程 .titlebar 底色一致（#fff / #475569），
 *   否则右上角会多出一块「更亮」的白矩形。引导遮罩也不得压暗标题栏
 *   （见 TourOverlay TITLEBAR_H），否则同样会露出这块原生白底。
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
  // 与 renderer .titlebar { background: #fff } 同色；符号取 slate，深浅主题都靠这一套浅色 chrome
  const overlay: Electron.TitleBarOverlay = {
    color: '#ffffff',
    symbolColor: '#475569',
    height: TITLEBAR_HEIGHT
  }
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
    width: 900,
    height: 640,
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
  // macOS 关掉唯一窗口后进程还活着；不置空的话 activate / 托盘都会对
  // 已销毁的 BrowserWindow 调方法（“Object has been destroyed”）。
  mainWindow.on('closed', () => {
    mainWindow = null
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
}

function registerIpc(): void {
  ipcMain.handle('app:info', async () => ({
    appRoot: DEFAULT_APP_ROOT,
    dataRoot: dataRoot(),
    cliPath: resolveCli(),
    display: process.env.DISPLAY,
    version: app.getVersion(),
    cli: await probeCliCapabilities()
  }))

  ipcMain.handle('settings:get', () => loadSettings())
  ipcMain.handle('settings:save', (_e, settings: Settings) => saveSettings(settings))
  ipcMain.handle('settings:reset', () => saveSettings(defaultSettings()))
  /**
   * 模型状态、就绪判据、开跑门禁、可选模型清单（catalog）——全部来自
   * `oneasr-cli status --json`。壳只转发，不再自己判断目录够不够。
   */
  ipcMain.handle('tasks:applyToAll', (_e, id: string) => jobs.applyToAll(id))
  ipcMain.handle('models:status', async () => {
    const r = await runCliJson<StatusSnapshot>(['status', '--json'], 20000)
    return r.ok ? r.data : null
  })
  // 下载：不同模型并行，进度从各自的 CLI stdout 机器行来
  ipcMain.handle('download:start', (_e, model: string, targetDir?: string) =>
    downloads.start(model, targetDir)
  )
  ipcMain.handle('download:cancel', (_e, model: string) => {
    downloads.cancel(model)
  })
  ipcMain.handle('download:state', () => downloads.state())
  ipcMain.handle('models:openFolder', async () => {
    const dir = join(dataRoot(), 'models')
    await shell.openPath(dir)
  })

  ipcMain.handle('dialog:pickFiles', async () => {
    try {
      // 过滤器也问 CLI 要扩展名：媒体与文稿两份表都在 core，壳不复制。
      const status = await runCliJson<StatusSnapshot>(['status', '--json'], 20000)
      const media = status.ok ? (status.data?.media_exts ?? []) : []
      const transcripts = status.ok ? (status.data?.transcript_exts ?? []) : []
      const all = [...media, ...transcripts]
      const filters =
        all.length > 0
          ? [
              { name: 'All supported', extensions: all },
              ...(media.length > 0 ? [{ name: 'Media', extensions: media }] : []),
              ...(transcripts.length > 0
                ? [{ name: 'Transcript', extensions: transcripts }]
                : [])
            ]
          : [{ name: 'All', extensions: ['*'] }]
      const r = await dialog.showOpenDialog(mainWindow!, {
        properties: ['openFile', 'multiSelections'],
        filters
      })
      return { ok: true as const, paths: r.canceled ? [] : r.filePaths }
    } catch (e) {
      return { ok: false as const, paths: [] as string[], error: String(e) }
    }
  })

  ipcMain.handle('dialog:pickTranscript', async () => {
    try {
      const status = await runCliJson<StatusSnapshot>(['status', '--json'], 20000)
      const exts = status.ok ? (status.data?.transcript_exts ?? []) : []
      const filters =
        exts.length > 0
          ? [
              { name: 'Transcript', extensions: exts },
              { name: 'All', extensions: ['*'] }
            ]
          : [{ name: 'All', extensions: ['*'] }]
      const r = await dialog.showOpenDialog(mainWindow!, {
        properties: ['openFile'],
        filters
      })
      return { ok: true as const, path: r.canceled ? null : r.filePaths[0] ?? null }
    } catch (e) {
      return { ok: false as const, path: null, error: String(e) }
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
  /** 设置页：当前日志和上一份轮转加在一起的字节数。 */
  ipcMain.handle('shell:logInfo', () => logInfo())
  /** 删掉这两份。下一笔错误会重新建文件并写会话头。 */
  ipcMain.handle('shell:clearLog', () => clearDiagnosticLog())
  /** 出问题时一键把日志拿出来（还没写过就只打开所在目录）。 */
  ipcMain.handle('shell:openLog', async () => {
    const file = logFileToOpen()
    if (file) await shell.openPath(file)
    else await shell.openPath(DEFAULT_APP_ROOT)
  })
  ipcMain.handle('shell:openOutputFolder', async (_e, filePath: string) => {
    if (filePath) shell.showItemInFolder(filePath)
  })

  ipcMain.handle('tasks:list', () => jobs.list())
  ipcMain.handle('tasks:add', (_e, items: Array<string | AddTaskInput>) => jobs.add(items))
  ipcMain.handle('tasks:remove', (_e, id: string) => jobs.remove(id))
  ipcMain.handle('tasks:clear', () => jobs.clear())
  // 门禁结果要回给渲染层（翻文案 / 翻设置页），所以这里必须把值传回去
  ipcMain.handle('tasks:start', (_e, id: string) => jobs.startOne(id))
  ipcMain.handle('tasks:startAll', () => jobs.startAll())
  ipcMain.handle('tasks:cancel', (_e, id: string) => {
    jobs.cancel(id)
  })
  ipcMain.handle('transcript:get', (_e, id: string) => jobs.getTranscript(id))
  ipcMain.handle('transcript:set', async (_e, id: string, p: string | null) =>
    jobs.setTranscript(id, p)
  )
  ipcMain.handle('transcript:save', async (_e, id: string, text: string) =>
    jobs.saveTranscript(id, text)
  )
  ipcMain.handle('transcript:split', async (_e, id: string) => jobs.splitTranscript(id))
  ipcMain.handle('transcript:reread', async (_e, id: string) => jobs.rereadTranscript(id))
  ipcMain.handle('transcript:remove', (_e, id: string) => jobs.removeTranscript(id))
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
  // 打包后安装目录可能只读（Linux /opt、macOS .app）；数据先落到 userData，
  // 再初始化日志/设置/模型目录。
  setUserDataDir(app.getPath('userData'))
  initLog(dataRoot())
  registerIpc()
  // macOS 的 Dock 图标不认 BrowserWindow.icon：打包后由 .app 里的 AppIcon.icns 决定
  // （scripts/pack-unix.sh 用 iconutil 生成），未打包开发期这里先顶上同一份图标。
  if (process.platform === 'darwin') {
    const dockIcon = resolveAppIcon()
    if (dockIcon) app.dock?.setIcon(dockIcon)
  }
  createWindow()
  createTray(resolveAppIcon())
  // 事件订阅只注册一次：macOS 重开窗口时 createWindow 会被再次调用，
  // 订阅若放在里面，每开一次窗口就多发一份 IPC。
  jobs.on('tasks', (tasks: Array<{ status: string; stage?: string }>) => {
    mainWindow?.webContents.send('tasks:updated', tasks)
    if (!tray) return
    // 托盘 tooltip 跟着当前阶段走（旧版托盘同款）
    const running = tasks.find((t) => t.status === 'running')
    const base = m().trayTooltip
    tray.setToolTip(running?.stage ? `${base} · ${running.stage}` : base)
  })
  jobs.on('stats', (stats) => mainWindow?.webContents.send('stats:updated', stats))
  // 下载进度转发给渲染层（设置 → 模型 里的下载区）
  downloads.on('updated', (state) => mainWindow?.webContents.send('download:updated', state))
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

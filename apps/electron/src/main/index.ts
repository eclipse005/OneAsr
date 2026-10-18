import { app, BrowserWindow, dialog, ipcMain, shell } from 'electron'
import { join } from 'path'
import {
  DEFAULT_APP_ROOT,
  checkModels,
  defaultSettings,
  loadSettings,
  saveSettings
} from './settings'
import { JobManager } from './jobs'
import { loadHotwords, saveHotwords } from './hotword'
import type { HotwordStore, Settings } from '../shared/types'

app.disableHardwareAcceleration()
app.commandLine.appendSwitch('disable-gpu')
app.commandLine.appendSwitch('no-sandbox')

let mainWindow: BrowserWindow | null = null
const jobs = new JobManager()

function createWindow(): void {
  mainWindow = new BrowserWindow({
    width: 1200,
    height: 800,
    minWidth: 900,
    minHeight: 620,
    frame: false,
    backgroundColor: '#f4f6f8',
    title: 'OneAsr',
    webPreferences: {
      preload: join(__dirname, '../preload/index.js'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: false
    }
  })

  if (process.env.ELECTRON_RENDERER_URL) {
    void mainWindow.loadURL(process.env.ELECTRON_RENDERER_URL)
  } else {
    void mainWindow.loadFile(join(__dirname, '../renderer/index.html'))
  }

  jobs.on('tasks', (tasks) => mainWindow?.webContents.send('tasks:updated', tasks))
  jobs.on('progress', (payload) => mainWindow?.webContents.send('task:progress', payload))
  jobs.on('stats', (stats) => mainWindow?.webContents.send('stats:updated', stats))
}

function registerIpc(): void {
  ipcMain.handle('app:info', () => ({
    appRoot: DEFAULT_APP_ROOT,
    cliPath: join(DEFAULT_APP_ROOT, 'oneasr-cli'),
    display: process.env.DISPLAY,
    version: '0.2.0-electron'
  }))

  ipcMain.handle('settings:get', () => loadSettings())
  ipcMain.handle('settings:save', (_e, settings: Settings) => saveSettings(settings))
  ipcMain.handle('settings:reset', () => saveSettings(defaultSettings()))
  ipcMain.handle('models:check', () => checkModels(loadSettings()))
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
  ipcMain.handle('shell:openOutputFolder', async (_e, filePath: string) => {
    if (filePath) shell.showItemInFolder(filePath)
  })

  ipcMain.handle('tasks:list', () => jobs.list())
  ipcMain.handle('tasks:add', (_e, paths: string[]) => jobs.add(paths))
  ipcMain.handle('tasks:remove', (_e, id: string) => jobs.remove(id))
  ipcMain.handle('tasks:clear', () => jobs.clear())
  ipcMain.handle('tasks:start', (_e, id: string) => {
    jobs.startOne(id)
  })
  ipcMain.handle('tasks:startAll', () => {
    jobs.startAll()
  })
  ipcMain.handle('tasks:cancel', (_e, id: string) => {
    jobs.cancel(id)
  })
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
  createWindow()
})

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') app.quit()
})

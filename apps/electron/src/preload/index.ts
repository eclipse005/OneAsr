import { contextBridge, ipcRenderer } from 'electron'
import type { OneAsrApi } from './api'
import type { HotwordStore, Settings, StatsSnapshot, TaskSnapshot } from '../shared/types'

const api: OneAsrApi = {
  getAppInfo: () => ipcRenderer.invoke('app:info'),
  getSettings: () => ipcRenderer.invoke('settings:get'),
  saveSettings: (settings: Settings) => ipcRenderer.invoke('settings:save', settings),
  resetSettings: () => ipcRenderer.invoke('settings:reset'),
  checkModels: () => ipcRenderer.invoke('models:check'),
  openModelsFolder: () => ipcRenderer.invoke('models:openFolder'),
  pickFiles: () => ipcRenderer.invoke('dialog:pickFiles'),
  pickDirectory: (defaultPath?: string) => ipcRenderer.invoke('dialog:pickDirectory', defaultPath),
  pickModelDir: (defaultPath?: string) => ipcRenderer.invoke('dialog:pickModelDir', defaultPath),
  openPath: (path: string) => ipcRenderer.invoke('shell:openPath', path),
  openOutputFolder: (filePath: string) => ipcRenderer.invoke('shell:openOutputFolder', filePath),
  listTasks: () => ipcRenderer.invoke('tasks:list'),
  addTasks: (paths: string[]) => ipcRenderer.invoke('tasks:add', paths),
  removeTask: (id: string) => ipcRenderer.invoke('tasks:remove', id),
  clearTasks: () => ipcRenderer.invoke('tasks:clear'),
  startTask: (id: string) => ipcRenderer.invoke('tasks:start', id),
  startAll: () => ipcRenderer.invoke('tasks:startAll'),
  cancelTask: (id: string) => ipcRenderer.invoke('tasks:cancel', id),
  getStats: () => ipcRenderer.invoke('stats:get'),
  getHotwords: () => ipcRenderer.invoke('hotword:get'),
  saveHotwords: (store: HotwordStore) => ipcRenderer.invoke('hotword:save', store),
  onTasksUpdated: (cb) => {
    const listener = (_: Electron.IpcRendererEvent, tasks: TaskSnapshot[]): void => cb(tasks)
    ipcRenderer.on('tasks:updated', listener)
    return () => ipcRenderer.removeListener('tasks:updated', listener)
  },
  onTaskProgress: (cb) => {
    const listener = (_: Electron.IpcRendererEvent, payload: { id: string; line: string }): void =>
      cb(payload)
    ipcRenderer.on('task:progress', listener)
    return () => ipcRenderer.removeListener('task:progress', listener)
  },
  onStatsUpdated: (cb) => {
    const listener = (_: Electron.IpcRendererEvent, stats: StatsSnapshot): void => cb(stats)
    ipcRenderer.on('stats:updated', listener)
    return () => ipcRenderer.removeListener('stats:updated', listener)
  },
  windowMinimize: () => ipcRenderer.invoke('window:minimize'),
  windowMaximize: () => ipcRenderer.invoke('window:maximize'),
  windowClose: () => ipcRenderer.invoke('window:close')
}

contextBridge.exposeInMainWorld('oneasr', api)

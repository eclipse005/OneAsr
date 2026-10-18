import type { Settings, ModelStatus, TaskSnapshot, StatsSnapshot, AppInfo } from '../shared/types'

export interface OneAsrApi {
  getAppInfo: () => Promise<AppInfo>
  getSettings: () => Promise<Settings>
  saveSettings: (settings: Settings) => Promise<Settings>
  resetSettings: () => Promise<Settings>
  checkModels: () => Promise<ModelStatus>
  pickFiles: () => Promise<string[]>
  pickDirectory: (defaultPath?: string) => Promise<string | null>
  pickModelDir: (defaultPath?: string) => Promise<string | null>
  openPath: (path: string) => Promise<void>
  openOutputFolder: (filePath: string) => Promise<void>
  listTasks: () => Promise<TaskSnapshot[]>
  addTasks: (paths: string[]) => Promise<TaskSnapshot[]>
  removeTask: (id: string) => Promise<TaskSnapshot[]>
  clearTasks: () => Promise<TaskSnapshot[]>
  startTask: (id: string) => Promise<void>
  startAll: () => Promise<void>
  getStats: () => Promise<StatsSnapshot>
  onTasksUpdated: (cb: (tasks: TaskSnapshot[]) => void) => () => void
  onTaskProgress: (cb: (payload: { id: string; line: string }) => void) => () => void
  windowMinimize: () => Promise<void>
  windowMaximize: () => Promise<void>
  windowClose: () => Promise<void>
}

import type {
  Settings,
  StartResult,
  DownloadState,
  ModelEntry,
  ModelStatus,
  TaskSnapshot,
  StatsSnapshot,
  AppInfo,
  HotwordStore
} from '../shared/types'

export interface DialogFilesResult {
  ok: boolean
  paths: string[]
  error?: string
}
export interface DialogPathResult {
  ok: boolean
  path: string | null
  error?: string
}

export interface OneAsrApi {
  /** process.platform：win32 / darwin / linux —— 窗口按钮归系统还是自绘按它分。 */
  platform: string
  getAppInfo: () => Promise<AppInfo>
  getSettings: () => Promise<Settings>
  saveSettings: (settings: Settings) => Promise<Settings>
  resetSettings: () => Promise<Settings>
  checkModels: () => Promise<ModelStatus>
  listModels: () => Promise<ModelEntry[]>
  startDownload: (model: string) => Promise<{ ok: boolean; code?: string }>
  cancelDownload: () => Promise<void>
  getDownloadState: () => Promise<DownloadState | null>
  onDownloadUpdated: (cb: (state: DownloadState) => void) => () => void
  openModelsFolder: () => Promise<void>
  pickFiles: () => Promise<DialogFilesResult>
  pickDirectory: (defaultPath?: string) => Promise<DialogPathResult>
  pickModelDir: (defaultPath?: string) => Promise<DialogPathResult>
  openPath: (path: string) => Promise<void>
  openOutputFolder: (filePath: string) => Promise<void>
  listTasks: () => Promise<TaskSnapshot[]>
  addTasks: (paths: string[]) => Promise<TaskSnapshot[]>
  removeTask: (id: string) => Promise<TaskSnapshot[]>
  clearTasks: () => Promise<TaskSnapshot[]>
  startTask: (id: string) => Promise<StartResult>
  startAll: () => Promise<StartResult>
  cancelTask: (id: string) => Promise<void>
  setTaskLanguage: (id: string, language: string) => Promise<TaskSnapshot[]>
  setTaskVocalSeparation: (id: string, on: boolean) => Promise<TaskSnapshot[]>
  setTaskHotwordGroup: (id: string, groupId: string | null) => Promise<TaskSnapshot[]>
  copyText: (text: string) => Promise<void>
  openLog: () => Promise<void>
  getStats: () => Promise<StatsSnapshot>
  getHotwords: () => Promise<HotwordStore>
  saveHotwords: (store: HotwordStore) => Promise<HotwordStore>
  onTasksUpdated: (cb: (tasks: TaskSnapshot[]) => void) => () => void
  onTaskProgress: (cb: (payload: { id: string; line: string }) => void) => () => void
  onStatsUpdated: (cb: (stats: StatsSnapshot) => void) => () => void
  windowMinimize: () => Promise<void>
  windowMaximize: () => Promise<void>
  windowClose: () => Promise<void>
}

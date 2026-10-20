import type {
  AddTaskInput,
  Settings,
  StartResult,
  DownloadState,
  StatusSnapshot,
  TaskSnapshot,
  StatsSnapshot,
  AppInfo,
  HotwordStore,
  TranscriptEditResult
} from '../shared/types'

/** 拖放/选择拿到的文件 → 绝对路径（Electron 32+ 只能用 webUtils.getPathForFile） */
export type GetPathForFile = (file: File) => string

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
  /** 拖放取路径（Electron 32+ 用 webUtils.getPathForFile；File.path 已被移除） */
  getPathForFile: (file: File) => string
  getAppInfo: () => Promise<AppInfo>
  getSettings: () => Promise<Settings>
  saveSettings: (settings: Settings) => Promise<Settings>
  resetSettings: () => Promise<Settings>
  /** 模型状态/就绪/门禁/清单：CLI 的 `status --json` 原样转发 */
  getStatus: () => Promise<StatusSnapshot | null>
  startDownload: (model: string, targetDir?: string) => Promise<{ ok: boolean; code?: string }>
  cancelDownload: () => Promise<void>
  getDownloadState: () => Promise<DownloadState | null>
  onDownloadUpdated: (cb: (state: DownloadState) => void) => () => void
  openModelsFolder: () => Promise<void>
  pickFiles: () => Promise<DialogFilesResult>
  pickTranscript: () => Promise<DialogPathResult>
  pickDirectory: (defaultPath?: string) => Promise<DialogPathResult>
  pickModelDir: (defaultPath?: string) => Promise<DialogPathResult>
  openPath: (path: string) => Promise<void>
  openOutputFolder: (filePath: string) => Promise<void>
  listTasks: () => Promise<TaskSnapshot[]>
  /** 加任务：字符串 = 纯媒体；对象 = 媒体 + 可选文稿（配对后的结果） */
  addTasks: (items: Array<string | AddTaskInput>) => Promise<TaskSnapshot[]>
  removeTask: (id: string) => Promise<TaskSnapshot[]>
  clearTasks: () => Promise<TaskSnapshot[]>
  startTask: (id: string) => Promise<StartResult>
  startAll: () => Promise<StartResult>
  cancelTask: (id: string) => Promise<void>
  /** 文稿：读取正文 / 挂载 / 保存编辑 / 智能断句 / 重读 / 移除 */
  getTranscript: (id: string) => Promise<TranscriptEditResult>
  setTaskTranscript: (id: string, path: string | null) => Promise<TaskSnapshot[]>
  saveTaskTranscript: (id: string, text: string) => Promise<TranscriptEditResult>
  splitTaskTranscript: (id: string) => Promise<TranscriptEditResult>
  rereadTaskTranscript: (id: string) => Promise<TranscriptEditResult>
  removeTaskTranscript: (id: string) => Promise<TaskSnapshot[]>
  setTaskLanguage: (id: string, language: string) => Promise<TaskSnapshot[]>
  /** 把这一行的语言/词库/人声分离推给其它行（运行中的行跳过） */
  applyTaskToAll: (id: string) => Promise<TaskSnapshot[]>
  setTaskVocalSeparation: (id: string, on: boolean) => Promise<TaskSnapshot[]>
  setTaskHotwordGroup: (id: string, groupId: string | null) => Promise<TaskSnapshot[]>
  copyText: (text: string) => Promise<void>
  /** 诊断日志当前大小（当前文件 + 轮转的 .old）。没有时 bytes 为 0。 */
  logInfo: () => Promise<{ bytes: number }>
  /** 删掉诊断日志，返回清理后的大小。 */
  clearLog: () => Promise<{ bytes: number }>
  openLog: () => Promise<void>
  getStats: () => Promise<StatsSnapshot>
  getHotwords: () => Promise<HotwordStore>
  saveHotwords: (store: HotwordStore) => Promise<HotwordStore>
  onTasksUpdated: (cb: (tasks: TaskSnapshot[]) => void) => () => void
  onStatsUpdated: (cb: (stats: StatsSnapshot) => void) => () => void
  windowMinimize: () => Promise<void>
  windowMaximize: () => Promise<void>
  windowClose: () => Promise<void>
}

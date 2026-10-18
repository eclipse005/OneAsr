import { AnimatePresence, motion } from 'framer-motion'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { GuidePanel } from './components/GuidePanel'
import { SettingsDrawer, type SettingsTab } from './components/SettingsDrawer'
import { StatsPanel } from './components/StatsPanel'
import { StatusBar } from './components/StatusBar'
import { TaskList } from './components/TaskList'
import { TitleBar } from './components/TitleBar'
import { WelcomeModal } from './components/WelcomeModal'
import { resolveUiLang, t, unsupportedCount } from './i18n'
import { toastMotion } from './motion'

/** 支持的媒体扩展名（与主进程文件对话框的过滤器保持一致）。 */
const MEDIA_RE = /\.(wav|mp3|flac|m4a|aac|ogg|mp4|mkv|mov|webm|avi)$/i
import type {
  HotwordStore,
  DownloadState,
  ModelEntry,
  ModelStatus,
  Settings,
  StartResult,
  StatsSnapshot,
  TaskSnapshot,
  ToastPayload
} from '../../shared/types'

function deepEq(a: unknown, b: unknown): boolean {
  return JSON.stringify(a) === JSON.stringify(b)
}

let toastSeq = 0

export default function App() {
  const [settings, setSettings] = useState<Settings | null>(null)
  const [draft, setDraft] = useState<Settings | null>(null)
  const [models, setModels] = useState<ModelStatus | null>(null)
  const [tasks, setTasks] = useState<TaskSnapshot[]>([])
  const [stats, setStats] = useState<StatsSnapshot | null>(null)
  const [hotwords, setHotwords] = useState<HotwordStore | null>(null)
  const [hotwordDraft, setHotwordDraft] = useState<HotwordStore | null>(null)
  const [settingsBaseline, setSettingsBaseline] = useState<Settings | null>(null)
  const [hotwordsBaseline, setHotwordsBaseline] = useState<HotwordStore | null>(null)
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [statsOpen, setStatsOpen] = useState(false)
  const [guideOpen, setGuideOpen] = useState(false)
  const [settingsTab, setSettingsTab] = useState<SettingsTab>('general')
  const [welcomeOpen, setWelcomeOpen] = useState(false)
  const [dragging, setDragging] = useState(false)
  const [toasts, setToasts] = useState<ToastPayload[]>([])
  const [statusMsg, setStatusMsg] = useState<{ kind: string; text: string } | null>(null)
  const statusTimer = useRef<number | undefined>(undefined)
  /** 正在开系统文件对话框（防重入） */
  const [picking, setPicking] = useState(false)
  const [modelEntries, setModelEntries] = useState<ModelEntry[]>([])
  const [download, setDownload] = useState<DownloadState | null>(null)

  const lang = useMemo(
    () => resolveUiLang(draft?.ui_language || settings?.ui_language || 'system'),
    [draft?.ui_language, settings?.ui_language]
  )
  const s = t(lang)
  const modelsReady = !!models?.ready
  const dirty = !!(
    settingsBaseline &&
    draft &&
    hotwordsBaseline &&
    hotwordDraft &&
    (!deepEq(settingsBaseline, draft) || !deepEq(hotwordsBaseline, hotwordDraft))
  )

  const openSettingsFn = useCallback(
    async (tab: SettingsTab = 'general') => {
      const savedSettings = settings || (await window.oneasr.getSettings())
      const savedHw = hotwords || (await window.oneasr.getHotwords())
      setSettings(savedSettings)
      setHotwords(savedHw)
      const sSnap = structuredClone(savedSettings)
      const hSnap = structuredClone(savedHw)
      setSettingsBaseline(sSnap)
      setHotwordsBaseline(hSnap)
      setDraft(structuredClone(sSnap))
      setHotwordDraft(structuredClone(hSnap))
      setSettingsTab(tab)
      setSettingsOpen(true)
    },
    [settings, hotwords]
  )

  useEffect(() => {
    document.documentElement.lang = lang === 'zh' ? 'zh-CN' : 'en'
  }, [lang])

  useEffect(() => {
    const api = {
      setUiLang: (code: string) => {
        setDraft((d) => (d ? { ...d, ui_language: code } : d))
      },
      open: async (name: 'settings' | 'hotwords' | 'stats' | 'guide' | 'none') => {
        setWelcomeOpen(false)
        if (name === 'stats') {
          setStats(await window.oneasr.getStats())
        }
        if (name === 'hotwords' || name === 'settings') {
          const savedSettings = await window.oneasr.getSettings()
          const savedHw = await window.oneasr.getHotwords()
          setSettings(savedSettings)
          setHotwords(savedHw)
          const sSnap = structuredClone(savedSettings)
          const hSnap = structuredClone(savedHw)
          setSettingsBaseline(sSnap)
          setHotwordsBaseline(hSnap)
          setDraft(structuredClone(sSnap))
          setHotwordDraft(structuredClone(hSnap))
          setSettingsTab(name === 'hotwords' ? 'hotwords' : 'general')
          setSettingsOpen(true)
          setStatsOpen(false)
          setGuideOpen(false)
          return
        }
        setSettingsOpen(false)
        setStatsOpen(name === 'stats')
        setGuideOpen(name === 'guide')
      },
      dismissWelcome: () => setWelcomeOpen(false)
    }
    ;(window as unknown as { __oneasrShotApi?: typeof api }).__oneasrShotApi = api
    return () => {
      delete (window as unknown as { __oneasrShotApi?: typeof api }).__oneasrShotApi
    }
  }, [])

  const pushToast = useCallback((kind: ToastPayload['kind'], message: string) => {
    const id = `toast-${++toastSeq}`
    setToasts((prev) => [...prev, { id, kind, message }])
    setTimeout(() => setToasts((prev) => prev.filter((t) => t.id !== id)), 4200)
    // 状态栏再留一份：toast 4.2s 就没了，长路径/报错要能回看与复制
    setStatusMsg({ kind, text: message })
    if (statusTimer.current) window.clearTimeout(statusTimer.current)
    statusTimer.current = window.setTimeout(
      () => setStatusMsg(null),
      kind === 'warn' || kind === 'error' ? 6000 : 3000
    )
  }, [])

  const refreshModels = useCallback(async () => {
    setModels(await window.oneasr.checkModels())
    setModelEntries(await window.oneasr.listModels())
  }, [])

  const refreshStats = useCallback(async () => {
    setStats(await window.oneasr.getStats())
  }, [])

  useEffect(() => {
    void (async () => {
      const loaded = await window.oneasr.getSettings()
      setSettings(loaded)
      setDraft(loaded)
      setWelcomeOpen(!loaded.welcome_done)
      setTasks(await window.oneasr.listTasks())
      setHotwords(await window.oneasr.getHotwords())
      await refreshModels()
      await refreshStats()
    })()
    const offTasks = window.oneasr.onTasksUpdated((next) => {
      setTasks(next)
      void refreshStats()
    })
    const offStats = window.oneasr.onStatsUpdated((next) => setStats(next))
    const offDownload = window.oneasr.onDownloadUpdated((state) => setDownload(state))
    return () => {
      offTasks()
      offStats()
      offDownload()
    }
  }, [refreshModels, refreshStats])

  // 下载跑完/失败时刷新模型清单与状态点，并给一次提示（CLI 的人读行不进 UI）
  const downloadPhase = download?.state
  useEffect(() => {
    if (downloadPhase !== 'done' && downloadPhase !== 'error') return
    if (downloadPhase === 'done') pushToast('success', s.downloadDone)
    else pushToast('error', s.downloadFailed)
    void refreshModels()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [downloadPhase])

  /** 启动被门禁挡下时：把原因翻译成文案弹出来；模型问题直接把设置翻到「模型」页。 */
  const reportStart = useCallback(
    (r: StartResult) => {
      if (r.ok) return
      const code = r.code ?? 'nothing-to-start'
      const msg =
        code === 'models'
          ? s.blockModels
          : code === 'sep-weights'
            ? s.blockSepWeights
            : code === 'hotwords-empty'
              ? s.blockHotwordsEmpty
              : s.blockNothing
      pushToast('warn', msg)
      if (code === 'models') void openSettingsFn('models')
    },
    [s, pushToast, openSettingsFn]
  )

  const saveAllSettings = async () => {
    if (!draft || !hotwordDraft) return
    const saved = await window.oneasr.saveSettings(draft)
    const savedHw = await window.oneasr.saveHotwords(hotwordDraft)
    setSettings(saved)
    setDraft(saved)
    setHotwords(savedHw)
    setHotwordDraft(structuredClone(savedHw))
    setSettingsBaseline(structuredClone(saved))
    setHotwordsBaseline(structuredClone(savedHw))
    await refreshModels()
    pushToast('success', s.saved)
  }


  const dismissWelcome = async (openSettings: boolean) => {
    if (!draft) return
    const next = { ...draft, welcome_done: true }
    const saved = await window.oneasr.saveSettings(next)
    setSettings(saved)
    setDraft(saved)
    setWelcomeOpen(false)
    if (openSettings) void openSettingsFn('general')
  }

  const addFiles = async (paths?: string[]) => {
    if (picking) return
    let list = paths
    if (!list) {
      // 系统对话框是异步的：不加锁的话连点会开好几个窗口
      setPicking(true)
      try {
        const r = await window.oneasr.pickFiles()
        if (!r.ok) {
          pushToast('error', `${s.dialogFailed}: ${r.error || ''}`)
          return
        }
        list = r.paths
      } finally {
        setPicking(false)
      }
      if (!list.length) {
        pushToast('info', s.noFiles)
        return
      }
    }
    // 拖进来的可能什么都有：只收媒体，其余报一次数——不静默吞掉（GPUI 是静默跳过并记日志）
    const media = list.filter((p) => MEDIA_RE.test(p))
    const skipped = list.length - media.length
    if (skipped > 0) {
      pushToast(media.length === 0 ? 'warn' : 'info', unsupportedCount(lang, skipped))
    }
    if (!media.length) return
    setTasks(await window.oneasr.addTasks(media))
  }

  const onDrop = (e: React.DragEvent) => {
    e.preventDefault()
    setDragging(false)
    // 过滤与提示都在 addFiles 里统一做（对话框与拖放走同一条路）
    const files = Array.from(e.dataTransfer.files || [])
    const paths = files
      .map((f) => (f as File & { path?: string }).path)
      .filter((p): p is string => !!p)
    if (paths.length) void addFiles(paths)
    else pushToast('warn', s.noFiles)
  }

  const emptyStats: StatsSnapshot = {
    tasksOk: 0,
    tasksErr: 0,
    tasksTotal: 0,
    mediaSec: 0,
    processMs: 0,
    savedSec: null,
    avgRealtime: null,
    cues: 0,
    sepTasks: 0,
    langs: [],
    perDay: [],
    longestMediaSec: null,
    fastestSpeed: null,
    firstDay: null,
    recentSession: []
  }

  return (
    <div className="app">
      <TitleBar lang={lang} />
      <div className="toolbar">
        <button className="btn btn-primary" disabled={picking} onClick={() => void addFiles()}>
          {s.add}
        </button>
        <button
          className="btn btn-ghost"
          disabled={!tasks.length || tasks.every((x) => x.status === 'running')}
          onClick={() => void window.oneasr.startAll().then(reportStart)}
        >
          {s.startAll}
        </button>
        <button
          className="btn btn-ghost btn-danger-ghost"
          disabled={!tasks.length}
          onClick={() =>
            void window.oneasr.clearTasks().then((next) => {
              setTasks(next)
              // 正在跑的那条不再被一起清掉；说一声，免得以为没生效
              if (next.some((t) => t.status === 'running')) pushToast('info', s.clearedKeptRunning)
            })
          }
        >
          {s.clear}
        </button>
        <div className="grow" />
        <button className="btn btn-ghost" data-shot="guide" onClick={() => setGuideOpen(true)}>
          {s.guide}
        </button>
        <button
          className="btn btn-ghost"
          data-shot="stats"
          onClick={() => {
            void refreshStats()
            setStatsOpen(true)
          }}
        >
          {s.stats}
        </button>
        <button
          className="btn btn-ghost"
          data-shot="settings"
          onClick={() => void openSettingsFn('general')}
        >
          {s.settings}
        </button>
      </div>

      <div
        className={`main ${dragging ? 'dragging' : ''}`}
        onDragOver={(e) => {
          e.preventDefault()
          setDragging(true)
        }}
        onDragLeave={() => setDragging(false)}
        onDrop={onDrop}
      >
        {tasks.length === 0 ? (
          <motion.div
            className={`dropzone ${dragging ? 'active' : ''}`}
            initial={{ opacity: 0.6 }}
            animate={{ opacity: 1 }}
          >
            <div className="icon">
              <span className="audio-glyph" />
            </div>
            <h2>{dragging ? s.dropActive : modelsReady ? s.emptyTitle : s.emptyTitleNotReady}</h2>
            <p>{modelsReady ? s.emptyHint : s.emptyHintNotReady}</p>
            <div className="dropzone-actions">
              <button className="btn btn-primary" disabled={picking} onClick={() => void addFiles()}>
                {s.add}
              </button>
              <button className="btn btn-ghost" onClick={() => void openSettingsFn('models')}>
                {s.openSettings}
              </button>
            </div>
          </motion.div>
        ) : (
          <TaskList
            lang={lang}
            tasks={tasks}
            hotwordGroups={hotwords?.groups ?? []}
            defaultLanguage={settings?.language ?? 'zh'}
            onStart={(id) => void window.oneasr.startTask(id).then(reportStart)}
            onCancel={(id) => void window.oneasr.cancelTask(id)}
            onRemove={(id) => void window.oneasr.removeTask(id).then(setTasks)}
            onOpen={(p) => void window.oneasr.openOutputFolder(p)}
            onSetLanguage={(id, language) =>
              void window.oneasr.setTaskLanguage(id, language).then(setTasks)
            }
            onSetVocalSeparation={(id, on) =>
              void window.oneasr.setTaskVocalSeparation(id, on).then(setTasks)
            }
            onSetHotwordGroup={(id, groupId) =>
              void window.oneasr.setTaskHotwordGroup(id, groupId).then(setTasks)
            }
            onCopyError={(text) =>
              void window.oneasr.copyText(text).then(() => pushToast('success', s.copied))
            }
          />
        )}
      </div>

      <StatusBar
        lang={lang}
        models={models}
        tasks={tasks}
        message={statusMsg}
        onCopyMessage={(text) => void window.oneasr.copyText(text).then(() => pushToast('success', s.copied))}
      />

      <div className="toast-stack">
        <AnimatePresence>
          {toasts.map((toast) => (
            <motion.div
              key={toast.id}
              className={`toast ${toast.kind}`}
              {...toastMotion}
            >
              {toast.message}
            </motion.div>
          ))}
        </AnimatePresence>
      </div>

      <AnimatePresence>
        {welcomeOpen && (
          <WelcomeModal
            lang={lang}
            onGotIt={() => void dismissWelcome(false)}
            onOpenSettings={() => void dismissWelcome(true)}
          />
        )}
      </AnimatePresence>

      <AnimatePresence>
        {settingsOpen && draft && hotwordDraft && (
          <SettingsDrawer
            lang={lang}
            draft={draft}
            models={models}
            modelEntries={modelEntries}
            download={download}
            onDownloadStart={(id) => void window.oneasr.startDownload(id)}
            onDownloadCancel={() => void window.oneasr.cancelDownload()}
            dirty={dirty}
            hotwordDraft={hotwordDraft}
            initialTab={settingsTab}
            onChange={setDraft}
            onHotwordsChange={setHotwordDraft}
            onSave={() => void saveAllSettings()}
            onReset={() => {
              if (settingsBaseline) setDraft(structuredClone(settingsBaseline))
              if (hotwordsBaseline) setHotwordDraft(structuredClone(hotwordsBaseline))
              pushToast('info', s.discardChanges)
            }}
            onRestoreDefaults={() => {
              // 真·恢复默认：写盘（settings:reset），再把草稿与基线对齐
              void window.oneasr.resetSettings().then((saved) => {
                setSettings(saved)
                setSettingsBaseline(structuredClone(saved))
                setDraft(structuredClone(saved))
                void refreshModels()
                pushToast('success', s.restored)
              })
            }}
            onClose={() => {
              if (settings) setDraft(structuredClone(settings))
              if (hotwords) setHotwordDraft(structuredClone(hotwords))
              setSettingsOpen(false)
              setSettingsTab('general')
            }}
            onPickDir={() =>
              void window.oneasr.pickDirectory(draft.output_dir).then((r) => {
                if (!r.ok) {
                  pushToast('error', `${s.dialogFailed}: ${r.error || ''}`)
                  return
                }
                if (r.path) setDraft({ ...draft, output_dir: r.path })
              })
            }
            onPickModel={(field) =>
              void window.oneasr.pickModelDir(draft[field] as string).then((r) => {
                if (!r.ok) {
                  pushToast('error', `${s.dialogFailed}: ${r.error || ''}`)
                  return
                }
                if (r.path) setDraft({ ...draft, [field]: r.path })
              })
            }
            onOpenModels={() => void window.oneasr.openModelsFolder()}
            onRefreshModels={() => void refreshModels()}
          />
        )}
      </AnimatePresence>

      <AnimatePresence>
        {statsOpen && (
          <StatsPanel lang={lang} stats={stats || emptyStats} onClose={() => setStatsOpen(false)} />
        )}
      </AnimatePresence>

      <AnimatePresence>
        {guideOpen && <GuidePanel lang={lang} onClose={() => setGuideOpen(false)} />}
      </AnimatePresence>

    </div>
  )
}

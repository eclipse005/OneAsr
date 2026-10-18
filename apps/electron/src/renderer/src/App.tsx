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
import type {
  HotwordStore,
  DownloadState,
  StatusSnapshot,
  Settings,
  StartResult,
  StatsSnapshot,
  TaskSnapshot,
} from '../../shared/types'

function deepEq(a: unknown, b: unknown): boolean {
  return JSON.stringify(a) === JSON.stringify(b)
}


export default function App() {
  const [settings, setSettings] = useState<Settings | null>(null)
  const [draft, setDraft] = useState<Settings | null>(null)
  /** CLI 的 status --json 快照：就绪、门禁、catalog 全在里面 */
  const [status, setStatus] = useState<StatusSnapshot | null>(null)
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
  /**
   * 提示只有**一条通道**：底部状态栏右侧那枚可点的消息片。
   *
   * 之前是 toast（右下角浮层）+ 状态栏各来一份，同一个「已复制」显示两遍；
   * 而且 toast 浮在列表上方会挡住内容。状态栏在最下方、不挡东西、还支持点击复制，
   * 留它就够了。
   */
  const [statusMsg, setStatusMsg] = useState<{ kind: string; text: string } | null>(null)
  const statusTimer = useRef<number | undefined>(undefined)
  /** 正在开系统文件对话框（防重入） */
  const [picking, setPicking] = useState(false)

  const [download, setDownload] = useState<DownloadState | null>(null)

  const lang = useMemo(
    () => resolveUiLang(draft?.ui_language || settings?.ui_language || 'system'),
    [draft?.ui_language, settings?.ui_language]
  )
  const s = t(lang)
  // 就绪与否 = Rust 的 can_start（壳不再自己判断目录够不够）
  const modelsReady = !!status?.can_start.ok
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

  const notify = useCallback((kind: 'info' | 'success' | 'warn' | 'error', message: string) => {
    setStatusMsg({ kind, text: message })
    if (statusTimer.current) window.clearTimeout(statusTimer.current)
    // 没有 toast 了，报错/警告多留一会儿（8s），普通提示 4s
    statusTimer.current = window.setTimeout(
      () => setStatusMsg(null),
      kind === 'warn' || kind === 'error' ? 8000 : 4000
    )
  }, [])

  const refreshModels = useCallback(async () => {
    setStatus(await window.oneasr.getStatus())
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
    if (downloadPhase === 'done') notify('success', s.downloadDone)
    else notify('error', s.downloadFailed)
    void refreshModels()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [downloadPhase])

  /**
   * 启动被门禁挡下时把原因弹出来。**文案是 Rust 给的**（status --json 的
   * can_start.reason，双语、与 GPUI 逐字一致），壳不再自己拼"模型未就绪/热词组为空"。
   */
  const reportStart = useCallback(
    (r: StartResult) => {
      if (r.ok) return
      notify('warn', r.reason ?? s.blockNothing)
    },
    [s, notify]
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
    notify('success', s.saved)
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
          notify('error', `${s.dialogFailed}: ${r.error || ''}`)
          return
        }
        list = r.paths
      } finally {
        setPicking(false)
      }
      if (!list.length) {
        notify('info', s.noFiles)
        return
      }
    }
    // 拖进来的可能什么都有：只收后端认的媒体，其余报一次数——不静默吞掉
    // （扩展名清单来自 `status --json`，壳不再自己维护正则；状态还没拿到时不拦，
    //   免得刚启动那几百毫秒里把正常文件也当成不支持）
    const exts = status?.media_exts ?? []
    const isSupported = (p: string): boolean => {
      const lower = p.toLowerCase()
      return exts.some((e) => lower.endsWith(`.${e}`))
    }
    const media = exts.length ? list.filter(isSupported) : list
    const skipped = list.length - media.length
    if (skipped > 0) {
      notify(media.length === 0 ? 'warn' : 'info', unsupportedCount(lang, skipped))
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
    else notify('warn', s.noFiles)
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
          <span className="btn-glyph i-plus" aria-hidden />
          {s.add}
        </button>
        <button
          className="btn btn-ghost"
          disabled={!tasks.length || tasks.every((x) => x.status === 'running')}
          onClick={() => void window.oneasr.startAll().then(reportStart)}
        >
          <span className="btn-glyph i-play" aria-hidden />
          {s.startAll}
        </button>
        <button
          className="btn btn-ghost btn-danger-ghost"
          disabled={!tasks.length}
          onClick={() =>
            void window.oneasr.clearTasks().then((next) => {
              setTasks(next)
              // 正在跑的那条不再被一起清掉；说一声，免得以为没生效
              if (next.some((t) => t.status === 'running')) notify('info', s.clearedKeptRunning)
            })
          }
        >
          <span className="btn-glyph i-trash" aria-hidden />
          {s.clear}
        </button>
        <div className="grow" />
        <button className="btn btn-ghost" data-shot="guide" onClick={() => setGuideOpen(true)}>
          <span className="btn-glyph i-help" aria-hidden />
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
          <span className="btn-glyph i-chart" aria-hidden />
          {s.stats}
        </button>
        <button
          className="btn btn-ghost"
          data-shot="settings"
          onClick={() => void openSettingsFn('general')}
        >
          <span className="btn-glyph i-gear" aria-hidden />
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
            onApplyToAll={(id) => void window.oneasr.applyTaskToAll(id).then(setTasks)}
            onSetHotwordGroup={(id, groupId) =>
              void window.oneasr.setTaskHotwordGroup(id, groupId).then(setTasks)
            }
            onCopyError={(text) =>
              void window.oneasr.copyText(text).then(() => notify('success', s.copied))
            }
          />
        )}
      </div>

      <StatusBar
        lang={lang}
        status={status}
        tasks={tasks}
        message={statusMsg}
        onCopyMessage={(text) => void window.oneasr.copyText(text).then(() => notify('success', s.copied))}
      />

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
            status={status}
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
              notify('info', s.discardChanges)
            }}
            onRestoreDefaults={() => {
              // 真·恢复默认：写盘（settings:reset），再把草稿与基线对齐
              void window.oneasr.resetSettings().then((saved) => {
                setSettings(saved)
                setSettingsBaseline(structuredClone(saved))
                setDraft(structuredClone(saved))
                void refreshModels()
                notify('success', s.restored)
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
                  notify('error', `${s.dialogFailed}: ${r.error || ''}`)
                  return
                }
                if (r.path) setDraft({ ...draft, output_dir: r.path })
              })
            }
            onPickModel={(field) =>
              void window.oneasr.pickModelDir(draft[field] as string).then((r) => {
                if (!r.ok) {
                  notify('error', `${s.dialogFailed}: ${r.error || ''}`)
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

import { AnimatePresence, motion } from 'framer-motion'
import { useCallback, useEffect, useMemo, useState } from 'react'
import { GuidePanel } from './components/GuidePanel'
import { HotwordPanel } from './components/HotwordPanel'
import { SettingsDrawer } from './components/SettingsDrawer'
import { StatsPanel } from './components/StatsPanel'
import { StatusBar } from './components/StatusBar'
import { TaskList } from './components/TaskList'
import { TitleBar } from './components/TitleBar'
import { WelcomeModal } from './components/WelcomeModal'
import { resolveUiLang, t } from './i18n'
import type {
  HotwordStore,
  ModelStatus,
  Settings,
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
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [statsOpen, setStatsOpen] = useState(false)
  const [guideOpen, setGuideOpen] = useState(false)
  const [hotwordOpen, setHotwordOpen] = useState(false)
  const [welcomeOpen, setWelcomeOpen] = useState(false)
  const [dragging, setDragging] = useState(false)
  const [toasts, setToasts] = useState<ToastPayload[]>([])

  const lang = useMemo(
    () => resolveUiLang(draft?.ui_language || settings?.ui_language || 'system'),
    [draft?.ui_language, settings?.ui_language]
  )
  const s = t(lang)
  const dirty = !!(settings && draft && !deepEq(settings, draft))

  const pushToast = useCallback((kind: ToastPayload['kind'], message: string) => {
    const id = `toast-${++toastSeq}`
    setToasts((prev) => [...prev, { id, kind, message }])
    setTimeout(() => setToasts((prev) => prev.filter((t) => t.id !== id)), 4200)
  }, [])

  const refreshModels = useCallback(async () => {
    setModels(await window.oneasr.checkModels())
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
    return () => {
      offTasks()
      offStats()
    }
  }, [refreshModels, refreshStats])

  const saveSettings = async () => {
    if (!draft) return
    const saved = await window.oneasr.saveSettings(draft)
    setSettings(saved)
    setDraft(saved)
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
    if (openSettings) setSettingsOpen(true)
  }

  const addFiles = async (paths?: string[]) => {
    let list = paths
    if (!list) {
      const r = await window.oneasr.pickFiles()
      if (!r.ok) {
        pushToast('error', `${s.dialogFailed}: ${r.error || ''}`)
        return
      }
      list = r.paths
      if (!list.length) {
        pushToast('info', s.noFiles)
        return
      }
    }
    if (!list.length) return
    setTasks(await window.oneasr.addTasks(list))
  }

  const onDrop = (e: React.DragEvent) => {
    e.preventDefault()
    setDragging(false)
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
        <button className="btn btn-primary" onClick={() => void addFiles()}>
          {s.add}
        </button>
        <button
          className="btn btn-ghost"
          disabled={!tasks.length || tasks.every((x) => x.status === 'running')}
          onClick={() => void window.oneasr.startAll()}
        >
          {s.startAll}
        </button>
        <button
          className="btn btn-ghost btn-danger-ghost"
          disabled={!tasks.length}
          onClick={() => void window.oneasr.clearTasks().then(setTasks)}
        >
          {s.clear}
        </button>
        <div className="grow" />
        <button className="btn btn-ghost" onClick={() => setGuideOpen(true)}>
          {s.guide}
        </button>
        <button
          className="btn btn-ghost"
          onClick={() => {
            void (async () => {
              setHotwords(await window.oneasr.getHotwords())
              setHotwordOpen(true)
            })()
          }}
        >
          {s.hotwords}
        </button>
        <button
          className="btn btn-ghost"
          onClick={() => {
            void refreshStats()
            setStatsOpen(true)
          }}
        >
          {s.stats}
        </button>
        <button className="btn btn-ghost" onClick={() => setSettingsOpen(true)}>
          {s.settings}
        </button>
      </div>

      <div
        className="main"
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
            <div className="icon">♪</div>
            <h2>{dragging ? s.dropActive : s.emptyTitle}</h2>
            <p>{s.emptyHint}</p>
          </motion.div>
        ) : (
          <TaskList
            lang={lang}
            tasks={tasks}
            onStart={(id) => void window.oneasr.startTask(id)}
            onCancel={(id) => void window.oneasr.cancelTask(id)}
            onRemove={(id) => void window.oneasr.removeTask(id).then(setTasks)}
            onOpen={(p) => void window.oneasr.openOutputFolder(p)}
          />
        )}
      </div>

      <StatusBar lang={lang} models={models} tasks={tasks} />

      <div className="toast-stack">
        <AnimatePresence>
          {toasts.map((toast) => (
            <motion.div
              key={toast.id}
              className={`toast ${toast.kind}`}
              initial={{ opacity: 0, y: 12, x: 8 }}
              animate={{ opacity: 1, y: 0, x: 0 }}
              exit={{ opacity: 0, y: 8 }}
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
        {settingsOpen && draft && (
          <SettingsDrawer
            lang={lang}
            draft={draft}
            models={models}
            dirty={dirty}
            onChange={setDraft}
            onSave={() => void saveSettings()}
            onReset={() =>
              void window.oneasr.resetSettings().then(async (r) => {
                setSettings(r)
                setDraft(r)
                await refreshModels()
                pushToast('info', s.reset)
              })
            }
            onClose={() => {
              setDraft(settings)
              setSettingsOpen(false)
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

      <AnimatePresence>
        {hotwordOpen && hotwords && (
          <HotwordPanel
            lang={lang}
            store={hotwords}
            onClose={() => setHotwordOpen(false)}
            onSave={async (store) => {
              const saved = await window.oneasr.saveHotwords(store)
              setHotwords(saved)
              pushToast('success', s.saved)
            }}
          />
        )}
      </AnimatePresence>
    </div>
  )
}

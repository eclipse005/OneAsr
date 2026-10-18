import { AnimatePresence } from 'framer-motion'
import { useCallback, useEffect, useMemo, useState } from 'react'
import { GuidePanel } from './components/GuidePanel'
import { SettingsDrawer } from './components/SettingsDrawer'
import { StatsPanel } from './components/StatsPanel'
import { StatusBar } from './components/StatusBar'
import { TaskList } from './components/TaskList'
import { TitleBar } from './components/TitleBar'
import { WelcomeModal } from './components/WelcomeModal'
import { resolveUiLang, t } from './i18n'
import type { ModelStatus, Settings, StatsSnapshot, TaskSnapshot } from '../../shared/types'

function deepEq(a: unknown, b: unknown): boolean {
  return JSON.stringify(a) === JSON.stringify(b)
}

export default function App() {
  const [settings, setSettings] = useState<Settings | null>(null)
  const [draft, setDraft] = useState<Settings | null>(null)
  const [models, setModels] = useState<ModelStatus | null>(null)
  const [tasks, setTasks] = useState<TaskSnapshot[]>([])
  const [stats, setStats] = useState<StatsSnapshot>({
    totalRuns: 0,
    successCount: 0,
    errorCount: 0,
    recent: []
  })
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [statsOpen, setStatsOpen] = useState(false)
  const [guideOpen, setGuideOpen] = useState(false)
  const [welcomeOpen, setWelcomeOpen] = useState(false)
  const [dragging, setDragging] = useState(false)

  const lang = useMemo(
    () => resolveUiLang(draft?.ui_language || settings?.ui_language || 'system'),
    [draft?.ui_language, settings?.ui_language]
  )
  const s = t(lang)
  const dirty = !!(settings && draft && !deepEq(settings, draft))

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
      await refreshModels()
      await refreshStats()
    })()
    const offTasks = window.oneasr.onTasksUpdated((next) => {
      setTasks(next)
      void refreshStats()
    })
    return () => offTasks()
  }, [refreshModels, refreshStats])

  const saveSettings = async () => {
    if (!draft) return
    const saved = await window.oneasr.saveSettings(draft)
    setSettings(saved)
    setDraft(saved)
    await refreshModels()
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
    const picked = paths ?? (await window.oneasr.pickFiles())
    if (!picked.length) return
    setTasks(await window.oneasr.addTasks(picked))
  }

  const onDrop = (e: React.DragEvent) => {
    e.preventDefault()
    setDragging(false)
    const files = Array.from(e.dataTransfer.files || [])
    const paths = files
      .map((f) => (f as File & { path?: string }).path)
      .filter((p): p is string => !!p)
    if (paths.length) void addFiles(paths)
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
          disabled={!tasks.length || tasks.every((t) => t.status === 'running')}
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
          <div className={`dropzone ${dragging ? 'active' : ''}`}>
            <h2>{dragging ? s.dropActive : s.emptyTitle}</h2>
            <p>{s.emptyHint}</p>
          </div>
        ) : (
          <TaskList
            lang={lang}
            tasks={tasks}
            onStart={(id) => void window.oneasr.startTask(id)}
            onRemove={(id) => void window.oneasr.removeTask(id).then(setTasks)}
            onOpen={(p) => void window.oneasr.openOutputFolder(p)}
          />
        )}
      </div>

      <StatusBar lang={lang} models={models} />

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
            dirty={dirty}
            onChange={setDraft}
            onSave={() => void saveSettings()}
            onReset={() =>
              void window.oneasr.resetSettings().then(async (r) => {
                setSettings(r)
                setDraft(r)
                await refreshModels()
              })
            }
            onClose={() => {
              setDraft(settings)
              setSettingsOpen(false)
            }}
            onPickDir={() =>
              void window.oneasr.pickDirectory(draft.output_dir).then((p) => {
                if (p) setDraft({ ...draft, output_dir: p })
              })
            }
            onPickModel={(field) =>
              void window.oneasr.pickModelDir(draft[field]).then((p) => {
                if (!p) return
                setDraft({ ...draft, [field]: p })
              })
            }
          />
        )}
      </AnimatePresence>

      <AnimatePresence>
        {statsOpen && <StatsPanel lang={lang} stats={stats} onClose={() => setStatsOpen(false)} />}
      </AnimatePresence>

      <AnimatePresence>
        {guideOpen && <GuidePanel lang={lang} onClose={() => setGuideOpen(false)} />}
      </AnimatePresence>
    </div>
  )
}

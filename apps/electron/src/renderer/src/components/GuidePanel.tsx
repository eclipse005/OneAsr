import { useState } from 'react'
import { AnimatePresence, motion } from 'framer-motion'
import { t, UiLang } from '../i18n'
import { usePaneMotion } from '../motion'
import { ModalShell } from './ui'

type GuideTab = 'use' | 'transcript' | 'general' | 'output' | 'models' | 'hotwords' | 'compute'
type Copy = ReturnType<typeof t>

function Entry({ title, body }: { title: string; body: string }) {
  return (
    <section className="guide-entry">
      <h4>{title}</h4>
      <p>{body}</p>
    </section>
  )
}

function UseTab({ s }: { s: Copy }) {
  return (
    <div className="guide-use">
      <section className="guide-scene">
        <h4>{s.guideReadyLead}</h4>
        <div className="guide-ready">
          <span className="dot ok" />
          <span>{s.modelsReady}</span>
        </div>
        <p>{s.guideReadyHint}</p>
      </section>

      <section className="guide-scene">
        <h4>{s.guideDropLead}</h4>
        <div className="guide-drop-row">
          <div className="guide-drop">{s.guideDropZone}</div>
          <span className="btn btn-primary">
            <span className="btn-glyph i-plus" aria-hidden />
            {s.add}
          </span>
        </div>
        <p>{s.guideDropHint}</p>
      </section>

      <section className="guide-scene">
        <h4>{s.guideRowLead}</h4>
        <div className="task-card status-queued guide-demo" aria-hidden="true">
          <div className="card-info">
            <div className="name">
              <span className="media-icon video" />
              <span className="task-name">{s.guideSampleName}</span>
            </div>
            {/* 状态跟真实行一样排在文件名下面这一行（右列不再有状态槽） */}
            <div className="meta-line">
              <span className="badge queued">
                <span>{s.pending}</span>
              </span>
            </div>
          </div>
          <div className="controls">
            <div className="row-chips">
              <span className="row-chip lang-chip">
                <span className="ellipsis">{s.guideSampleLang}</span>
                <span className="caret">▾</span>
              </span>
              <span className="row-chip group-chip">
                <span className="ellipsis">{s.hotwords}</span>
                <span className="caret">▾</span>
              </span>
              <span className="row-chip sep-chip">
                <span className="sep-mark" />
                {s.vocalSep}
              </span>
              <span className="row-chip transcript-chip has">
                <span className="transcript-glyph" />
                <span className="ellipsis">{s.transcript}</span>
              </span>
            </div>
            <span className="actions">
              <span className="icon-btn primary">
                <span className="icon-glyph i-play" />
              </span>
              <span className="icon-btn">
                <span className="icon-glyph i-folder" />
              </span>
              <span className="icon-btn">
                <span className="icon-glyph i-trash" />
              </span>
            </span>
          </div>
        </div>
        <div className="guide-legend">
          <p>
            <b>
              {s.recognitionLanguage} / {s.hotwords}
            </b>
            {s.guideLangHint}
          </p>
          <p>
            <b>{s.vocalSep}</b>
            {s.guideSepHint}
          </p>
          <p>
            <b>{s.transcript}</b>
            {s.guideTranscriptHint}
          </p>
          <p>
            <b>{s.start}</b>
            {s.guideStartHint}
          </p>
          <p>
            <b>{s.openOutput}</b>
            {s.guideOutputHint}
          </p>
          <p>
            <b>{s.remove}</b>
            {s.guideRemoveHint}
          </p>
        </div>
      </section>
    </div>
  )
}

function TabBody({ tab, s }: { tab: GuideTab; s: Copy }) {
  if (tab === 'use') return <UseTab s={s} />
  if (tab === 'transcript') {
    return (
      <div className="guide-copy">
        <Entry title={s.transcript} body={s.guideTranscriptPair} />
        <Entry title={s.transcriptPanelTitle} body={s.guideTranscriptPanel} />
        <Entry title={s.transcriptSave} body={s.guideTranscriptEdit} />
        <Entry title={s.transcriptSmartSplit} body={s.guideTranscriptSplit} />
        <Entry title={s.transcriptStart} body={s.guideTranscriptRealign} />
        <Entry title={s.outputs} body={s.guideTranscriptTimed} />
      </div>
    )
  }
  if (tab === 'general') {
    return (
      <div className="guide-copy">
        <Entry title={s.uiLanguage} body={s.guideUiLanguage} />
        <Entry title={s.recognitionLanguage} body={s.guideLanguage} />
        <Entry title={s.subtitleLength} body={s.guideSubtitleLength} />
      </div>
    )
  }
  if (tab === 'output') {
    return (
      <div className="guide-copy">
        <Entry title={s.outputs} body={s.guideOutputs} />
        <Entry title={s.chineseScript} body={s.guideScript} />
        <Entry title={s.subtitlePolish} body={s.guidePolish} />
        <Entry title={s.saveNextToSource} body={s.guideSaveNext} />
        <Entry title={s.outputPath} body={s.guideOutputDir} />
      </div>
    )
  }
  if (tab === 'models') {
    return (
      <div className="guide-copy">
        <Entry title={s.guideModelPathsTitle} body={s.guideModelPaths} />
        <Entry title={s.asrSize} body={s.guideAsr} />
        <Entry title={s.quantization} body={s.guideQuant} />
        <Entry title={s.asrPath} body={s.guideAsrPath} />
        <Entry title={s.alignerKind} body={s.guideAligner} />
        <Entry title={s.vocalSep} body={s.guideVocal} />
        <Entry title={s.demucsPath} body={s.guideDemucs} />
        <Entry title={s.sensevoicePath} body={s.guideSense} />
        <Entry title={s.downloadSection} body={s.guideDownload} />
      </div>
    )
  }
  if (tab === 'hotwords') {
    return (
      <div className="guide-copy">
        <Entry title={s.guideHwInputTitle} body={s.guideHwInput} />
        <Entry title={s.groups} body={s.guideHwGroups} />
        <Entry title={s.defaultGroupShort} body={s.guideHwDefault} />
        <Entry title={s.hotwords} body={s.guideHwRow} />
      </div>
    )
  }
  return (
    <div className="guide-copy">
      <Entry title={s.computeBackend} body={s.guideBackend} />
      <Entry title={s.sounds} body={s.guideSounds} />
      <Entry title={s.closeToTray} body={s.guideTray} />
      <Entry title={s.diagnostics} body={s.guideLog} />
    </div>
  )
}

export function GuidePanel({ lang, onClose }: { lang: UiLang; onClose: () => void }) {
  const s = t(lang)
  const pane = usePaneMotion()
  const [tab, setTab] = useState<GuideTab>('use')

  const tabs: { id: GuideTab; label: string }[] = [
    { id: 'use', label: s.guideTabUse },
    { id: 'transcript', label: s.guideTabTranscript },
    { id: 'general', label: s.sectionGeneral },
    { id: 'output', label: s.sectionOutput },
    { id: 'models', label: s.sectionModels },
    { id: 'hotwords', label: s.sectionHotwords },
    { id: 'compute', label: s.sectionCompute }
  ]

  return (
    <ModalShell
      title={s.guideTitle}
      onClose={onClose}
      closeLabel={s.close}
      shellClassName="guide-shell"
      closeDataShot="close-modal"
    >
        <div className="settings-body">
          <nav className="settings-nav" aria-label={s.guideTitle}>
            {tabs.map((item) => (
              <button
                key={item.id}
                type="button"
                className={`settings-nav-item ${tab === item.id ? 'active' : ''}`}
                aria-current={tab === item.id ? 'page' : undefined}
                onClick={() => setTab(item.id)}
              >
                {item.label}
              </button>
            ))}
          </nav>
          <div className="settings-pane">
            <AnimatePresence mode="wait" initial={false}>
              <motion.div key={tab} className="settings-pane-inner guide-pane" {...pane}>
                <TabBody tab={tab} s={s} />
              </motion.div>
            </AnimatePresence>
          </div>
        </div>
    </ModalShell>
  )
}

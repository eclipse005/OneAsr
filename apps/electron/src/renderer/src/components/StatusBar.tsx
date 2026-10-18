import { t, UiLang } from '../i18n'

import type { StatusSnapshot, TaskSnapshot } from '../../../shared/types'

export function StatusBar({
  lang,
  status,
  tasks,
  message,
  onCopyMessage,
  onOpenModels
}: {
  lang: UiLang
  /** CLI 的 status --json 快照：就绪、门禁、后端全在里面，壳只显示 */
  status: StatusSnapshot | null
  tasks: TaskSnapshot[]
  /** 点「模型就绪 / 未就绪」直接进设置 → 模型（用户要修模型时不必自己找） */
  onOpenModels?: () => void
  /** 最近一条提示：toast 会消失，这里留 3~6 秒并且可点击复制（长路径/报错不至于看了就丢） */
  message?: { kind: string; text: string } | null
  onCopyMessage?: (text: string) => void
}) {
  const s = t(lang)
  // 就绪与否 = Rust 的 can_start（含「只导 TXT 就不要求对齐」这类细则），失败时 title 给原因
  const ready = !!status?.can_start.ok
  const notReadyReason = status?.can_start.reason ?? undefined
  const running = tasks.filter((x) => x.status === 'running').length
  // 「排队」只算真正进了运行队列的（有 queueRank）；刚添加还没点开始的是「待处理」，行内有徽章
  const queued = tasks.filter((x) => x.status === 'queued' && x.queueRank != null).length
  const done = tasks.filter((x) => ['done', 'error', 'cancelled'].includes(x.status)).length
  return (
    <div className="statusbar">
      <span className={`dot ${ready ? 'ok' : ''}`} />
      <button
        type="button"
        className="status-link"
        style={{ color: ready ? 'var(--logo)' : 'var(--warn)' }}
        title={notReadyReason ?? s.modelsReady}
        onClick={onOpenModels}
      >
        {ready ? s.modelsReady : s.modelsNotReady}
      </button>
      <span className="sep" />
      {/* 显示模型**短名**（catalog id）而不是目录名：完整路径放 title ——
          Qwen3-ASR-0.6B-hf 这种目录名对普通用户太重，也容易把状态栏挤到折行 */}
      <span title={status?.models.asr.dir}>
        {s.asr}: {status?.models.asr.ready ? '✓' : '✗'} {status?.models.asr.id ?? ''}
      </span>
      <span className="sep" />
      <span title={status?.models.aligner.dir}>
        {s.aligner}: {status?.models.aligner.ready ? '✓' : '✗'} {status?.models.aligner.id ?? ''}
      </span>
      <span className="sep" />
      <span title={status?.models.demucs.dir}>
        {s.demucs}: {status?.models.demucs.ready ? '✓' : '—'}
      </span>
      <span className="sep" />
      <span title={status?.models.sensevoice.dir}>
        {s.sensevoice}: {status?.models.sensevoice.ready ? '✓' : '—'}
      </span>
      <span className="sep" />
      <span>
        {s.backend}: {status?.backend || '—'}
      </span>
      {(running > 0 || queued > 0) && (
        <>
          <span className="sep" />
          <span>
            {s.progress} {Math.min(done + 1, tasks.length)}/{tasks.length} · {s.running} {running} ·{' '}
            {s.queued} {queued}
          </span>
        </>
      )}
      {/* 提示贴着右下角：左侧是"环境信息"，右侧是"最近发生的事" */}
      <span className="grow" />
      {message && (
        <>
          <span className="sep" />
          <button
            type="button"
            className={`status-chip ${message.kind}`}
            title={message.text}
            onClick={() => onCopyMessage?.(message.text)}
          >
            {message.text}
          </button>
        </>
      )}
    </div>
  )
}

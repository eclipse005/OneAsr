//! Batch task model for the list rows.
//!
//! Lives in the GUI crate: `Task` / `TaskStatus` / `DurationState` describe the
//! batch list, and nothing in `oneasr-core` consumes them. `TaskTiming` stays
//! in core (it is produced by the pipeline).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use oneasr_core::{TaskTiming, normalize_source_language};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    /// 待处理（未加入运行队列）
    Pending,
    /// 已点开始，等待轮到（同一时间只跑一个）
    Queued,
    /// 处理中
    Processing,
    /// 完成
    Done,
    /// 错误
    Error,
}

impl TaskStatus {
    /// Only the active ASR job locks start/delete.
    pub fn locks_row_actions(self) -> bool {
        matches!(self, Self::Processing)
    }

    /// 文稿在进入队列后已成为本次运行输入，运行中也不可修改。
    pub fn locks_transcript_actions(self) -> bool {
        matches!(self, Self::Queued | Self::Processing)
    }
}

/// Duration probe lifecycle for list display.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DurationState {
    /// Background probe in flight.
    Probing,
    /// Known length in seconds.
    Known(f64),
    /// Probe finished but duration unavailable.
    Unknown,
}

impl DurationState {
    pub fn label(self) -> String {
        match self {
            Self::Probing => "…".into(),
            Self::Known(s) if s.is_finite() && s >= 0.0 => format_duration(s),
            Self::Known(_) | Self::Unknown => "—".into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Task {
    pub id: String,
    pub path: PathBuf,
    pub name: String,
    pub size_bytes: u64,
    pub duration: DurationState,
    /// Extension / container, e.g. `wav`, `mp4`.
    pub format: String,
    /// Source language short id (`zh`, `en`, …). Per-task; defaults from settings.
    pub language: String,
    /// Run HTDemucs vocal separation for this task; defaults from settings,
    /// overridable per row (same model as [`Self::language`]).
    pub vocal_separation: bool,
    pub status: TaskStatus,
    pub error: Option<String>,
    /// FIFO order when [`TaskStatus::Queued`] (lower runs first).
    pub queue_seq: Option<u64>,
    /// Primary deliverable written by a finished run (`.srt`, or `.txt` when
    /// SRT output is switched off) — what the row's “打开” action reveals.
    pub output_file: Option<PathBuf>,
    /// Per-stage processing wall time (set when a run finishes or fails mid-way).
    pub timing: Option<TaskTiming>,
    /// 这条任务挂着的文稿。挂了就意味着「这次不识别」——所以它不是任务的模式
    /// 开关，是任务的**另一个输入**。行上那个芯片就是它的全部呈现。
    pub transcript: Option<StagedTranscript>,
}

/// 一份挂在任务上的文稿。
///
/// **程序永不写回用户的原文件。** `text` 是从文件解析出来的一份暂存：只有
/// 「智能断句」会改它（且只改换行），其余时候它和文件里的内容一致——所以
/// 「恢复原始内容」不是撤销，是重新读一遍。
///
/// 这里**没有**「文稿第几版 / 字幕是哪一版打出来的」这类计数：这不是文稿编辑器，
/// 没有多轮改写要记账，进度由状态胶囊的 `打轴中 8/8` 说完就够了。
#[derive(Debug, Clone)]
pub struct StagedTranscript {
    /// 规范化后的纯文本，**一行 = 一条字幕**。
    ///
    /// 私有是有意的：改写只能走 [`Self::set_text`]。任务列表每帧重绘一次、每行都要
    /// 重问一次行数/字数/语速，这三个数由 [`Self::stats`] 一次性算好缓存着；留一个
    /// 公开字段就等于留了一条「改了正文、缓存不重算」的路，而那条路不报错，只是
    /// 让整列任务一直显示旧的字数。
    text: String,
    /// 由 `text` 派生的缓存，见 [`Self::set_text`]。
    stats: oneasr_core::transcript::TextStats,
    /// 原始文件路径。文稿来自粘贴时为 `None`。
    pub path: Option<PathBuf>,
    /// 原时间轴被丢弃了没有——挂 `.srt` 时为真，界面上要说明一次。这是程序
    /// 唯一一次对「你文件的内容」动了手脚，所以那句话说一次就够。
    pub dropped_timecodes: bool,
}

impl StagedTranscript {
    pub fn from_core(t: &oneasr_core::transcript::Transcript, path: Option<PathBuf>) -> Self {
        let text = t.text.clone();
        let stats = oneasr_core::transcript::text_stats(&text);
        Self {
            text,
            stats,
            path,
            dropped_timecodes: t.dropped_timecodes(),
        }
    }

    /// 正文。只读——要改走 [`Self::set_text`]，那条路会顺带重算缓存。
    pub fn text(&self) -> &str {
        &self.text
    }

    /// 换掉整份文稿，并把行数/字数/语速重算一遍。
    ///
    /// 唯一在用它是「智能断句」：那份 DP 要在后台线程上跑，回来时正文可能已经被
    /// 换过（重新读取、换了文稿），所以换文本的时机由调用方决定，缓存在这里保证
    /// 永远跟着正文走。
    pub fn set_text(&mut self, text: String) {
        self.stats = oneasr_core::transcript::text_stats(&text);
        self.text = text;
    }

    /// 文字量（不含空白）——卡片上的字数与密度校验共用。
    pub fn char_count(&self) -> usize {
        self.stats.chars
    }

    pub fn line_count(&self) -> usize {
        self.stats.lines
    }

    /// 语速：按文稿自己的书写系统计数（字/秒 或 词/秒）。
    ///
    /// 留着这个数是因为它守的是一种**不会报错**的错：文稿挂错文件时，强制对齐
    /// 照样给出一条看着正常、实则慢慢漂移的时间轴。判断在 core 里（那里能单测）。
    pub fn speech_rate(&self, audio_seconds: f64) -> Option<oneasr_core::transcript::SpeechRate> {
        self.stats.rate(audio_seconds)
    }
}

impl Task {
    /// Build a task; `language` is normalized to a catalog short id (e.g. `zh`).
    /// `vocal_separation` is the settings default captured when the row is added.
    pub fn from_path(
        path: impl Into<PathBuf>,
        language: impl Into<String>,
        vocal_separation: bool,
    ) -> Self {
        let path = path.into();
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let size_bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let format = path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_else(|| "—".into());
        Self {
            id: new_id(),
            path,
            name,
            size_bytes,
            duration: DurationState::Probing,
            format,
            language: normalize_source_language(&language.into()),
            vocal_separation,
            status: TaskStatus::Pending,
            error: None,
            queue_seq: None,
            output_file: None,
            timing: None,
            transcript: None,
        }
    }

    /// Set source language (normalized to a known catalog id).
    pub fn set_language(&mut self, language: impl AsRef<str>) {
        self.language = normalize_source_language(language.as_ref());
    }

    /// Toggle per-task vocal separation.
    pub fn set_vocal_separation(&mut self, enabled: bool) {
        self.vocal_separation = enabled;
    }

    pub fn size_label(&self) -> String {
        format_bytes(self.size_bytes)
    }

    pub fn set_duration(&mut self, sec: Option<f64>) {
        self.duration = match sec {
            Some(s) if s.is_finite() && s >= 0.0 => DurationState::Known(s),
            _ => DurationState::Unknown,
        };
    }
}

fn new_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    format!("t{n}")
}

/// Monotonic queue sequence for FIFO start order.
pub fn next_queue_seq() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

fn format_bytes(n: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let x = n as f64;
    if x >= GB {
        format!("{:.2} GB", x / GB)
    } else if x >= MB {
        format!("{:.2} MB", x / MB)
    } else if x >= KB {
        format!("{:.1} KB", x / KB)
    } else {
        format!("{n} B")
    }
}

fn format_duration(sec: f64) -> String {
    let s = sec.max(0.0).round() as u64;
    let h = s / 3600;
    let m = (s % 3600) / 60;
    let r = s % 60;
    if h > 0 {
        format!("{h}:{m:02}:{r:02}")
    } else {
        format!("{m}:{r:02}")
    }
}

fn is_media_path(path: &Path) -> bool {
    const EXT: &[&str] = &[
        "wav", "mp3", "m4a", "aac", "flac", "ogg", "opus", "wma", "mp4", "mkv", "mov", "webm",
        "avi", "flv", "ts", "m4v", "mpeg", "mpg", "wmv", "3gp",
    ];
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| EXT.iter().any(|x| e.eq_ignore_ascii_case(x)))
        .unwrap_or(false)
}

/// Accept existing **media** files only (rejects `.srt` and other non-media).
pub fn accept_input_path(path: &Path) -> bool {
    if !is_media_path(path) {
        return false;
    }
    if path.is_file() {
        return true;
    }
    std::fs::metadata(path)
        .map(|m| m.is_file())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn rejects_srt_and_non_media() {
        assert!(!is_media_path(Path::new("a.srt")));
        assert!(!is_media_path(Path::new("notes.txt")));
        assert!(!accept_input_path(Path::new("a.srt")));
        assert!(is_media_path(Path::new("clip.WAV")));
        assert!(is_media_path(Path::new("v.mp4")));
    }

    #[test]
    fn duration_labels() {
        assert_eq!(DurationState::Probing.label(), "…");
        assert_eq!(DurationState::Unknown.label(), "—");
        assert_eq!(DurationState::Known(90.0).label(), "1:30");
    }

    #[test]
    fn transcript_actions_lock_once_task_is_queued() {
        assert!(!TaskStatus::Pending.locks_transcript_actions());
        assert!(TaskStatus::Queued.locks_transcript_actions());
        assert!(TaskStatus::Processing.locks_transcript_actions());
        assert!(!TaskStatus::Done.locks_transcript_actions());
        assert!(!TaskStatus::Error.locks_transcript_actions());
    }

    #[test]
    fn task_ids_are_unique() {
        let a = Task::from_path("a.wav", "zh", false);
        let b = Task::from_path("b.wav", "en", true);
        let c = Task::from_path("c.wav", "ja", false);
        assert_ne!(a.id, b.id);
        assert_ne!(b.id, c.id);
        assert_ne!(a.id, c.id);
        assert_eq!(a.language, "zh");
        assert_eq!(b.language, "en");
        assert!(!a.vocal_separation);
        assert!(b.vocal_separation);
    }
}

#[cfg(test)]
mod transcript_tests {
    use super::*;
    use oneasr_core::transcript::{TranscriptKind, parse_transcript};

    fn staged(text: &str) -> StagedTranscript {
        let t = parse_transcript(TranscriptKind::Plain, text).expect("transcript");
        StagedTranscript::from_core(&t, None)
    }

    #[test]
    fn counts_match_what_the_card_shows() {
        let st = staged("第一行。\n第二行。");
        assert_eq!(st.char_count(), 8, "第一行。第二行。");
        assert_eq!(st.line_count(), 2);
    }

    /// `set_text` 必须把缓存一起换掉。
    ///
    /// 这三个数是**缓存**而不是每次现算的（任务列表每帧每行都要问一次），所以唯一
    /// 的失效点就是 `set_text`。漏了它，正文已经是断过句的、卡片上的行数字数却还是
    /// 旧的——不报错，只是永远对不上。这条测试就是钉住那个时刻。
    #[test]
    fn set_text_refreshes_the_cached_counts() {
        let mut st = staged("第一行。\n第二行。");
        assert_eq!((st.line_count(), st.char_count()), (2, 8));

        st.set_text("第一行。\n第二行。\n第三行。\n".to_string());
        assert_eq!(st.line_count(), 3, "末尾换行不算新行");
        assert_eq!(st.char_count(), 12);
        assert_eq!(st.text(), "第一行。\n第二行。\n第三行。\n");

        // 换成以拉丁为主：语速的计数单位随之从「字」变成「词」。
        st.set_text("one two three".to_string());
        assert_eq!(st.line_count(), 1);
        assert_eq!(st.char_count(), 11, "空格不算文字量");
        let rate = st.speech_rate(2.0).expect("rate");
        assert!((rate.per_second - 1.5).abs() < 1e-9, "3 词 / 2 秒");
        assert!(rate.spaced, "拉丁书写系统按词计");
    }

    /// 缓存出来的语速与 core 现算的逐项一致。
    ///
    /// `StagedTranscript` 现在只是 core `text_stats` 的一层壳，两条路径必须给出同
    /// 一个答案——否则界面上「文稿太密/太稀」的提示会跟密度校验里的数打架。
    #[test]
    fn cached_rate_matches_the_core_function() {
        for text in [
            "你好世界，这是一段中文。",
            "hello there world",
            "混合 mixed 文本 text",
        ] {
            for secs in [0.5_f64, 12.0, 3600.0] {
                let st = staged(text);
                assert_eq!(
                    st.speech_rate(secs),
                    oneasr_core::transcript::speech_rate(text, secs),
                    "text={text:?} secs={secs}"
                );
            }
        }
    }

    /// 挂 SRT 时原时间轴被丢了，界面上要说明一次。
    #[test]
    fn an_srt_records_that_its_timings_were_dropped() {
        let t = parse_transcript(
            TranscriptKind::SubRip,
            "1\n00:00:01,000 --> 00:00:03,000\n你好。\n",
        )
        .expect("srt");
        let st = StagedTranscript::from_core(&t, None);
        assert!(st.dropped_timecodes);
        assert_eq!(st.text(), "你好。", "只剩正文，时间轴没了");
        assert!(!staged("你好。").dropped_timecodes);
    }

    #[test]
    fn an_unknown_length_gives_no_rate_rather_than_a_zero_one() {
        // 0.0 字/秒 会被读成「文稿是空的」；没有时长就是**没有分母**，不显示。
        assert_eq!(staged("第一行。").speech_rate(0.0), None);
        let r = staged("字字字字")
            .speech_rate(2.0)
            .expect("4 chars over 2 s");
        assert!((r.per_second - 2.0).abs() < 1e-9, "{r:?}");
        assert!(!r.spaced, "中文按字算");
    }

    /// 英文文稿按词数——这就是那次误报的根：按字母数算，正常语速会掉进中文的
    /// 上限之外，卡片于是红着脸说「可能与音频不匹配」。
    #[test]
    fn an_english_transcript_reports_words_not_letters() {
        let st = staged("hello there my friend how are you doing tonight");
        let r = st.speech_rate(4.0).expect("9 words over 4 s");
        assert!(r.spaced, "latin must be counted per word");
        assert!((r.per_second - 9.0 / 4.0).abs() < 1e-9, "{r:?}");
        assert!(r.plausible(), "2.25 words/s is normal English");
    }
}

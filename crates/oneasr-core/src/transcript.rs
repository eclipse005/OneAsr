//! 文稿读取与规范化。
//!
//! 文稿是这个功能的**唯一文字来源**，所以读进来这一段要处理干净三件事，
//! 剩下的都不是这一层该管的：
//!
//! 1. **BOM / CRLF**。`\u{feff}你好` 会让第一行凭空多一个字符（对齐器认不出来，
//!    会被当成词表外字符插值），CRLF 不处理则整篇变成一行、一个 cue 都切不出来。
//! 2. **SRT 的序号与时间轴**。它们不是文稿，剥掉；时间轴整个丢掉——对齐会从零
//!    重新给，这才是这个功能的意义。
//! 3. **一个 cue 的多行合成一行**。SRT 里的换行是**排版产物**（那个播放器、那个
//!    字号、那个边距算出来的），不是编辑决定。冻进文稿等于把一个字体相关的决定
//!    变成不可改的意图。合成用 `join_words`，和我们渲染 cue 时是同一条规则。

use std::path::Path;

use crate::sentence_boundary::join_words;

/// 文稿是什么格式。界面不区分它——两种读进来之后都是同一坨纯文本——但保下来是
/// 为了「这个文稿原来的时间轴被忽略了」这句话能说准。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptKind {
    /// `.txt` / `.md`：行就是行。
    Plain,
    /// `.srt`：序号与时间轴已剥除，每个 cue 合成一行。
    SubRip,
}

impl TranscriptKind {
    pub fn from_path(path: &Path) -> Option<Self> {
        match path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("txt" | "md" | "markdown") => Some(Self::Plain),
            Some("srt") => Some(Self::SubRip),
            _ => None,
        }
    }
}

/// 一份读完的文稿。
#[derive(Debug, Clone)]
pub struct Transcript {
    /// 规范化后的纯文本。**一行 = 一条字幕**，这是整个功能的契约。
    pub text: String,
    pub kind: TranscriptKind,
    /// 读进来的原始内容，一字未动。文稿界面上的「恢复原始内容」用它。
    pub original: String,
}

impl Transcript {
    /// 文字量（不含空白）——密度校验与字数展示用。
    pub fn char_count(&self) -> usize {
        self.text.chars().filter(|c| !c.is_whitespace()).count()
    }

    /// 字幕行数。
    pub fn line_count(&self) -> usize {
        self.text.lines().filter(|l| !l.trim().is_empty()).count()
    }

    /// 原时间轴被丢弃了没有——界面上一次性提示用的就是这句。
    pub fn dropped_timecodes(&self) -> bool {
        self.kind == TranscriptKind::SubRip
    }

    /// 密度：每秒多少字。文稿与音频对不上时，强制对齐不会报错，它给出一个看起来
    /// 正常但慢慢漂移的时间轴——所以这个数要在**点开始之前**给用户看。
    pub fn chars_per_second(&self, audio_seconds: f64) -> f64 {
        if audio_seconds <= 0.0 {
            return 0.0;
        }
        self.char_count() as f64 / audio_seconds
    }
}

/// 读一份文稿。扩展名不认识就报错，不猜。
pub fn read_transcript(path: &Path) -> Result<Transcript, String> {
    let kind = TranscriptKind::from_path(path)
        .ok_or_else(|| format!("不是支持的文稿格式: {}", path.display()))?;
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse_transcript(kind, &String::from_utf8_lossy(&bytes))
}

/// 按格式解析已经读成字符串的内容。
pub fn parse_transcript(kind: TranscriptKind, raw: &str) -> Result<Transcript, String> {
    let text = match kind {
        TranscriptKind::Plain => normalize_lines(raw),
        TranscriptKind::SubRip => normalize_lines(&srt_to_lines(raw)?),
    };
    if text.trim().is_empty() {
        return Err("文稿是空的".to_string());
    }
    Ok(Transcript {
        text,
        kind,
        original: raw.to_string(),
    })
}

/// BOM 去掉、CRLF 变 LF、行尾空白去掉、首尾空行去掉。**不碰行内的空格**——
/// 用户自己排的字距是他写的，我们只负责不让编码问题混进去。
fn normalize_lines(raw: &str) -> String {
    let body = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let body = body.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines: Vec<&str> = body.split('\n').map(str::trim_end).collect();
    while lines.first().is_some_and(|l| l.is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}

/// SRT → 纯文本，一行一个 cue。
///
/// 按空行分块，逐块判断哪几行是正文：时间轴行（带 `-->`）在第二行时正文从第三行
/// 开始；正文自己就是一个数字时（「2025」当字幕）不能被当成序号吃掉，所以**序号
/// 只在「它是块首且块里还有别的行」时丢弃**。
fn srt_to_lines(raw: &str) -> Result<String, String> {
    let body = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let body = body.replace("\r\n", "\n").replace('\r', "\n");
    let mut out: Vec<String> = Vec::new();
    for block in body.split("\n\n") {
        let lines: Vec<&str> = block
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect();
        if lines.is_empty() {
            continue;
        }
        let text_lines: &[&str] = if lines.iter().any(|l| l.contains("-->")) {
            // 有时间轴：它之前是序号，它之后全是正文。
            let at = lines.iter().position(|l| l.contains("-->")).unwrap_or(0);
            &lines[at + 1..]
        } else if lines.len() > 1 && lines[0].chars().all(|c| c.is_ascii_digit()) {
            &lines[1..]
        } else {
            &lines[..]
        };
        let text = join_words(text_lines.iter().copied());
        if !text.trim().is_empty() {
            out.push(text);
        }
    }
    if out.is_empty() {
        return Err("SRT 里没有可用的字幕行".to_string());
    }
    Ok(out.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRT: &str = "1\r\n00:00:04,240 --> 00:00:07,440\r\n原版 Whisper，\r\n表现并不像可靠。\r\n\r\n2\r\n00:00:07,440 --> 00:00:09,680\r\n接下来是第二句。\r\n";

    #[test]
    fn bom_and_crlf_are_cleaned_but_inner_spacing_is_kept() {
        let t = parse_transcript(TranscriptKind::Plain, "\u{feff}你  好\r\n第二行\r\n").unwrap();
        assert_eq!(t.text, "你  好\n第二行");
    }

    #[test]
    fn a_plain_file_keeps_its_line_breaks() {
        let t = parse_transcript(TranscriptKind::Plain, "第一行。\n第二行。\n\n第四行。").unwrap();
        assert_eq!(t.text, "第一行。\n第二行。\n\n第四行。");
        assert_eq!(t.line_count(), 3);
        assert!(!t.dropped_timecodes());
    }

    /// SRT 的一个 cue 变成一行：多行按 `join_words` 合，序号与时间轴丢掉。
    #[test]
    fn an_srt_cue_becomes_one_line() {
        let t = parse_transcript(TranscriptKind::SubRip, SRT).unwrap();
        // `原版` 与 `Whisper` 之间的空格来自 SRT 那一行的内部排版，原样留着；
        // 两行之间补的那个空格是 `join_words` 加的——上一段以带空格的词收尾。
        // 这和渲染 cue 时是同一条规则，所以编出来的这一行正是播放器会画的样子。
        assert_eq!(t.text, "原版 Whisper，表现并不像可靠。\n接下来是第二句。");
        assert!(t.dropped_timecodes());
    }

    /// 正文本身是数字时不能被当成序号吃掉。
    #[test]
    fn a_numeric_cue_survives() {
        let srt =
            "1\n00:00:01,000 --> 00:00:03,000\n2025\n\n2\n00:00:03,000 --> 00:00:05,000\n1500人\n";
        let t = parse_transcript(TranscriptKind::SubRip, srt).unwrap();
        assert_eq!(t.text, "2025\n1500人");
    }

    /// 拉丁文按词合行，空格由共享的 `join_words` 决定，前后一致。
    #[test]
    fn latin_cue_lines_join_with_the_shared_rule() {
        let srt = "1\n00:00:01,000 --> 00:00:03,000\nHello\nworld\n";
        let t = parse_transcript(TranscriptKind::SubRip, srt).unwrap();
        assert_eq!(t.text, "Hello world");
    }

    #[test]
    fn counts_and_density_are_reported_for_the_guard() {
        let t = parse_transcript(TranscriptKind::Plain, "这是第一句。\n这是第二句。").unwrap();
        assert_eq!(t.char_count(), 12);
        assert_eq!(t.line_count(), 2);
        assert!((t.chars_per_second(6.0) - 2.0).abs() < 1e-9);
        assert_eq!(t.chars_per_second(0.0), 0.0, "音频时长未知时不编造密度");
    }

    #[test]
    fn an_empty_transcript_is_refused_rather_than_made_into_a_blank_cue() {
        assert!(parse_transcript(TranscriptKind::Plain, "  \n\n ").is_err());
        assert!(parse_transcript(TranscriptKind::SubRip, "1\n00:00 --> 00:01\n\n").is_err());
    }

    #[test]
    fn the_original_is_kept_byte_for_byte() {
        let t = parse_transcript(TranscriptKind::SubRip, SRT).unwrap();
        assert_eq!(t.original, SRT, "「恢复原始内容」要能回到一个字不差");
    }

    #[test]
    fn only_supported_extensions_are_transcripts() {
        assert_eq!(
            TranscriptKind::from_path(Path::new("a.txt")),
            Some(TranscriptKind::Plain)
        );
        assert_eq!(
            TranscriptKind::from_path(Path::new("a.SRT")),
            Some(TranscriptKind::SubRip)
        );
        assert_eq!(TranscriptKind::from_path(Path::new("a.mp4")), None);
    }
}

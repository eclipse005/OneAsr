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

    /// 语速：每秒多少「单位」，单位由文稿自己的书写系统决定（见 [`speech_rate`]）。
    ///
    /// 留着这个数是因为它守的是一种**不会报错**的错：文稿挂错文件时，强制对齐照样
    /// 给出一条看着正常、实则慢慢漂移的时间轴。
    pub fn speech_rate(&self, audio_seconds: f64) -> Option<SpeechRate> {
        speech_rate(&self.text, audio_seconds)
    }
}

/// 一份文稿的语速，按**它自己的书写系统**计数。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpeechRate {
    /// 每秒多少个单位。
    pub per_second: f64,
    /// `true` = 拉丁等**分词书写**，单位是词；`false` = 中日韩，单位是字。
    pub spaced: bool,
}

impl SpeechRate {
    /// 这个语速像不像人话。
    ///
    /// 两条带子不是一个尺子：中文 4–6 **字**/秒，英语 2–3.5 **词**/秒。拿同一
    /// 条上限去卡两种文字，正常语速会被判成「文稿与音频不匹配」——一段正常的英
    /// 语字幕换算成字母是 14–20 个/秒，在中文那 9 的上限之下就是全错。
    pub fn plausible(&self) -> bool {
        if self.spaced {
            (0.8..=5.0).contains(&self.per_second)
        } else {
            (1.5..=9.0).contains(&self.per_second)
        }
    }
}

/// 这份文稿按字算还是按词算。
///
/// 看的是**中日韩字符 vs 拉丁字母**的量，不是「有没有空格」：中文文稿里混着英文
/// 术语、日文文稿里混着片假名latin，都属于「按字算」。阈值给到 4:1，因为一段中文
/// 里嵌一两个英文单词是常态，不该因此翻到词那一侧。
fn is_cjk_dominant(text: &str) -> bool {
    let mut cjk = 0usize;
    let mut latin = 0usize;
    for c in text.chars() {
        let cp = c as u32;
        if matches!(cp, 0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0xAC00..=0xD7AF)
        {
            cjk += 1;
        } else if c.is_ascii_alphabetic() {
            latin += 1;
        }
    }
    cjk * 4 >= latin
}

/// 文稿语速。`audio_seconds` 未知、或文稿里数不出任何单位时是 `None`——没有分母
/// 就不画条，而不是显示一个 0。
pub fn speech_rate(text: &str, audio_seconds: f64) -> Option<SpeechRate> {
    if audio_seconds <= 0.0 {
        return None;
    }
    let spaced = !is_cjk_dominant(text);
    let units: usize = if spaced {
        text.split_whitespace()
            .filter(|w| w.chars().any(|c| c.is_alphanumeric()))
            .count()
    } else {
        // 标点不发音：「，」「。」计进语速会把一段正常的中文文稿算快两成。
        // `is_alphanumeric` 覆盖汉字、假名、谚文（它们带 Alphabetic 属性）。
        text.chars().filter(|c| c.is_alphanumeric()).count()
    };
    (units > 0).then(|| SpeechRate {
        per_second: units as f64 / audio_seconds,
        spaced,
    })
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

    /// 英文文稿按**词**数，不是按字母数。这条钉住的就是那次误报：一段正常的英语
    /// 字幕按字母算是 14–20 个/秒，落在中文那 9 的上限之外，被判成「文稿与音频
    /// 不匹配」——而它其实是完全正常的素材。
    #[test]
    fn an_english_transcript_is_counted_in_words() {
        let text = "the quick brown fox jumps over the lazy dog again and again";
        let r = speech_rate(text, 10.0).expect("a countable transcript");
        assert!(r.spaced, "latin text must be counted per word");
        assert_eq!(r.per_second, 1.2, "12 words over 10 s");
        assert!(r.plausible(), "{r:?} should read as normal speech");
    }

    #[test]
    fn a_chinese_transcript_is_counted_in_characters() {
        // 20 个字（，和。不发音，不计），5 秒 → 4.0 字/秒，正是中文正常语速。
        let text = "今天我们聊一聊字幕这件事，以及它为什么重要。";
        let r = speech_rate(text, 5.0).expect("a countable transcript");
        assert!(!r.spaced, "cjk text must be counted per character");
        assert!((r.per_second - 4.0).abs() < 1e-9, "{r:?}");
        assert!(r.plausible(), "4.0 字/秒 is a normal speaking rate");
    }

    /// 同一段文字：音频短一半太快、长五倍太慢——带子要真的能分辨得出。
    #[test]
    fn a_mismatch_shows_up_as_an_impossible_rate() {
        let text = "今天我们聊一聊字幕这件事，以及它为什么重要。";
        assert!(!speech_rate(text, 2.0).unwrap().plausible(), "10 字/秒");
        assert!(!speech_rate(text, 30.0).unwrap().plausible(), "0.67 字/秒");
    }

    /// 中文里嵌一两个英文单词是常态，不该因此翻到「按词算」那一侧。
    #[test]
    fn a_chinese_transcript_with_english_terms_still_counts_characters() {
        let text = "我们来聊聊 Whisper 这个模型，还有 OneAsr 这个软件。";
        let r = speech_rate(text, 5.0).expect("a countable transcript");
        assert!(!r.spaced, "still cjk-dominant: {r:?}");
    }

    /// 没数出任何东西（空文稿、纯标点）就没有分母，不显示 0。
    #[test]
    fn nothing_countable_means_no_rate_at_all() {
        assert_eq!(speech_rate("", 10.0), None);
        assert_eq!(speech_rate("   \n  \n", 10.0), None);
        assert_eq!(speech_rate("。。。？！", 10.0), None);
        // 时长未知同样没有分母。
        assert_eq!(speech_rate("hello world", 0.0), None);
    }

    /// 两条带子确实不是同一把尺子。
    #[test]
    fn the_two_scripts_get_their_own_bands() {
        let rate = |per_second, spaced| SpeechRate { per_second, spaced };
        // 2.9 在两种写法下都是人话。
        assert!(rate(2.9, true).plausible() && rate(2.9, false).plausible());
        // 14.2 词/秒是疯了；14.2 字/秒对英语是正常语速（≈2.5 词/秒），对中文太快。
        // 「按字还是按词」这个判断本身就决定了结论——所以它必须先判对。
        assert!(!rate(14.2, true).plausible());
        assert!(!rate(14.2, false).plausible());
    }

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
    fn counts_and_rate_are_reported_for_the_guard() {
        let t = parse_transcript(TranscriptKind::Plain, "这是第一句。\n这是第二句。").unwrap();
        assert_eq!(t.char_count(), 12);
        assert_eq!(t.line_count(), 2);
        let r = t.speech_rate(6.0).expect("10 chars over 6 s");
        assert!(!r.spaced, "中文按字算");
        assert!((r.per_second - 10.0 / 6.0).abs() < 1e-9, "{r:?}");
        assert!(r.plausible(), "1.67 字/秒 is a slow but real speaking rate");
        assert_eq!(t.speech_rate(0.0), None, "音频时长未知时不编造语速");
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

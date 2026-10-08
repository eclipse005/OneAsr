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
    /// 行数、字数与语速单位——一次扫描算完（见 [`text_stats`]）。
    pub fn stats(&self) -> TextStats {
        text_stats(&self.text)
    }

    /// 文字量（不含空白）——密度校验与字数展示用。
    pub fn char_count(&self) -> usize {
        self.stats().chars
    }

    /// 字幕行数。
    pub fn line_count(&self) -> usize {
        self.stats().lines
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
        self.stats().rate(audio_seconds)
    }
}

/// 一份文稿的统计量——**一次全文扫描**算完。
///
/// 这几个数以前是各算各的：行数一遍、字数一遍、中日韩判定一遍、语速单位又一
/// 遍。任务列表每帧重绘一次、每行都要重问一次，挂 100 份长文稿时一秒就是几万
/// 遍全文。规则一个字都没改，只是合成一遍扫描，所以算出来的数必然一模一样
/// （[`text_stats`] 的对照测试拿旧写法逐条比过）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextStats {
    /// 非空行数——`trim` 后还剩东西的行。
    pub lines: usize,
    /// 文字量（不含空白）。
    pub chars: usize,
    /// `true` = 拉丁等**分词书写**，单位是词；`false` = 中日韩，单位是字。
    pub spaced: bool,
    /// 语速单位数：`spaced` 时是「含字母数字的词」数，否则是「字母数字字符」数。
    pub units: usize,
}

impl TextStats {
    /// 这份文稿按每秒多少「单位」说话。判定与 [`speech_rate`] 逐条对应。
    pub fn rate(&self, audio_seconds: f64) -> Option<SpeechRate> {
        if audio_seconds <= 0.0 {
            return None;
        }
        // 没有分母就不画条，而不是显示一个 0。
        (self.units > 0).then(|| SpeechRate {
            per_second: self.units as f64 / audio_seconds,
            spaced: self.spaced,
        })
    }
}

/// 一次扫描算出文稿的行数、字数与语速单位。
///
/// 每个判定都与原先分散在四个函数里的写法**逐条对应**，没有换过任何一个条件：
///
/// - **行数**：`lines()` 只按 `\n` 切，且不把末尾换行算成多出来的一行；`\r\n`
///   的 `\r` 会被剥掉，但 `\r` 本身是空白，所以剥不剥它不改变「这行是不是空的」。
///   于是「非空行数」= 每个 `\n` 处已经见过非空白字符的行数，再加末尾那段没被
///   换行结束的（如果它非空）。
/// - **字数**：`!c.is_whitespace()`。
/// - **按字还是按词**：仍是 `cjk * 4 >= latin`。看的是**中日韩字符 vs 拉丁字母**
///   的量，不是「有没有空格」：中文文稿里混着英文术语、日文文稿里混着片假名，
///   都属于「按字算」。阈值给到 4:1，因为一段中文里嵌一两个英文单词是常态。
/// - **语速单位**：按词时数「含 `is_alphanumeric` 的空白分隔词」，按字时数
///   `is_alphanumeric` 的字符（标点不发音，「，」「。」计进去会把一段正常的中文
///   文稿算快两成）。两种都数了，因为落到哪一侧要扫完才知道。
pub fn text_stats(text: &str) -> TextStats {
    let mut chars = 0usize; // 不含空白的文字量
    let mut alnum_chars = 0usize; // 语速单位候选：字母数字**字符**数（按字算时用它）
    let mut alnum_words = 0usize; // 语速单位候选：含字母数字的**词**数（按词算时用它）
    let mut cjk = 0usize;
    let mut latin = 0usize;
    let mut lines = 0usize; // 非空行数
    let mut line_has_text = false; // 当前行见过非空白字符
    let mut word_has_alnum = false; // 当前词见过字母数字
    let mut saw_any = false; // 文本非空（决定末尾那段算不算一行）
    for c in text.chars() {
        saw_any = true;
        let ws = c.is_whitespace();
        if !ws {
            chars += 1;
            line_has_text = true;
        }
        if c.is_alphanumeric() {
            alnum_chars += 1;
            word_has_alnum = true;
        }
        let cp = c as u32;
        if matches!(cp, 0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0xAC00..=0xD7AF)
        {
            cjk += 1;
        } else if c.is_ascii_alphabetic() {
            latin += 1;
        }
        if ws {
            // 空白既是词边界也是（`\n` 时的）行边界。`split_whitespace` 按任意
            // 空白切词，这里逐字符判空白等价：连续空白只在第一个上收口一次词。
            if c == '\n' && line_has_text {
                lines += 1;
            }
            if c == '\n' {
                line_has_text = false;
            }
            if word_has_alnum {
                alnum_words += 1;
            }
            word_has_alnum = false;
        }
    }
    if saw_any && line_has_text {
        lines += 1;
    }
    if word_has_alnum {
        alnum_words += 1;
    }
    let cjk_dominant = cjk * 4 >= latin;
    let spaced = !cjk_dominant;
    TextStats {
        lines,
        chars,
        spaced,
        units: if spaced { alnum_words } else { alnum_chars },
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

/// 文稿语速。`audio_seconds` 未知、或文稿里数不出任何单位时是 `None`——没有分母
/// 就不画条，而不是显示一个 0。
///
/// 统计量只算一遍（[`text_stats`]），分行还是按词见 [`TextStats`]。
pub fn speech_rate(text: &str, audio_seconds: f64) -> Option<SpeechRate> {
    text_stats(text).rate(audio_seconds)
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

    /// 合成一次扫描之后，判定必须和原先分散在四个函数里的写法**逐条等价**。
    /// 这条钉住的就是「优化」最容易出事的地方：省掉一遍扫描时顺手把某个条件改
    /// 了（比如行尾 `\r`、末尾换行、全角空格），卡片上的数字就会悄悄变。
    ///
    /// 参考实现就是优化前那四段代码的逐字拷贝，逐个用例对照。
    fn old_char_count(text: &str) -> usize {
        text.chars().filter(|c| !c.is_whitespace()).count()
    }

    fn old_line_count(text: &str) -> usize {
        text.lines().filter(|l| !l.trim().is_empty()).count()
    }

    fn old_speech_rate(text: &str, audio_seconds: f64) -> Option<SpeechRate> {
        if audio_seconds <= 0.0 {
            return None;
        }
        let cjk = text
            .chars()
            .filter(|c| {
                let cp = *c as u32;
                matches!(cp, 0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0xAC00..=0xD7AF)
            })
            .count();
        let latin = text.chars().filter(char::is_ascii_alphabetic).count();
        let cjk_dominant = cjk * 4 >= latin;
        let spaced = !cjk_dominant;
        let units: usize = if spaced {
            text.split_whitespace()
                .filter(|w| w.chars().any(|c| c.is_alphanumeric()))
                .count()
        } else {
            text.chars().filter(|c| c.is_alphanumeric()).count()
        };
        (units > 0).then(|| SpeechRate {
            per_second: units as f64 / audio_seconds,
            spaced,
        })
    }

    #[test]
    fn one_scan_reproduces_every_old_scan() {
        // 覆盖每一条会被两种算法区别对待的边界：CRLF、末尾有无换行、纯空白行、
        // 连续空白、不发音的标点、全角空格，以及中 / 英 / 混排三种书写系统。
        let corpus = [
            "这是第一句。\n这是第二句。",
            "hello there my friend how are you doing tonight",
            "中英 mixed 混排 Whisper OneAsr",
            "第一行。\r\n\r\n   \r\n第三行。",
            "尾行没有换行",
            "尾行有换行\n",
            "\n只有一个换行",
            "。。。？！",
            "全角空格\u{a0}夹着\u{a0}文字\u{a0}",
            "日文とカタカナと漢字",
            "한국어 한 줄",
            "a  b\tc\u{b}\u{c}d",
            "   leading spaces",
            "trailing spaces   ",
        ];
        for text in corpus {
            let s = text_stats(text);
            assert_eq!(s.chars, old_char_count(text), "字数: {text:?}");
            assert_eq!(s.lines, old_line_count(text), "行数: {text:?}");
            for audio in [0.0, 0.5, 2.0, 6.0, 120.0] {
                assert_eq!(
                    s.rate(audio),
                    old_speech_rate(text, audio),
                    "语速 {audio}s: {text:?}"
                );
                assert_eq!(
                    speech_rate(text, audio),
                    old_speech_rate(text, audio),
                    "speech_rate {audio}s: {text:?}"
                );
            }
        }
    }

    /// 时长是 `NaN` 时 `NaN <= 0.0` 为假，所以两条路都会算出一个 `NaN` 语速而
    /// 不是 `None`——**这是原样保留的行为**（密度校验那边靠 `plausible()` 把
    /// `NaN` 判掉）。这里只钉住两条路一致，不把它改成 `None`。
    #[test]
    fn a_nan_duration_behaves_identically_on_both_paths() {
        let text = "hello world";
        let from_stats = text_stats(text).rate(f64::NAN);
        let from_fn = speech_rate(text, f64::NAN);
        assert_eq!(
            from_stats.map(|r| r.spaced),
            from_fn.map(|r| r.spaced),
            "按字还是按词，两条路要一样"
        );
        assert!(
            from_stats.expect("NaN 时长不返回 None").per_second.is_nan(),
            "{from_stats:?}"
        );
        // 负时长与零时长照旧是没有分母。
        assert_eq!(text_stats(text).rate(-1.0), None);
        assert_eq!(text_stats(text).rate(0.0), None);
    }

    /// `Transcript` 上的三个数与自由函数一致：调用方（CLI 的加载提示、密度校验）
    /// 无论走哪条路，看到的都是同一个数。
    #[test]
    fn a_transcript_reports_the_same_stats_as_the_free_function() {
        let t = parse_transcript(TranscriptKind::Plain, "第一行。\n\n第二行 hello。").unwrap();
        let s = t.stats();
        assert_eq!(s.chars, old_char_count(&t.text));
        assert_eq!(s.lines, old_line_count(&t.text));
        assert_eq!(t.char_count(), s.chars);
        assert_eq!(t.line_count(), s.lines);
        assert_eq!(t.speech_rate(4.0), s.rate(4.0));
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

//! Assembling finished source sentences into subtitle text (SRT).
//!
//! TXT never comes through here: it is not a subtitle without timestamps but a
//! plain sentence list built from the text ([`crate::txt`]).

use crate::sentence_boundary::WordTokenDto;
use crate::subtitle::srt::to_srt_from_cues;
use crate::timeline::Cue;

use super::types::{
    BoundaryDecision, BoundaryDecisionKind, MicroChunk, SourceSentence, SourceSentenceStep2,
    SplitReason,
};
use super::util::join_words;
use super::util::{gap_ms, seconds_to_ms};
use super::vad_align::SpeechSegmentIndex;

pub fn source_sentences_to_srt(step2: &SourceSentenceStep2) -> String {
    let cues = step2
        .translation_sentences
        .iter()
        .map(|sentence| Cue {
            index: sentence.sentence_id,
            start_ms: sentence.start_ms,
            end_ms: sentence.end_ms,
            text: sentence.text.clone(),
        })
        .collect::<Vec<_>>();
    to_srt_from_cues(&cues)
}

pub(super) fn build_micro_chunks(
    words: &[WordTokenDto],
    vad_index: &SpeechSegmentIndex,
) -> Vec<MicroChunk> {
    words
        .iter()
        .enumerate()
        .map(|(index, word)| {
            let gap_before_ms = index
                .checked_sub(1)
                .and_then(|prev| words.get(prev))
                .map(|prev| gap_ms(prev.end, word.start))
                .unwrap_or(0);
            let gap_after_ms = words
                .get(index + 1)
                .map(|next| gap_ms(word.end, next.start))
                .unwrap_or(0);
            let hard_split_before = index
                .checked_sub(1)
                .and_then(|prev| words.get(prev))
                .map(|prev| vad_index.crosses_silence(prev.end, word.start))
                .unwrap_or(false);
            let hard_split_after = words
                .get(index + 1)
                .map(|next| vad_index.crosses_silence(word.end, next.start))
                .unwrap_or(false);
            MicroChunk {
                chunk_id: index + 1,
                start_ms: seconds_to_ms(word.start),
                end_ms: seconds_to_ms(word.end.max(word.start)),
                text: word.word.clone(),
                word_start: index,
                word_end: index,
                gap_before_ms,
                gap_after_ms,
                hard_split_before,
                hard_split_after,
            }
        })
        .collect()
}

pub(super) fn build_sentences_from_word_spans(
    words: &[WordTokenDto],
    spans: &[(usize, usize)],
) -> Vec<SourceSentence> {
    let mut out: Vec<SourceSentence> = Vec::with_capacity(spans.len());
    for (start, end) in spans.iter().copied() {
        if let Some((start_ms, end_ms)) = word_span_time(words, start, end) {
            out.push(SourceSentence {
                sentence_id: out.len() + 1,
                start_ms,
                end_ms,
                text: join_words(words[start..=end].iter().map(|word| word.word.as_str())),
                word_start: start,
                word_end: end,
                chunk_start: start + 1,
                chunk_end: end + 1,
            });
        }
    }
    out
}

/// 文稿的一行，以及它覆盖的那段**字符**。
///
/// `text` 是**文稿那一行的原文**，不是从词拼出来的：文稿模式的字幕文字由用户
/// 决定，行内的空格、全角空格、全角标点都原样保留（见
/// [`build_sentences_from_lines`]）。`first`/`last` 是这一行在**非空白字符流**
/// 里的下标（首字、末字，闭区间），时间靠它们从词上取。
///
/// 用字符而不是词做下标，是因为这两者的边界并不重合：对齐器的 token 会把标点
/// 粘到前一个字上（`道：“`、`的。”“`），一行文稿却常常正好切在这种 token 中间。
/// 按整词分配时，这种跨界只能整个判给前一行，于是下一行的首字被推到它**后面**
/// 那个 token 的开头——字幕比声音晚出，而且这个误差会一路累积下去。
pub(super) struct LineSpan<'a> {
    pub first: usize,
    pub last: usize,
    pub text: &'a str,
}

/// 文稿模式：一条字幕 = 文稿的一行，**文字逐字取原文**，时间精确到字符。
///
/// 与 [`build_sentences_from_word_spans`] 的差别不只是文字来源。那条路按整词取
/// 时间（`words[start].start` / `words[end].end`），因为自动断句的切点本来就在
/// 词边界上；这条路不能那么算——文稿的行边界落在 token 中间时，整词取时间会把
/// 边界往后推一整个 token，一行推一点，几十行之后就是秒级偏差（见
/// [`char_stream_time`] 的说明）。
///
/// `words` 必须与 `text` 是**同一段字符流**：`first`/`last` 是在这条流上数的，
/// 越界（词流被文稿超用）时那一条不产出。
pub(super) fn build_sentences_from_lines(
    words: &[WordTokenDto],
    lines: &[LineSpan<'_>],
) -> Vec<SourceSentence> {
    let stream = CharStream::new(words);
    let mut out: Vec<SourceSentence> = Vec::with_capacity(lines.len());
    for line in lines {
        let (first, last) = (line.first, line.last);
        let Some((start_ms, end_ms)) = stream.span_time(first, last) else {
            continue;
        };
        out.push(SourceSentence {
            sentence_id: out.len() + 1,
            start_ms,
            end_ms,
            text: line.text.to_string(),
            // 卡拉OK 要按单元逐字扫（`subtitle::ass`），所以这两个下标仍指向
            // **词**：覆盖这一行字符的那一段词。时间是字符级的，逐字扫描仍按词走。
            word_start: stream.token_of(first),
            word_end: stream.token_of(last),
            chunk_start: stream.token_of(first) + 1,
            chunk_end: stream.token_of(last) + 1,
        });
    }
    out
}

/// 对齐器 token 流按**非空白字符**展开后的视图，每个字符都记得自己落在哪个
/// token 的第几个字。
///
/// 为什么需要它：对齐器的 token 是「一个整词或一个字 + 跟在后面的标点」
/// （`道：“` 是一个 token，`的。”“` 也是）。而文稿的行边界是**字**边界，两者
/// 并不对齐——一行可能正好切在 `道：“` 的中间。按整词分配只能把跨界判给前一行
/// 或后一行，误差一整个 token；一行一行攒下来就是用户看到的那句
/// 「文本落后于声音」。
///
/// 一个 token 内**标点没有自己的时长**（对齐器就是这么报的：标点的 start 等于
/// 前面那个字的 start），所以字符级时间只在字母数字上线性插值，标点沿用所在
/// token 的端点。这样切在 token 中间的边界拿到的是那个字真正被说出来的时刻，
/// 而不是整 token 的起点。
struct CharStream {
    /// 每个字符：所在 token、该 token 内第几个字母数字、该 token 有几个字母数字、
    /// 是不是标点。
    cells: Vec<Cell>,
    /// 每个字符所属 token 的 `start` / `end`（秒）。
    token_times: Vec<(f64, f64)>,
}

#[derive(Clone, Copy)]
struct Cell {
    token: usize,
    index: usize,
    count: usize,
    is_mark: bool,
}

impl CharStream {
    fn new(words: &[WordTokenDto]) -> Self {
        let mut cells = Vec::new();
        let mut token_times = Vec::with_capacity(words.len());
        for (token, word) in words.iter().enumerate() {
            token_times.push((word.start, word.end));
            let chars: Vec<char> = word.word.chars().filter(|c| !c.is_whitespace()).collect();
            let count = chars.iter().filter(|c| c.is_alphanumeric()).count();
            let mut index = 0usize;
            for ch in chars {
                let is_mark = !ch.is_alphanumeric();
                cells.push(Cell {
                    token,
                    index,
                    count,
                    is_mark,
                });
                if !is_mark {
                    index += 1;
                }
            }
        }
        Self { cells, token_times }
    }

    fn token_of(&self, char_index: usize) -> usize {
        self.cells
            .get(char_index)
            .map(|cell| cell.token)
            .unwrap_or_else(|| self.token_times.len().saturating_sub(1))
    }

    /// 第 `char_index` 个字符**开始被说出来**的时刻。
    fn start_of(&self, char_index: usize) -> Option<f64> {
        let cell = *self.cells.get(char_index)?;
        let (start, end) = self.token_times[cell.token];
        if cell.is_mark || cell.count == 0 {
            return Some(start);
        }
        Some(start + (end - start) * cell.index as f64 / cell.count as f64)
    }

    /// 第 `char_index` 个字符**说完**的时刻。
    fn end_of(&self, char_index: usize) -> Option<f64> {
        let cell = *self.cells.get(char_index)?;
        let (start, end) = self.token_times[cell.token];
        if cell.is_mark || cell.count == 0 {
            return Some(end);
        }
        Some(start + (end - start) * (cell.index + 1) as f64 / cell.count as f64)
    }

    /// 从 `char_index` 起**往后**第一个真正发声的字符的起点。
    ///
    /// 行首常常是一个标点（`“在我上船之前` 的那个引号），而标点粘在前一个字上、
    /// 自己没有时长。拿它的 token 起点当字幕起点，等于让这条字幕提前到上一行
    /// 那个字被说出来的时刻（实测能早 1–1.6 s）。所以这里跳过没有时长的字符，
    /// 取真正被说出来的那一个。
    ///
    /// 行尾不需要对称地往回跳：标点零时长，[`Self::end_of`] 对它的答案就是它粘
    /// 着那个字的终点，两个写法给出同一个数。往前跳反而会丢掉真实对齐器偶尔
    /// 给标点的独立时长。
    fn first_spoken_start(&self, char_index: usize) -> Option<f64> {
        self.cells
            .iter()
            .enumerate()
            .skip(char_index)
            .find(|(_, cell)| !cell.is_mark && cell.count > 0)
            .and_then(|(index, _)| self.start_of(index))
            .or_else(|| self.start_of(char_index))
    }

    /// 一段字符的起止时间；越界时返回 `None`（词流被文稿超用）。
    fn span_time(&self, first: usize, last: usize) -> Option<(u64, u64)> {
        let start = self.first_spoken_start(first)?;
        let end = self.end_of(last)?.max(start);
        Some((seconds_to_ms(start), seconds_to_ms(end)))
    }
}

/// 一段词的起止时间；区间越界（词流被文稿超用）时返回 `None`。
fn word_span_time(words: &[WordTokenDto], start: usize, end: usize) -> Option<(u64, u64)> {
    if start >= words.len() || end >= words.len() || start > end {
        return None;
    }
    Some((
        seconds_to_ms(words[start].start),
        seconds_to_ms(words[end].end.max(words[start].start)),
    ))
}

pub(super) fn build_boundaries_from_split_points(
    micro_chunks: &[MicroChunk],
    split_points: &[(usize, SplitReason)],
) -> Vec<BoundaryDecision> {
    if micro_chunks.len() < 2 {
        return Vec::new();
    }

    let mut split_by_end = std::collections::HashMap::<usize, SplitReason>::new();
    for (end, reason) in split_points.iter().copied() {
        split_by_end.insert(end, reason);
    }

    (0..micro_chunks.len() - 1)
        .map(|index| {
            let left = &micro_chunks[index];
            let right = &micro_chunks[index + 1];
            let split_reason = split_by_end.get(&index).copied();
            let (rule_decision, llm_decision, final_decision, confidence, reason_tag) =
                match split_reason {
                    Some(SplitReason::TerminalPunctuation) => (
                        BoundaryDecisionKind::Split,
                        BoundaryDecisionKind::Unknown,
                        BoundaryDecisionKind::Split,
                        1.0,
                        "terminal_punctuation",
                    ),
                    Some(SplitReason::SubtitleLayout) => (
                        BoundaryDecisionKind::Split,
                        BoundaryDecisionKind::Unknown,
                        BoundaryDecisionKind::Split,
                        0.9,
                        "subtitle_layout",
                    ),
                    None => (
                        BoundaryDecisionKind::Merge,
                        BoundaryDecisionKind::Unknown,
                        BoundaryDecisionKind::Merge,
                        0.95,
                        "merge",
                    ),
                };
            BoundaryDecision {
                left_chunk_id: left.chunk_id,
                right_chunk_id: right.chunk_id,
                gap_ms: gap_ms(
                    (left.end_ms as f64) / 1000.0,
                    (right.start_ms as f64) / 1000.0,
                ),
                rule_decision,
                llm_decision,
                final_decision,
                confidence,
                reason_tag: reason_tag.to_string(),
            }
        })
        .collect()
}

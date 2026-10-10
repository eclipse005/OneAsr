//! Karaoke ASS output: the same cues as the SRT, with a sweep per unit.
//!
//! # What a unit is here
//!
//! The unit is whatever the alignment already decided: a **character** for a
//! script that writes without spaces, a **word** for one that does not. That is
//! the granularity the timeline carries, so this renderer adds no decision of
//! its own — and it is the practical karaoke convention (per-character for CJK,
//! per-word for Latin; per-syllable would need phoneme boundaries nobody here
//! has). Both aligners land in the same place, which is the point: this file
//! knows nothing about CTC or Qwen, it reads `WordToken`s.
//!
//! Three details that are easy to get wrong, and what they cost when they are:
//!
//! * **A `\k` is the gap from this unit's onset to the next one**, not this
//!   unit's own span. Written the other way, every sweep is off by one and the
//!   highlight trails the voice.
//! * **A real space is `\h`, not a space.** A plain space is a wrap point; a
//!   hard space is not. Written as a space, the player breaks the line where the
//!   transcript has a word gap.
//! * **A zero sweep is no tag at all.** `{\k0}` is not a zero-length highlight —
//!   mpv, libass and xy-VSFilter each draw it differently from a positive one,
//!   and the character comes out in the unsung colour. Emitting nothing lets it
//!   ride inside its neighbour's sweep, which every player agrees on.
//!
//! The line times are the same ones the SRT got, including the monotonic pass
//! (see [`crate::timeline::enforce_monotonic`]): a karaoke file and an SRT
//! beside it must not disagree about when a line is on screen.
//!
//! [Script Info], [V4+ Styles] and [Events] are written out in full because a
//! partial ASS file is not a smaller ASS file. `WrapStyle: 0` is deliberate: the
//! line breaking is the player's, exactly as it is for an SRT.

use crate::sentence_boundary::{SourceSentences, WordTokenDto, spacing_pieces};
use crate::timeline::{Cue, enforce_monotonic};

/// Subtitle appearance for the karaoke output.
///
/// Exposed so a settings row can drive it later; the defaults are what the file
/// gets today. ASS colours are `AABBGGRR`, and the sweep runs from
/// `secondary` to `primary`: a unit sits in `secondary` until its `\k` has
/// elapsed, then stays `primary`.
#[derive(Debug, Clone)]
pub struct KaraokeStyle {
    pub title: String,
    /// Player-resolved. The default covers Simplified Chinese and Latin; other
    /// scripts fall back per glyph, which is what libass and VSFilter both do.
    pub font: String,
    pub font_size: f64,
    /// The colour a unit ends on, after its sweep.
    pub primary: u32,
    /// The colour a unit sits in before its sweep.
    pub secondary: u32,
    pub outline: u32,
    pub back: u32,
    pub outline_width: f64,
    pub shadow: f64,
    pub margin_l: i64,
    pub margin_r: i64,
    pub margin_v: i64,
    /// The coordinate space ASS draws in; players scale it to the video.
    pub play_res_x: i64,
    pub play_res_y: i64,
}

impl Default for KaraokeStyle {
    fn default() -> Self {
        Self {
            title: "OneAsr".to_string(),
            font: "Microsoft YaHei".to_string(),
            font_size: 64.0,
            primary: 0x0000E5FF,   // BGR: amber
            secondary: 0x00FFFFFF, // BGR: white
            outline: 0x00202020,
            back: 0x80000000,
            outline_width: 3.2,
            shadow: 1.0,
            margin_l: 80,
            margin_r: 80,
            margin_v: 90,
            play_res_x: 1920,
            play_res_y: 1080,
        }
    }
}

/// The karaoke rendering of finished sentences, one `\k` per aligned unit.
pub fn to_ass(sentences: &SourceSentences, style: &KaraokeStyle) -> String {
    to_ass_with(sentences, style, false)
}

/// [`to_ass`]，`polish` 为真时正文走字幕美化，扫光仍按原来的对齐单元。
pub(crate) fn to_ass_with(
    sentences: &SourceSentences,
    style: &KaraokeStyle,
    polish: bool,
) -> String {
    // The same times the SRT will carry, monotonicity included. Sentences come
    // out of the assembler in time order, which is what `enforce_monotonic`
    // assumes.
    let mut cues: Vec<Cue> = sentences
        .translation_sentences
        .iter()
        .map(|s| Cue {
            index: s.sentence_id,
            start_ms: s.start_ms,
            end_ms: s.end_ms,
            text: s.text.clone(),
        })
        .collect();
    enforce_monotonic(&mut cues);

    let mut out = String::new();
    header(&mut out, style);
    for cue in &cues {
        let Some(sentence) = sentences
            .translation_sentences
            .iter()
            .find(|s| s.sentence_id == cue.index)
        else {
            continue;
        };
        let tokens = &sentences.words[sentence.word_start..=sentence.word_end];
        let body = if polish {
            cue_body_polished(cue, tokens)
        } else {
            cue_body(cue, tokens)
        };
        if body.is_empty() {
            continue;
        }
        out.push_str(&format!(
            "Dialogue: 0,{},{},Karaoke,,0,0,0,,{}\n",
            fmt_ass_time(cue.start_ms),
            fmt_ass_time(cue.end_ms),
            body
        ));
    }
    out
}

/// One Dialogue line's markup: a sweep per unit, a hard space where the cue
/// text has a space.
fn cue_body(cue: &Cue, tokens: &[WordTokenDto]) -> String {
    // The pieces skip blank tokens, so the two lists line up only if this
    // filter removes the same ones. It does: a token that is blank once trimmed
    // is exactly the one `spacing_pieces` drops.
    let units: Vec<&WordTokenDto> = tokens
        .iter()
        .filter(|t| !t.word.trim().is_empty())
        .collect();
    let pieces = spacing_pieces(units.iter().map(|t| t.word.as_str()));
    debug_assert_eq!(pieces.len(), units.len());

    let mut body = String::new();
    // Where the previous sweep ended, in milliseconds. The first unit starts at
    // the line's own start, so the line's opening silence falls into its first
    // sweep rather than into nothing.
    let mut at = cue.start_ms as i64;
    let line_end_ms = cue.end_ms as i64;
    for (i, piece) in pieces.iter().enumerate() {
        let next_ms = units
            .get(i + 1)
            .map(|u| (u.start * 1000.0).round() as i64)
            .unwrap_or(line_end_ms)
            .max(at);
        if piece.space_before {
            body.push_str("\\h");
        }
        // The sweep is this unit's onset to the next one. A unit with no room of
        // its own (an interpolated character, a mark that opened its own word)
        // gets no tag at all and rides inside its neighbour's.
        let cs = (next_ms - at) / 10;
        if cs > 0 {
            body.push_str(&format!("{{\\k{cs}}}"));
        }
        at = at.max(next_ms);
        body.push_str(&piece.text);
    }
    // Whatever is left of the line after the last unit: the tail of the final
    // sweep, which the unit's own span would otherwise leave unsung.
    let tail = (line_end_ms - at) / 10;
    if tail > 0 {
        body.push_str(&format!("{{\\k{tail}}}"));
    }
    body
}

/// 美化后的卡拉 OK。扫光仍按对齐单元，不按美化插进去的空格再切一刀。
/// 空格一律写成 `\h`：普通空格是换行点，会把已经定好的一条字幕拆开。
fn cue_body_polished(cue: &Cue, tokens: &[WordTokenDto]) -> String {
    let units: Vec<&WordTokenDto> = tokens
        .iter()
        .filter(|t| !t.word.trim().is_empty())
        .collect();
    let pieces = spacing_pieces(units.iter().map(|t| t.word.as_str()));
    let mut original = String::new();
    let mut owners: Vec<Option<usize>> = Vec::new();
    for (pi, piece) in pieces.iter().enumerate() {
        if piece.space_before {
            original.push(' ');
            owners.push(None);
        }
        for _ in piece.text.chars() {
            owners.push(Some(pi));
        }
        original.push_str(&piece.text);
    }

    let mapped = crate::subtitle::polish::polish_mapped(&original);
    let mut body = String::new();
    let mut at = cue.start_ms as i64;
    let line_end = cue.end_ms as i64;
    let line_start = at;
    let onset = |pi: usize| -> i64 {
        units
            .get(pi)
            .map(|u| (u.start * 1000.0).round() as i64)
            .unwrap_or(line_start)
    };
    // 插进去的空格 src 指向它右边的字，所以和那个字同一个对齐单元。
    let owner = |src: usize| owners.get(src).copied().flatten();

    let mut i = 0;
    while i < mapped.len() {
        let piece = owner(mapped[i].src);
        if piece.is_none() {
            push_ass_char(&mut body, mapped[i].ch);
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        while i < mapped.len() && owner(mapped[i].src) == piece {
            i += 1;
        }
        let mut j = i;
        while j < mapped.len() && owner(mapped[j].src).is_none() {
            j += 1;
        }
        let next_at = if j < mapped.len() {
            owner(mapped[j].src).map(onset).unwrap_or(line_end)
        } else {
            line_end
        }
        .max(at);
        let cs = (next_at - at) / 10;
        if cs > 0 {
            body.push_str(&format!("{{\\k{cs}}}"));
        }
        at = at.max(next_at);
        for m in &mapped[start..i] {
            push_ass_char(&mut body, m.ch);
        }
    }
    let tail = (line_end - at) / 10;
    if tail > 0 {
        body.push_str(&format!("{{\\k{tail}}}"));
    }
    body
}

/// 空格写成硬空格，换行写成 `\N`。两者都不是播放器可以再折行的普通空格。
fn push_ass_char(body: &mut String, ch: char) {
    match ch {
        ' ' => body.push_str("\\h"),
        '\n' => body.push_str("\\N"),
        _ => body.push(ch),
    }
}

fn header(out: &mut String, st: &KaraokeStyle) {
    out.push_str("[Script Info]\n");
    out.push_str(&format!("Title: {}\n", st.title));
    out.push_str("ScriptType: v4.00+\n");
    out.push_str(&format!("PlayResX: {}\n", st.play_res_x));
    out.push_str(&format!("PlayResY: {}\n", st.play_res_y));
    // 0 = smart wrapping. An SRT carries no line breaks either and the player
    // supplies them; `WrapStyle: 2` ("only \N breaks") would send a long line
    // off the side of the frame.
    out.push_str("WrapStyle: 0\n");
    out.push_str("ScaledBorderAndShadow: yes\n");
    out.push_str("YCbCr Matrix: TV.709\n\n");
    out.push_str("[V4+ Styles]\n");
    out.push_str(
        "Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, \
         OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, \
         ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, \
         Alignment, MarginL, MarginR, MarginV, Encoding\n",
    );
    out.push_str(&format!(
        "Style: Karaoke,{},{},{},{},{},{},0,0,0,0,100,100,1,0,1,{},{},2,{},{},{},1\n",
        st.font,
        st.font_size as i64,
        ass_colour(st.primary),
        ass_colour(st.secondary),
        ass_colour(st.outline),
        ass_colour(st.back),
        st.outline_width,
        st.shadow,
        st.margin_l,
        st.margin_r,
        st.margin_v,
    ));
    out.push_str("\n[Events]\n");
    out.push_str(
        "Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
    );
}

fn ass_colour(v: u32) -> String {
    format!("&H{v:08X}")
}

/// `H:MM:SS.cc` — ASS counts centiseconds, not milliseconds.
fn fmt_ass_time(total_ms: u64) -> String {
    let cs = (total_ms / 10) as i64;
    let (cs, s) = (cs % 100, cs / 100);
    let (s, m) = (s % 60, s / 60);
    format!("{}:{:02}:{:02}.{:02}", s / 3600, m, s, cs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sentence_boundary::{
        SentenceBoundaryRequest, build_source_sentences_from_words, is_no_space_script,
    };

    /// The tokens the aligners actually hand over: one per character where the
    /// script writes without spaces, one per word where it does not. A fixture
    /// that emitted a whole Chinese sentence as one token would be testing a
    /// granularity nothing ever produces.
    fn aligner_tokens(text: &str) -> Vec<String> {
        let mut out = Vec::new();
        for raw in text.split_whitespace() {
            let spaced = raw
                .chars()
                .any(|c| c.is_alphanumeric() && !is_no_space_script(c));
            if spaced {
                out.push(raw.to_string());
            } else {
                out.extend(raw.chars().map(|c| c.to_string()));
            }
        }
        out
    }

    /// The sentences the way the pipeline makes them, so these tests exercise
    /// the real grouping rather than a convenient fixture.
    fn sentences_of(text: &str) -> SourceSentences {
        let words: Vec<WordTokenDto> = aligner_tokens(text)
            .into_iter()
            .enumerate()
            .map(|(i, t)| {
                let start = i as f64 * 0.4;
                WordTokenDto {
                    start,
                    end: start + 0.35,
                    word: t,
                }
            })
            .collect();
        build_source_sentences_from_words(SentenceBoundaryRequest {
            task_id: "t".into(),
            media_path: "clip.mp4".into(),
            source_lang: "zh".into(),
            subtitle_length_preset: "standard".into(),
            words,
            vad_speech_segments: vec![(0.0, 60.0)],
        })
        .expect("sentences")
    }

    /// The ASS text of each Dialogue line with its markup taken out — what a
    /// player would draw. `\h` is a space and a tag `{…}` is nothing; both are
    /// removed rather than reinterpreted.
    fn plain_lines(ass: &str) -> Vec<String> {
        ass.lines()
            .filter(|l| l.starts_with("Dialogue:"))
            .map(|l| {
                let body = l.splitn(10, ',').nth(9).unwrap_or("");
                let mut text = String::new();
                let mut chars = body.chars().peekable();
                let mut in_tag = false;
                while let Some(ch) = chars.next() {
                    match ch {
                        '{' => in_tag = true,
                        '}' => in_tag = false,
                        '\\' if !in_tag => {
                            if chars.peek() == Some(&'h') {
                                chars.next();
                                text.push(' ');
                            } else if chars.peek() == Some(&'N') {
                                chars.next();
                                text.push('\n');
                            }
                        }
                        _ if !in_tag => text.push(ch),
                        _ => {}
                    }
                }
                text
            })
            .collect()
    }

    /// The text of each SRT block, so the karaoke file can be checked against
    /// the file that ships beside it.
    fn srt_texts(srt: &str) -> Vec<String> {
        srt.split("\n\n")
            .filter_map(|block| {
                let lines: Vec<&str> = block.lines().collect();
                (lines.len() > 2 && lines[1].contains(" --> ")).then(|| lines[2..].join("\n"))
            })
            .collect()
    }

    fn sweeps(ass: &str) -> Vec<i64> {
        let mut out = Vec::new();
        for l in ass.lines().filter(|l| l.starts_with("Dialogue:")) {
            for tag in l.split('{').skip(1) {
                if let Some(cs) = tag
                    .split_once('}')
                    .map(|(t, _)| t.trim_start_matches('\\'))
                    .and_then(|t| t.strip_prefix('k'))
                    .and_then(|n| n.parse::<i64>().ok())
                {
                    out.push(cs);
                }
            }
        }
        out
    }

    /// The whole point of a karaoke file: strip the markup and you must get
    /// back the SRT's text, character for character. A `\h` in the wrong place
    /// or a token dropped here would show up as a subtitle that no longer says
    /// what the transcription said.
    #[test]
    fn the_karaoke_text_is_the_srt_text() {
        for text in [
            "这是第一句，测试确定性。这是第二句，也一样。",
            "原版Whisper，表现并不像可靠。",
            "AI语音识别工具，包括部署在N四六Whisper和Cap V二上的",
            "Hello world, this is karaoke. 说到English也要对得上。",
        ] {
            let sentences = sentences_of(text);
            let ass = to_ass(&sentences, &KaraokeStyle::default());
            let srt = crate::sentence_boundary::source_sentences_to_srt(&sentences);
            let srt_lines = srt_texts(&srt);
            assert!(!srt_lines.is_empty(), "fixture produced no cues: {text}");
            assert_eq!(
                plain_lines(&ass),
                srt_lines,
                "ASS 与 SRT 的文字不一致：{text}"
            );
        }
    }

    /// 美化打开后，卡拉 OK 去掉标记仍要和美化过的 SRT 逐字相同。
    #[test]
    fn polished_karaoke_text_matches_polished_srt() {
        let mut sentences = sentences_of("使用Whisper模型，共123人。");
        let ass = to_ass_with(&sentences, &KaraokeStyle::default(), true);
        for sentence in &mut sentences.translation_sentences {
            sentence.text = crate::subtitle::polish::polish_cue_text(&sentence.text);
        }
        sentences
            .translation_sentences
            .retain(|sentence| !sentence.text.trim().is_empty());
        let srt = crate::sentence_boundary::source_sentences_to_srt(&sentences);
        assert_eq!(plain_lines(&ass), srt_texts(&srt));
    }

    /// 数字和汉字被粘成一个对齐单元时，美化插进去的空格不能把扫光切碎，
    /// 也不能变成播放器可以折行的普通空格。
    #[test]
    fn polished_space_inside_a_glued_token_stays_one_sweep() {
        for text in ["1500人", "8月29日"] {
            let sentences = sentences_of(text);
            let ass = to_ass_with(&sentences, &KaraokeStyle::default(), true);
            let body = ass
                .lines()
                .find(|l| l.starts_with("Dialogue:"))
                .unwrap_or_else(|| panic!("no dialogue for {text}"));
            let payload = body.splitn(10, ',').nth(9).unwrap_or("");
            assert_eq!(sweeps(&ass).len(), 1, "{text} 应只有一次扫光：{body}");
            assert!(
                payload.contains("\\h"),
                "{text} 的空格应是硬空格：{payload}"
            );
            assert!(
                !ass_body_has_breakable_space(payload),
                "{text} 不能有普通空格：{payload}"
            );
            let sum: i64 = sweeps(&ass).into_iter().sum();
            let sentence = &sentences.translation_sentences[0];
            let span_cs = (sentence.end_ms as i64 - sentence.start_ms as i64).div_euclid(10);
            assert!(
                (sum - span_cs).abs() <= 1,
                "{text}: 扫光合计 {sum}cs 与行时长 {span_cs}cs 对不上"
            );
        }
    }

    fn ass_body_has_breakable_space(body: &str) -> bool {
        let mut chars = body.chars().peekable();
        let mut in_tag = false;
        while let Some(ch) = chars.next() {
            match ch {
                '{' => in_tag = true,
                '}' => in_tag = false,
                '\\' if !in_tag => {
                    chars.next();
                }
                ' ' if !in_tag => return true,
                _ => {}
            }
        }
        false
    }

    /// A sweep per character in Chinese, per word in Latin — the granularity the
    /// alignment already decided, so the count is the token count.
    #[test]
    fn cjk_sweeps_per_character_and_latin_per_word() {
        let cjk = to_ass(&sentences_of("你好世界"), &KaraokeStyle::default());
        assert_eq!(sweeps(&cjk).len(), 4, "四个汉字四次扫光");
        let latin = to_ass(&sentences_of("Hello world"), &KaraokeStyle::default());
        assert_eq!(sweeps(&latin).len(), 2, "两个单词两次扫光");
    }

    /// The sweeps have to add up to the line, or the highlight drifts and every
    /// later mark is late by the difference.
    #[test]
    fn the_sweeps_add_up_to_the_line() {
        let sentences = sentences_of("这是第一句，测试扫光是否闭合。这是第二句。");
        let ass = to_ass(&sentences, &KaraokeStyle::default());
        for (i, s) in sentences.translation_sentences.iter().enumerate() {
            let line = ass
                .lines()
                .filter(|l| l.starts_with("Dialogue:"))
                .nth(i)
                .unwrap_or_else(|| panic!("cue {i} missing"));
            let sum: i64 = sweeps(line).into_iter().sum();
            let span_cs = (s.end_ms as i64 - s.start_ms as i64).div_euclid(10);
            assert!(
                (sum - span_cs).abs() <= 1,
                "cue {i}: 扫光合计 {sum}cs 与行时长 {span_cs}cs 对不上"
            );
        }
    }

    /// A word gap has to survive as a hard space, or the player wraps the line
    /// where the transcript has a space.
    #[test]
    fn word_gaps_become_hard_spaces() {
        let ass = to_ass(&sentences_of("Hello world"), &KaraokeStyle::default());
        let body = ass.lines().find(|l| l.starts_with("Dialogue:")).unwrap();
        assert_eq!(body.matches("\\h").count(), 1, "恰好一个词间空格");
        assert!(!body.contains("} "), "不能有普通空格：普通空格是换行点");
    }

    #[test]
    fn the_header_is_a_complete_ass_document() {
        let ass = to_ass(&sentences_of("测试"), &KaraokeStyle::default());
        for section in [
            "[Script Info]",
            "[V4+ Styles]",
            "[Events]",
            "PlayResX: 1920",
        ] {
            assert!(ass.contains(section), "缺少 {section}");
        }
        assert_eq!(ass.matches("Dialogue:").count(), 1);
    }

    /// A unit with no room of its own — an interpolated character whose
    /// neighbours share a frame, a mark that opened its own word — must not
    /// become a `\k0`.
    ///
    /// It would be the obvious thing to write, and it is the one that breaks: a
    /// zero sweep is not a zero-length highlight. mpv, libass and xy-VSFilter
    /// each draw it differently from a positive one, and the character comes out
    /// in the *unsung* colour, sometimes with its neighbours' metrics dropped —
    /// a punctuation mark that never turns amber, or a gap in the line. Emitting
    /// no tag lets it ride inside the sweep already running, which is the one
    /// rendering every player agrees on. The cost is that it lights up with its
    /// neighbour rather than on its own instant; the sweeps still add up, so
    /// nothing downstream drifts.
    #[test]
    fn a_unit_with_no_room_of_its_own_carries_no_sweep() {
        let cue = Cue {
            index: 1,
            start_ms: 0,
            end_ms: 1000,
            text: "A好B".into(),
        };
        let tokens = vec![
            WordTokenDto {
                start: 0.0,
                end: 0.5,
                word: "A".into(),
            },
            // Interpolated: its neighbours share a frame, so it has none of its own.
            WordTokenDto {
                start: 0.5,
                end: 0.5,
                word: "好".into(),
            },
            WordTokenDto {
                start: 0.5,
                end: 1.0,
                word: "B".into(),
            },
        ];
        let body = cue_body(&cue, &tokens);
        assert_eq!(body, "{\\k50}A好{\\k50}B", "零时长字搭在前一段扫光里");
        assert!(!body.contains("\\k0"), "绝不能出现零扫光 tag");
        let line = format!("Dialogue: 0,0:00:00.00,0:00:10.00,Karaoke,,0,0,0,,{body}");
        assert_eq!(
            sweeps(&line).into_iter().sum::<i64>(),
            100,
            "扫光合计仍等于整行"
        );
    }

    /// A cue with no duration at all is a different case, and it does not
    /// reach the file as a lie about the audio: the writer no longer stretches
    /// a collapsed cue to 1 ms, because 1 ms is not showable either. No unit
    /// inherits anything from it — a unit's sweep comes from the units around
    /// it — and an ASS line shows its text for the whole event regardless, so a
    /// collapsed line simply never appears.
    #[test]
    fn a_cue_with_no_duration_stays_collapsed() {
        let sentences = sentences_of("测试");
        let mut cues: Vec<Cue> = sentences
            .translation_sentences
            .iter()
            .map(|s| Cue {
                index: s.sentence_id,
                start_ms: s.start_ms,
                end_ms: s.start_ms, // collapsed
                text: s.text.clone(),
            })
            .collect();
        crate::timeline::enforce_monotonic(&mut cues);
        for c in &cues {
            assert_eq!(c.end_ms, c.start_ms, "零时长 cue 保持零时长");
        }
        let ass = to_ass(&sentences, &KaraokeStyle::default());
        assert!(!ass.contains("\\k0"), "没有任何单元拿到零扫光");
    }
}

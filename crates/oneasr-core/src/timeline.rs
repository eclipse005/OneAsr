//! The timeline: the line between **measuring** a subtitle and **showing** it.
//!
//! # Why this module exists
//!
//! Before it, five layers each decided something about time, all of it implicit:
//! the align stage added the chunk offset and rounded to milliseconds, token
//! merging extended an end, the layout DP regrouped which words belong to which
//! cue, the assembler took the cue's time from its first and last word, and the
//! SRT writer gave zero-length cues a 1 ms floor and clamped overlaps. None of
//! them wrong; the problem was that no single place could answer "who decided
//! this cue starts at 7.44 s".
//!
//! So the rules are written down once, here, and are the only ones:
//!
//! 1. **Words are placed once.** [`place_aligned`] and [`place_whole`] are the
//!    only two ways a [`WordToken`] comes into existence. Both round to
//!    milliseconds, because the file is written in milliseconds — rounding is
//!    not a display concern, it is the timeline's unit.
//! 2. **Segmentation may only select.** Choosing which words form a cue is an
//!    index range; the cue's time is read off the words at its ends
//!    ([`crate::sentence_boundary`]). No layer computes a timestamp of its own.
//! 3. **A cue list is made monotonic once**, by [`enforce_monotonic`], at the
//!    moment it is written out.
//! 4. **Text transforms may only remove** (see [`crate::subtitle::beautify`]),
//!    and the one that rewrites glyphs — Chinese script conversion — is a
//!    setting, not a default.
//!
//! # The two phases
//!
//! **Phase A — measure.** ASR + alignment + token normalization produce a
//! [`Timeline`]. It is written to `{stem}.timeline.json` next to the subtitle.
//! Every number in it is evidence: the aligner's own numbers, rounded once.
//! This phase is expensive and non-deterministic, so it runs once.
//!
//! **Phase B — present.** [`render`] turns a [`Timeline`] plus [`RenderOptions`]
//! into subtitle text. It is a pure function — no clock, no settings file, no
//! model, no disk — so changing the segment length, the output script or a
//! renderer costs a re-render in milliseconds instead of two model runs. That is
//! the whole point of the boundary, and `oneasr-cli render` is the proof: it
//! takes a timeline and options and nothing else.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::sentence_boundary::{
    SentenceBoundaryRequest, SourceSentences, WordTokenDto, build_sentences_from_transcript,
    build_source_sentences_from_words, source_sentences_to_srt, source_sentences_to_txt,
};
use crate::subtitle::ass::KaraokeStyle;
use crate::subtitle::segmenter::WordToken;
use crate::text_script::{self, TextScript};

/// One subtitle line's place on the timeline, in milliseconds.
///
/// Named for what it is rather than for the file it happens to be written to:
/// the karaoke renderer wants the same three numbers and a different box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cue {
    pub index: usize,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

/// Make a cue list monotonic, in place. The only place a cue time is adjusted
/// rather than read from a word.
///
/// One rule: each cue's end is clamped to the next cue's start, because aligner
/// jitter and the cue tiling at a shared boundary can otherwise overlap, and an
/// overlap in a subtitle file is two lines on screen at once. That is a real
/// visible fault, and it is the reason this function exists.
///
/// There is deliberately **no minimum-duration floor**. A cue with `end == start`
/// used to be stretched to 1 ms, on the theory that a zero-length cue is not a
/// subtitle a player can show — but 1 ms is not showable either, so the floor
/// bought nothing except a number in the file that does not describe the audio.
/// Strict SubRip validators do object to `end == start`; nothing else does, and
/// a line nobody can see is better than a line that claims to be 1 ms long. A
/// collapsed cue stays collapsed.
///
/// Note also what this does *not* do: it never moves a start, and it never
/// shortens a cue that fits. The audio's own boundaries are what they are.
pub fn enforce_monotonic(cues: &mut [Cue]) {
    for i in 0..cues.len() {
        if let Some(next_start) = cues.get(i + 1).map(|c| c.start_ms)
            && cues[i].end_ms > next_start
        {
            // `.max(start_ms)` keeps the clamp from producing `end < start` on
            // cues that were already out of order when they arrived here.
            cues[i].end_ms = next_start.max(cues[i].start_ms);
        }
    }
}

/// Round to milliseconds, the timeline's unit. The only rounding in the system:
/// rounding anywhere else would make the same instant read two ways.
pub fn round_millis(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

/// One word as the aligner reported it, times moved onto the file's timeline.
///
/// `rel_*` are seconds relative to the audio the aligner was given, which is one
/// chunk of the file; `chunk_start_sec` is where that chunk begins. Three
/// rules, all here and nowhere else: a relative time is never negative, an end
/// is never before its own start, and both land on a millisecond.
///
/// The end's floor is the *clamped* start, not the raw one. It used to be the
/// raw one, which only differed when the aligner reported a token before the
/// chunk began: the start was pulled up to zero while the end stayed at the
/// negative start, and the word came out with a negative duration. No aligner
/// does that today, which is exactly why it survived — the rule is now stated
/// where it can be read.
pub fn place_aligned(
    text: &str,
    chunk_start_sec: f64,
    rel_start_sec: f64,
    rel_end_sec: f64,
) -> WordToken {
    let start = rel_start_sec.max(0.0);
    place(
        text,
        chunk_start_sec + start,
        chunk_start_sec + rel_end_sec.max(start),
    )
}

/// A segment too short to align: the whole span becomes one word, with no model
/// call. Same rounding and same "end is never before start" rule as
/// [`place_aligned`] — a path that skips the model must not skip the timeline.
pub fn place_whole(text: &str, abs_start_sec: f64, abs_end_sec: f64) -> WordToken {
    place(text, abs_start_sec, abs_end_sec.max(abs_start_sec))
}

fn place(text: &str, abs_start_sec: f64, abs_end_sec: f64) -> WordToken {
    WordToken {
        start: round_millis(abs_start_sec),
        end: round_millis(abs_end_sec),
        word: text.to_string(),
    }
}

/// The measured subtitle: every word the models placed, and the context needed
/// to re-present them. Serialized next to the subtitle so Phase B can run
/// without them.
///
/// Times are seconds, exactly as the pipeline holds them. Milliseconds are the
/// *file's* unit, not this one's: storing rounded values here would mean the
/// timeline could disagree with the words it was made from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Timeline {
    /// Schema version. A reader that does not know this number must refuse the
    /// file rather than guess — a re-render that silently disagrees with the
    /// original is worse than an error.
    pub version: u32,
    /// Source media file name, for diagnostics and the task id.
    pub media: String,
    /// Short language key (`zh`, `en`, …) the segmentation profiles are keyed by.
    pub lang: String,
    /// The segment-length preset this was measured with — what `render` uses
    /// when the caller does not ask for a different one.
    pub preset: String,
    /// VAD speech spans, in seconds. Segmentation reads them for hard cuts at
    /// silence and for the gap measurements the layout DP costs, so a
    /// re-render without them would not be the same render.
    #[serde(default)]
    pub vad_speech_segments: Vec<(f64, f64)>,
    pub words: Vec<TimelineWord>,
}

/// One placed word. [`WordToken`] is the pipeline's working shape; this is the
/// one that crosses the boundary, so it carries its own field names and a
/// `version`-bearing schema rather than borrowing an internal type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineWord {
    pub text: String,
    /// Seconds from the start of the media.
    pub start: f64,
    pub end: f64,
}

impl Timeline {
    pub const VERSION: u32 = 1;

    pub fn from_words(
        media: impl Into<String>,
        lang: impl Into<String>,
        preset: impl Into<String>,
        vad_speech_segments: Vec<(f64, f64)>,
        words: Vec<WordToken>,
    ) -> Self {
        Self {
            version: Self::VERSION,
            media: media.into(),
            lang: lang.into(),
            preset: preset.into(),
            vad_speech_segments,
            words: words
                .into_iter()
                .map(|w| TimelineWord {
                    text: w.word,
                    start: w.start,
                    end: w.end,
                })
                .collect(),
        }
    }

    pub fn to_words(&self) -> Vec<WordToken> {
        self.words
            .iter()
            .map(|w| WordToken {
                start: w.start,
                end: w.end,
                word: w.text.clone(),
            })
            .collect()
    }

    /// Refuse a file this build cannot read faithfully.
    pub fn ensure_supported(&self) -> Result<(), String> {
        if self.version != Self::VERSION {
            return Err(format!(
                "timeline version {} is not this build's ({})",
                self.version,
                Self::VERSION
            ));
        }
        if self.words.is_empty() {
            return Err("timeline has no words".to_string());
        }
        Ok(())
    }
}

/// What to present and how. All of it is a choice the caller makes at
/// presentation time — which is the whole reason this type is separate from
/// [`Timeline`]: re-presenting the same measurement differently must not require
/// measuring it again.
#[derive(Debug, Clone)]
pub struct RenderOptions {
    /// Overrides the timeline's own preset when set.
    pub preset: Option<String>,
    /// Chinese output script. `Original` leaves the recognised glyphs alone.
    ///
    /// Ignored when `transcript` is set — see [`Self::transcript`].
    pub script: TextScript,
    /// The transcript, when this subtitle was aligned to one instead of
    /// transcribed. Its **line breaks are the cue boundaries**; the layout DP
    /// does not run, because re-deriving the breaks would undo a decision the
    /// author just made on purpose.
    pub transcript: Option<String>,
    /// `None` = do not produce this format.
    pub srt: bool,
    pub txt: bool,
    /// Karaoke ASS: the same cues, a sweep per aligned unit.
    pub ass: bool,
}

/// The presented subtitle, and the sentences it came from.
pub struct Rendered {
    pub sentences: SourceSentences,
    pub srt: Option<String>,
    pub txt: Option<String>,
    pub ass: Option<String>,
}

/// Phase B: the measured timeline plus a choice of presentation, and nothing
/// else. No clock, no settings file, no model, no disk — so the same call twice
/// gives the same bytes, and a caller can re-present without re-measuring.
///
/// A `transcript` switches the boundary source: the transcript's own lines
/// become the cues, and `script` is ignored. That second half is deliberate
/// rather than a convention — "机器不碰你的字" has to hold even for a caller
/// that forgets, so it is enforced here instead of trusted.
pub fn render(timeline: &Timeline, opts: &RenderOptions) -> Result<Rendered, String> {
    timeline.ensure_supported()?;
    let stem = crate::paths::media_stem(std::path::Path::new(&timeline.media));
    let request = SentenceBoundaryRequest {
        task_id: stem,
        media_path: timeline.media.clone(),
        source_lang: timeline.lang.clone(),
        subtitle_length_preset: opts
            .preset
            .clone()
            .unwrap_or_else(|| timeline.preset.clone()),
        words: timeline
            .to_words()
            .into_iter()
            .map(|w| WordTokenDto {
                start: w.start,
                end: w.end,
                word: w.word,
            })
            .collect(),
        vad_speech_segments: timeline.vad_speech_segments.clone(),
    };
    let sentences = match opts.transcript.as_deref() {
        Some(text) => build_sentences_from_transcript(request, text)?,
        None => {
            let mut s = build_source_sentences_from_words(request)?;
            // Script conversion rewrites cue text only, after alignment and
            // segmentation, and only for a transcript the models produced.
            text_script::convert_sentences(&mut s, opts.script);
            s
        }
    };

    Ok(Rendered {
        srt: opts.srt.then(|| source_sentences_to_srt(&sentences)),
        txt: opts.txt.then(|| source_sentences_to_txt(&sentences)),
        ass: opts
            .ass
            .then(|| crate::subtitle::ass::to_ass(&sentences, &KaraokeStyle::default())),
        sentences,
    })
}

/// Read a timeline back, refusing anything this build cannot read faithfully.
pub fn read_timeline(path: &Path) -> Result<Timeline, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let timeline: Timeline =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    timeline.ensure_supported()?;
    Ok(timeline)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(text: &str, start: f64, end: f64) -> WordToken {
        WordToken {
            start,
            end,
            word: text.to_string(),
        }
    }

    fn timeline_of(words: Vec<WordToken>) -> Timeline {
        Timeline::from_words("clip.mp4", "zh", "standard", vec![(0.0, 60.0)], words)
    }

    fn all_options() -> RenderOptions {
        RenderOptions {
            preset: None,
            transcript: None,
            script: TextScript::Original,
            srt: true,
            txt: true,
            ass: false,
        }
    }

    #[test]
    fn place_aligned_offsets_clamps_and_rounds_once() {
        let w = place_aligned("好", 7.44, 0.0, 0.1234);
        assert_eq!((w.start, w.end), (7.44, 7.563));
        // A relative time before zero is not before the file.
        let w = place_aligned("好", 10.0, -0.5, -0.9);
        assert_eq!((w.start, w.end), (10.0, 10.0));
        // An end before its own start is not either.
        let w = place_aligned("好", 1.0, 0.5, 0.2);
        assert_eq!((w.start, w.end), (1.5, 1.5));
    }

    #[test]
    fn place_whole_applies_the_same_rounding() {
        // The no-model path must not be a path that skips the timeline rules.
        let whole = place_whole("短句", 0.1234, 0.2);
        let aligned = place_aligned("短句", 0.1234, 0.0, 0.0766);
        assert_eq!(
            (whole.start, whole.end, whole.word.as_str()),
            (aligned.start, aligned.end, aligned.word.as_str())
        );
        assert_eq!(place_whole("短句", 1.0, 0.5).end, 1.0);
    }

    #[test]
    fn monotonic_clamps_overlaps_and_leaves_the_rest_alone() {
        // An overlap is cut at the next start, and no start is ever moved.
        let mut cues = vec![
            Cue {
                index: 1,
                start_ms: 1000,
                end_ms: 2000,
                text: "a".into(),
            },
            Cue {
                index: 2,
                start_ms: 1500,
                end_ms: 3000,
                text: "b".into(),
            },
        ];
        enforce_monotonic(&mut cues);
        assert_eq!((cues[0].start_ms, cues[0].end_ms), (1000, 1500));
        assert_eq!((cues[1].start_ms, cues[1].end_ms), (1500, 3000));

        // A collapsed cue stays collapsed: 1 ms is not showable either, so
        // padding it would only put a number in the file that the audio does
        // not support.
        let mut flat = vec![
            Cue {
                index: 1,
                start_ms: 2000,
                end_ms: 2000,
                text: "a".into(),
            },
            Cue {
                index: 2,
                start_ms: 3000,
                end_ms: 4000,
                text: "b".into(),
            },
        ];
        enforce_monotonic(&mut flat);
        assert_eq!((flat[0].start_ms, flat[0].end_ms), (2000, 2000));
        assert_eq!((flat[1].start_ms, flat[1].end_ms), (3000, 4000));
    }

    #[test]
    fn timeline_round_trips_without_drift() {
        // Phase B re-reads these numbers; a value that moved in the file would
        // make a re-render disagree with the subtitle it claims to re-present.
        let tl = timeline_of(vec![word("好", 0.123456, 1.987654)]);
        let json = serde_json::to_string(&tl).unwrap();
        let back: Timeline = serde_json::from_str(&json).unwrap();
        assert_eq!(back.words[0].start, tl.words[0].start);
        assert_eq!(back.words[0].end, tl.words[0].end);
        assert_eq!(back.words[0].text, "好");
    }

    #[test]
    fn a_foreign_version_is_refused_rather_than_guessed() {
        let mut tl = timeline_of(vec![word("好", 0.0, 1.0)]);
        tl.version = Timeline::VERSION + 1;
        assert!(tl.ensure_supported().is_err());
        assert!(render(&tl, &all_options()).is_err());
    }

    /// The removal-only invariant, checked where it can actually be broken:
    /// across the whole presentation path. Whatever segmentation does to the
    /// text, no character may appear that the alignment did not produce.
    #[test]
    fn rendering_never_invents_a_character() {
        let words: Vec<WordToken> = "最近看到一些关于AI与语音识别，表现并不像可靠。Whisper 表现"
            .split_inclusive(|c: char| c.is_whitespace())
            .enumerate()
            .map(|(i, t)| {
                let start = i as f64 * 0.3;
                word(t.trim(), start, start + 0.25)
            })
            .collect();
        let tl = timeline_of(words.clone());
        let rendered = render(&tl, &all_options()).unwrap();
        let spoken: String = words.iter().flat_map(|w| w.word.chars()).collect();
        let shown: String = rendered
            .sentences
            .translation_sentences
            .iter()
            .flat_map(|s| s.text.chars())
            .filter(|c| !c.is_whitespace())
            .collect();
        assert_eq!(
            shown,
            spoken
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect::<String>(),
            "字幕里的每个字都必须来自对齐结果"
        );
        assert!(rendered.srt.as_deref().is_some_and(|s| !s.is_empty()));
        assert!(rendered.txt.as_deref().is_some_and(|t| !t.is_empty()));
    }

    /// Phase B is a pure function: the same timeline and the same options give
    /// the same bytes. This is what makes a re-render cheap instead of a
    /// second run of two models.
    #[test]
    fn rendering_is_deterministic_and_option_only() {
        let tl = timeline_of(
            "这是第一句，测试确定性。这是第二句，也一样。"
                .split_inclusive(|c: char| c.is_whitespace())
                .enumerate()
                .map(|(i, t)| {
                    let start = i as f64 * 0.4;
                    word(t.trim(), start, start + 0.35)
                })
                .collect(),
        );
        let a = render(&tl, &all_options()).unwrap();
        let b = render(&tl, &all_options()).unwrap();
        assert_eq!(a.srt, b.srt);
        assert_eq!(a.txt, b.txt);

        // Asking for no format at all is legal and empty, not an error.
        let none = RenderOptions {
            srt: false,
            txt: false,
            ..all_options()
        };
        let empty = render(&tl, &none).unwrap();
        assert!(empty.srt.is_none() && empty.txt.is_none());
    }

    /// Changing the segment length changes the presentation, not the words.
    /// This is the re-render paying for itself: today this costs two model runs.
    #[test]
    fn a_different_preset_reshapes_the_cues_without_touching_the_text() {
        let long: Vec<WordToken> = (0..24)
            .map(|i| {
                let start = i as f64 * 1.0;
                word(if i == 23 { "了" } else { "字" }, start, start + 0.9)
            })
            .collect();
        let tl = timeline_of(long);
        let loose = render(
            &tl,
            &RenderOptions {
                preset: Some("loose".into()),
                ..all_options()
            },
        )
        .unwrap();
        let tight = render(
            &tl,
            &RenderOptions {
                preset: Some("short".into()),
                ..all_options()
            },
        )
        .unwrap();
        assert!(
            tight.sentences.translation_sentences.len()
                > loose.sentences.translation_sentences.len(),
            "更短的预设应该切出更多条"
        );
        let letters = |r: &Rendered| -> usize {
            r.sentences
                .translation_sentences
                .iter()
                .map(|s| s.text.chars().filter(|c| !c.is_whitespace()).count())
                .sum()
        };
        assert_eq!(letters(&loose), letters(&tight), "切分不增删任何字");
    }
}

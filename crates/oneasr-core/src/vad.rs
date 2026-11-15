//! VAD-based chunk planning for long-audio ASR.
//!
//! Uses FireRedVAD (embedded Rust library, no external exe) to locate speech
//! segments, derives silence gaps, then plans continuous `[start, end)` chunks
//! that align to silence midpoints near a target duration
//! ([`crate::settings::CHUNK_TARGET_MIN_SEC`]..=[`crate::settings::CHUNK_TARGET_MAX_SEC`]).
//!
//! If the lookback window holds no usable pause at all, the caller can hand in
//! the quietest point around the boundary instead of blind-cutting there
//! ([`plan_chunks_with_quiet`], same idea as the official Qwen3-ASR splitter).
//!
//! Cut placement mirrors the validated Python `asr.py` pipeline; any chunk
//! shorter than `MIN_TAIL_SEGMENT_SEC` is merged into a neighbor (VoxTrans
//! merges only the tail — see `merge_short_segments`).

use std::path::Path;

use fireredvad::{Vad, VadConfig};

use crate::asr::AsrError;

/// Tuning constants (seconds / frames).
const SILENCE_LOOKBACK: f32 = 30.0;
const MIN_SILENCE_PREFER: f32 = 0.5;
const MIN_SILENCE_FALLBACK: f32 = 0.3;
/// 最后一层：任何能测出的短停顿（换气）也比把切刀落进词中间好。
/// 调用方派生静音时用这个最宽的阈值，否则短停顿根本到不了 [`plan_chunks`]。
pub const MIN_SILENCE_MICRO: f32 = 0.12;
/// Any chunk shorter than this is absorbed into a neighbor so we avoid
/// tiny ASR/align windows (e.g. 1–5 s remainder after ~chunk_target splits,
/// or a sub-second sliver when the longest silence in the lookback window
/// sits right after the previous cut).
/// Max merged length is `chunk_target + just under this value`.
const MIN_TAIL_SEGMENT_SEC: f32 = 15.0;

/// 两条停顿算「一样长」的门槛（秒）。
///
/// 不能写成 1e-6：时长是 f32 秒，60 s 附近一个 ULP 就有 3.8e-6，同长度的
/// 停顿 `e - s` 会抖出 2~4e-6 的差。门槛压在噪声以下时，「一样长就取更靠
/// 目标的那条」时灵时不灵，退化成「取窗口里第一条」——实测 60 s 目标的
/// 头一刀落在 31.7 s（见 `equal_length_pauses_cut_near_target`）。
/// 1 ms 高于这个噪声、又低于 VAD 的 10 ms 帧精度，两档停顿不会被并成一档。
const SILENCE_DURATION_TIE_SEC: f32 = 1e-3;

/// A planned continuous audio range `[start, end)` (seconds, global timeline).
#[derive(Debug, Clone)]
pub struct Chunk {
    pub start: f32,
    pub end: f32,
}

/// VAD 判定“这里真的断开了”所需的最少静音帧数（10ms/帧，默认 20 = 200ms）。
/// 降到 10（100ms）后，`plan_chunks` 才有更多缝隙可选，尽量不盲切。
const VAD_MIN_SILENCE_FRAME: usize = 10;

fn vad_config() -> VadConfig {
    VadConfig {
        min_silence_frame: VAD_MIN_SILENCE_FRAME,
        ..VadConfig::default()
    }
}

/// Run FireRedVAD on a 16 kHz mono WAV and return speech segments.
///
/// Each pair is `(start_sec, end_sec)`. Model weights are embedded in the
/// FireRedVad crate, so no external model files are needed.
pub fn run_vad(wav_path: &Path) -> Result<Vec<(f32, f32)>, AsrError> {
    let vad = Vad::new().map_err(|e| AsrError::Other(format!("VAD init failed: {e}")))?;
    let out = vad
        .detect_wav(wav_path, &vad_config())
        .map_err(|e| AsrError::Other(format!("VAD detect failed: {e}")))?;
    Ok(out.timestamps)
}

/// Derive silence gaps from VAD speech intervals.
///
/// Only silences with length >= `min_silence` are kept as cut candidates.
pub fn speech_to_silences(
    speech: &[(f32, f32)],
    duration: f32,
    min_silence: f32,
) -> Vec<(f32, f32)> {
    let mut silences = Vec::new();
    let mut t = 0.0_f32;
    for &(s, e) in speech {
        if s > t && (s - t) >= min_silence {
            silences.push((t, s));
        }
        t = t.max(e);
    }
    if duration > t && (duration - t) >= min_silence {
        silences.push((t, duration));
    }
    silences
}

/// Pick the best silence in `(window_start, target]`: longest, then closest to target.
/// Returns the midpoint of the chosen silence, or `None` if no silence qualifies.
fn pick_silence_cut(
    silences: &[(f32, f32)],
    prev: f32,
    target: f32,
    window_start: f32,
    min_silence: f32,
) -> Option<f32> {
    let mut best: Option<(f32, f32)> = None; // (sil_dur, mid)
    for &(s, e) in silences {
        let sil_dur = e - s;
        if sil_dur < min_silence - 1e-6 {
            continue;
        }
        let full_mid = 0.5 * (s + e);
        let mid = if window_start < full_mid && full_mid <= target {
            full_mid
        } else {
            let left = s.max(window_start);
            let right = e.min(target);
            if right - left < min_silence - 1e-6 {
                continue;
            }
            0.5 * (left + right)
        };
        if mid <= prev || mid > target {
            continue;
        }
        if let Some((bd, best_mid)) = best {
            if sil_dur > bd + SILENCE_DURATION_TIE_SEC
                || (abs_diff(sil_dur, bd) <= SILENCE_DURATION_TIE_SEC && mid > best_mid)
            {
                best = Some((sil_dur, mid));
            }
        } else {
            best = Some((sil_dur, mid));
        }
    }
    best.map(|(_, mid)| mid)
}

#[inline]
fn abs_diff(a: f32, b: f32) -> f32 {
    (a - b).abs()
}

/// 兜底切点的搜索半窗口（秒）：VAD 一条合格停顿都没找到时，在目标点两侧
/// 这么大范围里找能量最低的点。量级同官方 Qwen3-ASR 的 `search_expand_sec`（5 s）。
pub const QUIET_SEARCH_HALF_SEC: f32 = 5.0;

/// Plan continuous `[start, end)` ranges covering the full timeline.
///
/// Near each ~`chunk_sec` boundary, look left within `SILENCE_LOOKBACK` (~30 s)
/// for the best pause: prefer >= 0.5 s, then >= 0.3 s, else a cut at the quietest
/// point around the nominal boundary ([`plan_chunks_with_quiet`]; this entry
/// point has no audio to look at, so it hard-cuts). Among matches, prefer the
/// longest pause, then the one closest to the boundary.
/// Cut at silence midpoint.
///
/// After cuts are built, any chunk shorter than `MIN_TAIL_SEGMENT_SEC` is
/// merged into a neighbor (any position, not just the tail — the
/// longest-silence rule can otherwise cut a sub-second sliver mid-stream).
/// Coverage always remains continuous from `0` to `duration`.
pub fn plan_chunks(duration: f32, silences: &[(f32, f32)], chunk_sec: f32) -> Vec<Chunk> {
    plan_chunks_with_quiet(duration, silences, chunk_sec, &mut |_| None)
}

/// [`plan_chunks`] with the last-tier escape hatch: `quiet(around_sec)` returns
/// the energy minimum near a nominal boundary, in seconds.
///
/// The three silence tiers need **some** pause in the 30 s lookback window; a
/// speaker who talks straight through the target leaves none, and the blind cut
/// then lands wherever the nominal boundary falls — mid-word, on a syllable's
/// loudest sample. `quiet` answers "where is the waveform calmest around here"
/// (the product path uses [`crate::media::quiet_point_near`], same idea as the
/// official Qwen3-ASR splitter). `None` keeps the blind cut, so a caller with no
/// audio (and every unit test) behaves exactly as before.
///
/// The quiet point may sit up to [`QUIET_SEARCH_HALF_SEC`] on either side of the
/// nominal boundary (the official splitter does the same), so one chunk can run
/// `chunk_sec + QUIET_SEARCH_HALF_SEC` long on this tier only.
pub fn plan_chunks_with_quiet(
    duration: f32,
    silences: &[(f32, f32)],
    chunk_sec: f32,
    quiet: &mut dyn FnMut(f32) -> Option<f32>,
) -> Vec<Chunk> {
    if duration <= chunk_sec + 1e-3 {
        return vec![Chunk {
            start: 0.0,
            end: duration,
        }];
    }

    let prefer = MIN_SILENCE_PREFER.max(MIN_SILENCE_FALLBACK);
    let fallback = MIN_SILENCE_PREFER.min(MIN_SILENCE_FALLBACK);
    let mut tiers = vec![prefer];
    if fallback < prefer - 1e-9 {
        tiers.push(fallback);
    }
    // 第三层：短停顿。宁可切在换气处，也不盲切进词中间。
    if MIN_SILENCE_MICRO < fallback - 1e-9 {
        tiers.push(MIN_SILENCE_MICRO);
    }

    let mut cuts: Vec<f32> = vec![0.0];
    let mut target = chunk_sec;

    while target < duration - 1e-3 {
        let prev = *cuts.last().unwrap();
        let window_start = (prev + 1e-4).max(target - SILENCE_LOOKBACK.max(prefer));

        let mut cut: Option<f32> = None;
        for &thr in &tiers {
            if let Some(mid) = pick_silence_cut(silences, prev, target, window_start, thr) {
                cut = Some(mid);
                break;
            }
        }

        let next = match cut {
            Some(mid) if mid > prev + 0.05 => {
                target = mid + chunk_sec;
                mid
            }
            _ => {
                let hard = target.min(duration);
                if hard - prev < 0.05 {
                    break;
                }
                // 最后一层：连续讲了 30 s 没给出一条合格停顿。切在目标点附近
                // **能量最低**的地方，而不是盲切在目标点上——盲切会落在半个字
                // 中间，两边各丢半截。读不到音频（`None`）就还是盲切。
                let quiet_cut = quiet(hard).map(|c| c.clamp(prev + 0.05, duration));
                let next_cut = match quiet_cut {
                    Some(c) if c > prev + 0.05 => c,
                    _ => hard,
                };
                target = next_cut + chunk_sec;
                next_cut
            }
        };
        cuts.push(next);
    }

    if *cuts.last().unwrap() < duration - 1e-3 {
        cuts.push(duration);
    }

    let mut chunks: Vec<Chunk> = cuts
        .windows(2)
        .map(|w| Chunk {
            start: (w[0] * 1000.0).round() / 1000.0,
            end: (w[1] * 1000.0).round() / 1000.0,
        })
        .filter(|c| c.end - c.start > 0.05)
        .collect();
    merge_short_segments(&mut chunks);
    chunks
}

/// Fold every chunk shorter than [`MIN_TAIL_SEGMENT_SEC`] into a neighbor.
///
/// VoxTrans only merges the *tail*; we generalize to any position because
/// mid-stream micro-chunks are real: `pick_silence_cut` takes the *longest*
/// silence in the lookback window, and when that silence sits right after
/// the previous cut, the span between the two cuts can be well under a
/// second. Merging into the previous chunk (or the next one for a leading
/// micro-chunk) keeps all cuts on silence boundaries while guaranteeing no
/// tiny segment ever reaches ASR/alignment.
fn merge_short_segments(chunks: &mut Vec<Chunk>) {
    let mut i = 0;
    while i < chunks.len() && chunks.len() > 1 {
        if chunks[i].end - chunks[i].start >= MIN_TAIL_SEGMENT_SEC {
            i += 1;
            continue;
        }
        if i == 0 {
            chunks[1].start = chunks[0].start;
            chunks.remove(0);
        } else {
            chunks[i - 1].end = chunks[i].end;
            chunks.remove(i);
            i -= 1; // re-check: the merged chunk may still be short
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_audio_single_chunk() {
        let chunks = plan_chunks(90.0, &[], 180.0);
        assert_eq!(chunks.len(), 1);
        assert!((chunks[0].start - 0.0).abs() < 0.01);
        assert!((chunks[0].end - 90.0).abs() < 0.01);
    }

    #[test]
    fn long_audio_hard_cut() {
        // No silences → hard cut at 180s boundaries
        let chunks = plan_chunks(450.0, &[], 180.0);
        assert!(chunks.len() >= 2);
        assert!((chunks[0].start).abs() < 0.01);
        assert!((chunks[0].end - 180.0).abs() < 0.1);
    }

    #[test]
    fn silence_cut_preferred() {
        // 400s audio with a silence at 170s → should cut near there
        let silences = vec![(169.0, 171.0)];
        let chunks = plan_chunks(400.0, &silences, 180.0);
        assert!(chunks.len() >= 2);
        // First cut should be near 170 (silence midpoint), not 180
        assert!(chunks[0].end > 168.0 && chunks[0].end < 172.0);
    }

    #[test]
    fn speech_to_silences_basic() {
        let speech = vec![(0.0, 5.0), (8.0, 12.0)]; // gap 5→8
        let sil = speech_to_silences(&speech, 12.0, 0.3);
        assert_eq!(sil.len(), 1);
        assert!((sil[0].0 - 5.0).abs() < 0.01);
        assert!((sil[0].1 - 8.0).abs() < 0.01);
    }

    #[test]
    fn short_tail_under_15s_merges_into_previous() {
        // hard cuts at 180 with 194.9 total → tail 14.9s merges
        let chunks = plan_chunks(194.9, &[], 180.0);
        assert_eq!(chunks.len(), 1);
        assert!((chunks[0].start).abs() < 0.01);
        assert!((chunks[0].end - 194.9).abs() < 0.1);
    }

    #[test]
    fn tail_at_least_15s_stays_separate() {
        let chunks = plan_chunks(195.0, &[], 180.0);
        assert_eq!(chunks.len(), 2);
        assert!((chunks[0].end - 180.0).abs() < 0.1);
        assert!((chunks[1].end - 195.0).abs() < 0.1);
        assert!((chunks[1].end - chunks[1].start - 15.0).abs() < 0.1);
    }

    /// 断骨验证：目标点附近**只有一个 0.2s 的短停顿**（够不到 0.3s 兜底门槛）时，
    /// 切点应该落在这个换气处，而不是在 180.0 盲切。拿掉第三层后这个测试会变红。
    #[test]
    fn micro_pause_beats_blind_cut() {
        // 缺口 179.0–179.2（0.2s）：低于 0.3s 兜底、高于 0.12s 短停顿层，
        // 且中点在目标 180 之前——只有第三层能切到这里。
        let silences = vec![(179.0, 179.2)];
        let chunks = plan_chunks(450.0, &silences, 180.0);
        assert!(chunks.len() >= 2);
        let cut = chunks[0].end;
        assert!(
            cut > 178.9 && cut < 179.3,
            "切点没落在短停顿上，而是盲切了: {cut}"
        );
    }

    /// 断骨验证：**同长度**的停顿成串出现时，切点要落在最靠目标的那条上。
    ///
    /// 素材是等间隔的 0.6s 停顿（`speech_to_silences` 产出的时长会因 f32
    /// 抖动差 2~4e-6）。把 `SILENCE_DURATION_TIE_SEC` 改回 1e-6 之后，
    /// 「一样长就取更靠目标的那条」会被这点抖动打断，头一刀退到 31.7s，
    /// 本测试变红。
    #[test]
    fn equal_length_pauses_cut_near_target() {
        let mut speech = Vec::new();
        let mut t = 0.0_f32;
        while t + 8.0 < 300.0 {
            speech.push((t, t + 7.4));
            t += 8.0;
        }
        let silences = speech_to_silences(&speech, 300.0, MIN_SILENCE_MICRO);
        let chunks = plan_chunks(300.0, &silences, 60.0);
        assert!(
            chunks[0].end > 55.0 && chunks[0].end <= 60.0,
            "同长度停顿里没选最靠目标的那条，切在 {}",
            chunks[0].end
        );
    }

    /// 反向约束：等长门槛不能宽到把**真的不一样长**的停顿算成一档。
    ///
    /// 目标前 2s 是 0.5s 的停顿，更靠左是 0.7s 的：取长的那条。
    /// 把 `SILENCE_DURATION_TIE_SEC` 放大到 0.5 就会改选近的这条，本测试变红。
    #[test]
    fn longer_pause_beats_closer_pause() {
        let silences = vec![(50.0, 50.7), (58.0, 58.5)];
        let chunks = plan_chunks(450.0, &silences, 60.0);
        let cut = chunks[0].end;
        assert!((cut - 50.35).abs() < 0.05, "更长的停顿没被选中，切在 {cut}");
    }

    /// 断骨验证：一条合格停顿都找不到时，兜底切点必须用 `quiet` 给的能量点，
    /// 而不是盲切在目标点上。`quiet` 返回 `None`（或拿掉这根线）时头一刀就回到
    /// 60.0，本测试变红。
    #[test]
    fn hard_cut_uses_quiet_point_when_no_pause_exists() {
        let mut quiet = |around: f32| Some(around - 1.2);
        let chunks = plan_chunks_with_quiet(300.0, &[], 60.0, &mut quiet);
        assert!(
            (chunks[0].end - 58.8).abs() < 0.05,
            "能量点没被用上，切在 {}",
            chunks[0].end
        );
        // 覆盖依然连续到结尾。
        let last = chunks.last().unwrap();
        assert!((last.end - 300.0).abs() < 0.01);
    }

    /// 反向约束：**块长上限不因短停顿层而放宽**。
    #[test]
    fn micro_pause_does_not_widen_the_cap() {
        // 180 之后才有 0.2s 缺口：目标点之前无可切点 → 仍然在 180.0 切
        let silences = vec![(185.0, 185.2)];
        let chunks = plan_chunks(450.0, &silences, 180.0);
        assert!(
            (chunks[0].end - 180.0).abs() < 0.1,
            "上限被拖走了: {}",
            chunks[0].end
        );
    }

    #[test]
    fn merge_short_tail_direct() {
        let mut chunks = vec![
            Chunk {
                start: 0.0,
                end: 60.0,
            },
            Chunk {
                start: 60.0,
                end: 120.0,
            },
            Chunk {
                start: 120.0,
                end: 125.0,
            },
        ];
        merge_short_segments(&mut chunks);
        assert_eq!(chunks.len(), 2);
        assert!((chunks[1].end - 125.0).abs() < 1e-6);
        assert!((chunks[1].end - chunks[1].start - 65.0).abs() < 1e-6);
    }

    #[test]
    fn merge_midstream_micro_chunk_into_previous() {
        // Real case (Trump/Zelensky video, 30s target): cuts at 84.25 and
        // 84.46 — the longest silence in the lookback window sat right after
        // the previous cut, producing a 0.21s sliver mid-stream.
        let mut chunks = vec![
            Chunk {
                start: 60.0,
                end: 84.25,
            },
            Chunk {
                start: 84.25,
                end: 84.46,
            },
            Chunk {
                start: 84.46,
                end: 114.46,
            },
        ];
        merge_short_segments(&mut chunks);
        assert_eq!(chunks.len(), 2);
        assert!((chunks[0].start - 60.0).abs() < 1e-6);
        assert!((chunks[0].end - 84.46).abs() < 1e-6);
        assert!((chunks[1].start - 84.46).abs() < 1e-6);
        assert!((chunks[1].end - 114.46).abs() < 1e-6);
    }

    #[test]
    fn merge_leading_micro_chunk_into_next() {
        let mut chunks = vec![
            Chunk {
                start: 0.0,
                end: 0.3,
            },
            Chunk {
                start: 0.3,
                end: 30.0,
            },
        ];
        merge_short_segments(&mut chunks);
        assert_eq!(chunks.len(), 1);
        assert!((chunks[0].start).abs() < 1e-6);
        assert!((chunks[0].end - 30.0).abs() < 1e-6);
    }

    #[test]
    fn plan_chunks_never_emits_micro_chunks() {
        // Long silence right after a cut + shorter silence near the target:
        // the longest-silence rule picks the early one, but the merge pass
        // must absorb the sliver. Chunk lengths (except single-chunk audio)
        // stay >= MIN_TAIL_SEGMENT_SEC.
        let silences = vec![(84.3, 86.0), (113.5, 114.0), (143.6, 144.2)];
        let chunks = plan_chunks(160.0, &silences, 30.0);
        assert!(chunks.len() >= 2);
        for c in &chunks {
            assert!(
                c.end - c.start >= MIN_TAIL_SEGMENT_SEC - 0.01,
                "micro chunk slipped through: {:.2}-{:.2}",
                c.start,
                c.end
            );
        }
        // Continuous full coverage.
        assert!((chunks[0].start).abs() < 0.01);
        assert!((chunks.last().unwrap().end - 160.0).abs() < 0.01);
        for w in chunks.windows(2) {
            assert!((w[0].end - w[1].start).abs() < 1e-6);
        }
    }

    #[test]
    fn silence_cut_then_short_tail_merges() {
        // Silence midpoint ~170 → first cut there; duration 184 → tail 14s < 15.
        let silences = vec![(169.0, 171.0)];
        let chunks = plan_chunks(184.0, &silences, 180.0);
        assert_eq!(chunks.len(), 1, "short tail after silence cut must merge");
        assert!((chunks[0].start).abs() < 0.01);
        assert!((chunks[0].end - 184.0).abs() < 0.1);
        // Continuous full coverage.
        assert!(chunks[0].end > chunks[0].start);
    }

    #[test]
    fn silence_cut_tail_at_least_15s_stays() {
        // Cut at ~170, duration 185 → tail 15s stays as its own chunk.
        let silences = vec![(169.0, 171.0)];
        let chunks = plan_chunks(185.0, &silences, 180.0);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].end > 168.0 && chunks[0].end < 172.0);
        assert!((chunks[1].end - 185.0).abs() < 0.1);
        assert!((chunks[1].end - chunks[1].start - 15.0).abs() < 0.15);
    }
}

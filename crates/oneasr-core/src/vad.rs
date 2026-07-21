//! VAD-based chunk planning for long-audio ASR.
//!
//! Uses FireRedVAD (embedded Rust library, no external exe) to locate speech
//! segments, derives silence gaps, then plans continuous `[start, end)` chunks
//! that align to silence midpoints near a target duration
//! ([`crate::settings::CHUNK_TARGET_MIN_SEC`]..=[`crate::settings::CHUNK_TARGET_MAX_SEC`]).
//!
//! Cut placement mirrors the validated Python `asr.py` pipeline; short-tail
//! merge matches VoxTrans ([`MIN_TAIL_SEGMENT_SEC`]).

use std::path::Path;

use fireredvad::{Vad, VadConfig};

use crate::asr::AsrError;

/// Tuning constants (seconds / frames).
const SILENCE_LOOKBACK: f32 = 30.0;
const MIN_SILENCE_PREFER: f32 = 0.5;
const MIN_SILENCE_FALLBACK: f32 = 0.3;
/// Tail shorter than this is absorbed into the previous chunk so we avoid
/// tiny ASR/align windows (e.g. 1–5 s remainder after ~chunk_target splits).
/// Max merged length is `chunk_target + just under this value`.
const MIN_TAIL_SEGMENT_SEC: f32 = 15.0;

/// A planned continuous audio range `[start, end)` (seconds, global timeline).
#[derive(Debug, Clone)]
pub struct Chunk {
    pub start: f32,
    pub end: f32,
}

/// Run FireRedVAD on a 16 kHz mono WAV and return speech segments.
///
/// Each pair is `(start_sec, end_sec)`. Model weights are embedded in the
/// FireRedVad crate, so no external model files are needed.
pub fn run_vad(wav_path: &Path) -> Result<Vec<(f32, f32)>, AsrError> {
    let vad = Vad::new().map_err(|e| AsrError::Msg(format!("VAD init failed: {e}")))?;
    let out = vad
        .detect_wav(wav_path, &VadConfig::default())
        .map_err(|e| AsrError::Msg(format!("VAD detect failed: {e}")))?;
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
fn pick_silence_cut(
    silences: &[(f32, f32)],
    prev: f32,
    target: f32,
    window_start: f32,
    min_silence: f32,
) -> Option<(f32, f32)> {
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
        if let Some((bd, _bm)) = best {
            if sil_dur > bd + 1e-6 || (abs_diff(sil_dur, bd) <= 1e-6 && mid > _bm) {
                best = Some((sil_dur, mid));
            }
        } else {
            best = Some((sil_dur, mid));
        }
    }
    // best is (sil_dur, mid); caller wants mid only
    best.map(|(_, mid)| mid).map(|m| (m, m))
}

#[inline]
fn abs_diff(a: f32, b: f32) -> f32 {
    (a - b).abs()
}

/// Plan continuous `[start, end)` ranges covering the full timeline.
///
/// Near each ~`chunk_sec` boundary, look left within `SILENCE_LOOKBACK` (~30 s):
/// 1) prefer silences >= 0.5 s,
/// 2) else silences >= 0.3 s,
/// 3) else hard-cut at the nominal boundary.
/// Among matches, prefer the longest pause, then closest to the boundary.
/// Cut at silence midpoint.
///
/// After cuts are built, if the final chunk is shorter than
/// [`MIN_TAIL_SEGMENT_SEC`], it is merged into the previous chunk (same rule as
/// VoxTrans). Coverage always remains continuous from `0` to `duration`.
pub fn plan_chunks(duration: f32, silences: &[(f32, f32)], chunk_sec: f32) -> Vec<Chunk> {
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

    let mut cuts: Vec<f32> = vec![0.0];
    let mut target = chunk_sec;

    while target < duration - 1e-3 {
        let prev = *cuts.last().unwrap();
        let window_start = (prev + 1e-4).max(target - SILENCE_LOOKBACK.max(prefer));

        let mut cut: Option<f32> = None;
        for &thr in &tiers {
            if let Some((mid, _)) = pick_silence_cut(silences, prev, target, window_start, thr) {
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
                target = hard + chunk_sec;
                hard
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
    merge_short_tail(&mut chunks);
    chunks
}

/// If the last chunk is shorter than [`MIN_TAIL_SEGMENT_SEC`], fold it into
/// the previous one (same rule as VoxTrans).
fn merge_short_tail(chunks: &mut Vec<Chunk>) {
    let n = chunks.len();
    if n < 2 {
        return;
    }
    let last_end = chunks[n - 1].end;
    let last_start = chunks[n - 1].start;
    if last_end - last_start >= MIN_TAIL_SEGMENT_SEC {
        return;
    }
    chunks[n - 2].end = last_end;
    chunks.pop();
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
        merge_short_tail(&mut chunks);
        assert_eq!(chunks.len(), 2);
        assert!((chunks[1].end - 125.0).abs() < 1e-6);
        assert!((chunks[1].end - chunks[1].start - 65.0).abs() < 1e-6);
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

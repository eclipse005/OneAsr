//! UI feedback sounds (Windows PlaySound from embedded WAV; no-op elsewhere).
//!
//! Two clips, two roles — `include_bytes!` via [`crate::assets`]:
//!
//! | Clip | Role | Target |
//! |------|------|--------|
//! | [`Sfx::Click`] | generic interaction tap | -14 dBFS |
//! | [`Sfx::Reminder`] | the whole run finished | -6 dBFS |
//!
//! [`Sfx::Click`] is the app's **original settings sound**, reused as the single
//! interaction tap instead of bolting on a second flavour of click.
//!
//! **Two sounds, on purpose.** An earlier build carried four (click / delete /
//! task-error / task-done), so removing a row and a failed row each announced
//! themselves. Those are gone. Removal now taps like any other interaction, and
//! the run's outcome is carried by the hint's colour and wording rather than by
//! a second chime family — success and failure both end with [`Sfx::Reminder`].
//! The reminder answers exactly one question: *"is the batch done?"*
//!
//! **What earns a sound.** The line is *"did this change the queue, or commit to
//! disk?"* — not *"did the user click something?"*:
//!
//! - Yes: add / drop files, remove a task, clear the list, start a batch, the
//!   row-level separation toggle, the gear (navigation), an explicit save.
//! - No: toggles inside the settings drawer (instantly visible, freely
//!   repeatable, and only the *default* for future tasks), hover, scroll,
//!   selection, and closing the drawer (a cancel, not a commit).
//!
//! One sound per operation, never one per item: dropping 20 files or clearing a
//! 20-row list must not stutter. The reminder follows the same rule — it fires
//! once when the run drains (see `end_batch_if_idle` in `main.rs`), never per row.
//!
//! **Levels live in the files, not in code.** `PlaySound` has no gain parameter,
//! so each clip is peak-normalised to its role's target and must stay that way.
//! Never add a runtime volume knob: the only lever is re-cutting the WAV.
//!
//! Windows: a single serial worker owns all `PlaySound` calls (API is process-global
//! and not concurrent-safe). Rapid clicks replace the previous clip cleanly.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::assets::asset_bytes;

#[derive(Clone, Copy)]
pub enum Sfx {
    /// Interaction tap: buttons, list actions, and the settings drawer.
    Click,
    /// The run finished — one chime for the whole batch, success or failure.
    Reminder,
}

/// Unix-millisecond deadline before which interaction taps stay silent.
///
/// The reminder is ~3.7 s long, and `PlaySound` is a process-global single
/// channel: any tap during it would `SND_PURGE` the reminder away. That is
/// exactly the case the reminder exists for — the user stepped away to another
/// machine and is *not* clicking here. So the reminder owns the channel while it
/// plays and taps step aside. The behaviour reads as correct anyway: an alarm
/// should not have button blips on top of it.
static TAP_MUTE_UNTIL_MS: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn play(kind: Sfx) {
    let (path, is_reminder) = match kind {
        Sfx::Click => ("sounds/click.wav", false),
        Sfx::Reminder => ("sounds/reminder.wav", true),
    };
    if !is_reminder && now_ms() < TAP_MUTE_UNTIL_MS.load(Ordering::Relaxed) {
        return;
    }
    let Some(bytes) = asset_bytes(path) else {
        return;
    };
    if is_reminder {
        // Hold the channel for the clip's full length plus a short tail, so the
        // final decay is not clipped by a tap landing on the last few frames.
        TAP_MUTE_UNTIL_MS.store(now_ms() + wav_duration_ms(bytes) + 150, Ordering::Relaxed);
    }
    play_wav_memory(bytes);
}

/// Length of an embedded PCM WAV in milliseconds, read straight from the RIFF
/// chunks.
///
/// A hardcoded duration would silently rot the day the clip is re-cut; the
/// header already knows. Returns 0 when the header is not recognised, which
/// simply means "do not hold the channel" — never a panic, never a wrong mute.
fn wav_duration_ms(bytes: &[u8]) -> u64 {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return 0;
    }
    let u32_at = |i: usize| -> u32 {
        u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]])
    };

    let mut pos = 12usize;
    let mut byte_rate = 0u32;
    while pos + 8 <= bytes.len() {
        let size = u32_at(pos + 4) as usize;
        let body = pos + 8;
        if &bytes[pos..pos + 4] == b"fmt " && size >= 16 && body + 12 <= bytes.len() {
            // fmt layout: format(0..2) channels(2..4) sample_rate(4..8) byte_rate(8..12)
            byte_rate = u32_at(body + 8);
        } else if &bytes[pos..pos + 4] == b"data" {
            if byte_rate == 0 {
                return 0;
            }
            let payload = size.min(bytes.len().saturating_sub(body));
            return payload as u64 * 1000 / byte_rate as u64;
        }
        // Chunks are word-aligned; an odd size carries one pad byte.
        pos = body + size + (size & 1);
    }
    0
}

#[cfg(windows)]
fn play_wav_memory(bytes: &'static [u8]) {
    sfx_tx().send(SfxCmd::Play(bytes)).ok();
}

#[cfg(windows)]
enum SfxCmd {
    Play(&'static [u8]),
}

/// Lazily start one dedicated SFX thread; all playback is serialized there.
#[cfg(windows)]
fn sfx_tx() -> &'static std::sync::mpsc::Sender<SfxCmd> {
    use std::sync::mpsc;
    use std::sync::OnceLock;
    use std::thread;

    static TX: OnceLock<mpsc::Sender<SfxCmd>> = OnceLock::new();
    TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<SfxCmd>();
        thread::Builder::new()
            .name("oneasr-sfx".into())
            .spawn(move || {
                // SND_ASYNC | SND_MEMORY | SND_NODEFAULT
                const PLAY: u32 = 0x0001 | 0x0004 | 0x0002;
                // SND_PURGE — stop current async clip before starting the next.
                const PURGE: u32 = 0x0040;
                while let Ok(cmd) = rx.recv() {
                    match cmd {
                        SfxCmd::Play(bytes) => {
                            let _ = unsafe { play_sound_memory(std::ptr::null(), PURGE) };
                            let _ = unsafe { play_sound_memory(bytes.as_ptr(), PLAY) };
                        }
                    }
                }
            })
            .expect("spawn sfx worker");
        tx
    })
}

#[cfg(windows)]
unsafe fn play_sound_memory(data: *const u8, flags: u32) -> bool {
    #[link(name = "winmm")]
    unsafe extern "system" {
        fn PlaySoundA(
            psz_sound: *const u8,
            hmod: *mut core::ffi::c_void,
            fdw_sound: u32,
        ) -> i32;
    }
    unsafe { PlaySoundA(data, std::ptr::null_mut(), flags) != 0 }
}

#[cfg(not(windows))]
fn play_wav_memory(_bytes: &'static [u8]) {}

#[cfg(test)]
mod tests {
    use super::wav_duration_ms;

    /// A minimal 16-bit mono PCM WAV: `frames` frames at `rate` Hz.
    fn wav(frames: u32, rate: u32) -> Vec<u8> {
        let block_align = 2u32;
        let byte_rate = rate * block_align;
        let data_len = frames * block_align;
        let mut v = Vec::new();
        v.extend_from_slice(b"RIFF");
        v.extend_from_slice(&(36 + data_len).to_le_bytes());
        v.extend_from_slice(b"WAVE");
        v.extend_from_slice(b"fmt ");
        v.extend_from_slice(&16u32.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes()); // PCM
        v.extend_from_slice(&1u16.to_le_bytes()); // mono
        v.extend_from_slice(&rate.to_le_bytes());
        v.extend_from_slice(&byte_rate.to_le_bytes());
        v.extend_from_slice(&(block_align as u16).to_le_bytes());
        v.extend_from_slice(&16u16.to_le_bytes());
        v.extend_from_slice(b"data");
        v.extend_from_slice(&data_len.to_le_bytes());
        v.resize(v.len() + data_len as usize, 0);
        v
    }

    #[test]
    fn reads_duration_from_a_well_formed_header() {
        assert_eq!(wav_duration_ms(&wav(44_100, 44_100)), 1_000);
        // The shipped reminder: 3.748 s at 44.1 kHz stereo 16-bit.
        assert_eq!(wav_duration_ms(&wav(165_288, 44_100)), 3_748);
    }

    #[test]
    fn an_unrecognised_header_never_panics_and_never_mutes() {
        assert_eq!(wav_duration_ms(&[]), 0);
        assert_eq!(wav_duration_ms(b"not a wave file at all"), 0);
        // Truncated mid-header: stop, do not index past the end.
        assert_eq!(wav_duration_ms(&wav(1_000, 22_050)[..20]), 0);
        // A data chunk whose payload was clipped still reports what is there.
        let full = wav(44_100, 44_100);
        let clipped = wav_duration_ms(&full[..full.len() - 20_000]);
        assert!(
            clipped > 0 && clipped < 1_000,
            "truncated payload must report the surviving length, got {clipped}"
        );
    }

    #[test]
    fn odd_sized_chunks_are_word_aligned() {
        // A 5-byte LIST chunk sits between WAVE and fmt, padded to 6 bytes.
        // Reading past its real size would misparse `fmt ` and report 0.
        let base = wav(22_050, 22_050);
        let mut v = Vec::new();
        v.extend_from_slice(&base[..12]);
        v.extend_from_slice(b"LIST");
        v.extend_from_slice(&5u32.to_le_bytes());
        v.extend_from_slice(b"INFO\x00");
        v.push(0);
        v.extend_from_slice(&base[12..]);
        assert_eq!(wav_duration_ms(&v), 1_000);
    }
}

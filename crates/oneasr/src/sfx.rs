//! UI feedback sounds with one cross-platform audio backend.
//!
//! The application owns the sound policy (which interaction earns a sound and
//! how long a reminder reserves the channel). The backend only decodes the
//! embedded WAV and hands it to the operating system's native audio output:
//! WASAPI on Windows, CoreAudio on macOS, and ALSA on Linux (PipeWire desktop
//! systems expose it through their ALSA compatibility layer). No temporary files
//! or external player processes are needed.

use std::io::Cursor;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use rodio::cpal::traits::HostTrait;
use rodio::stream::MixerDeviceSink;
use rodio::{Decoder, DeviceSinkBuilder, DeviceTrait, Player};

use crate::{assets::asset_bytes, crashlog};

const IDLE_OUTPUT_TIMEOUT: Duration = Duration::from_secs(30);
const OUTPUT_RETRY_DELAY: Duration = Duration::from_secs(5);
const REMINDER_TAIL: Duration = Duration::from_millis(150);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sfx {
    /// Interaction tap: buttons, list actions, and the settings drawer.
    Click,
    /// The run finished — one chime for the whole batch, success or failure.
    Reminder,
}

#[derive(Clone)]
struct Inbox {
    inner: Arc<(Mutex<InboxState>, Condvar)>,
}

struct InboxState {
    pending: Option<Sfx>,
    stream_fault: Option<String>,
}

enum InboxEvent {
    Play(Sfx),
    StreamFault(String),
    Idle,
}

impl Inbox {
    fn new() -> Self {
        Self {
            inner: Arc::new((
                Mutex::new(InboxState {
                    pending: None,
                    stream_fault: None,
                }),
                Condvar::new(),
            )),
        }
    }

    /// Latest-wins mailbox: rapid clicks collapse, and a reminder always wins.
    fn submit(&self, next: Sfx) {
        let (lock, wake) = &*self.inner;
        let mut state = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        state.pending = Some(match (state.pending, next) {
            (Some(Sfx::Reminder), _) | (_, Sfx::Reminder) => Sfx::Reminder,
            (None, kind) => kind,
            (Some(Sfx::Click), Sfx::Click) => Sfx::Click,
        });
        wake.notify_one();
    }

    /// Called by the native audio callback; keep this path allocation-light and
    /// never do logging or filesystem I/O here.
    fn report_stream_fault(&self, message: String) {
        let (lock, wake) = &*self.inner;
        let mut state = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        state.stream_fault = Some(message);
        wake.notify_one();
    }

    fn next_event(&self, timeout: Duration) -> InboxEvent {
        let (lock, wake) = &*self.inner;
        let mut state = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        loop {
            if let Some(error) = state.stream_fault.take() {
                return InboxEvent::StreamFault(error);
            }
            if let Some(kind) = state.pending.take() {
                return InboxEvent::Play(kind);
            }

            let (next_state, wait_result) = wake
                .wait_timeout(state, timeout)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state = next_state;
            if wait_result.timed_out() {
                return InboxEvent::Idle;
            }
        }
    }
}

fn sfx_inbox() -> Option<Inbox> {
    static INBOX: OnceLock<Option<Inbox>> = OnceLock::new();
    INBOX
        .get_or_init(|| {
            let inbox = Inbox::new();
            let worker_inbox = inbox.clone();
            match std::thread::Builder::new()
                .name("oneasr-sfx".into())
                .spawn(move || run_audio_worker(worker_inbox))
            {
                Ok(_) => Some(inbox),
                Err(error) => {
                    crashlog::log_warn(format!("提示音线程启动失败：{error}"));
                    None
                }
            }
        })
        .clone()
}

pub fn play(kind: Sfx) {
    if let Some(inbox) = sfx_inbox() {
        inbox.submit(kind);
    }
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
    while pos.checked_add(8).is_some_and(|end| end <= bytes.len()) {
        let size = u32_at(pos + 4) as usize;
        let body = pos + 8;
        if &bytes[pos..pos + 4] == b"fmt "
            && size >= 16
            && body.checked_add(12).is_some_and(|end| end <= bytes.len())
        {
            // fmt layout: format(0..2) channels(2..4) sample_rate(4..8) byte_rate(8..12)
            byte_rate = u32_at(body + 8);
        } else if &bytes[pos..pos + 4] == b"data" {
            if byte_rate == 0 {
                return 0;
            }
            let payload = size.min(bytes.len().saturating_sub(body));
            return payload as u64 * 1000 / byte_rate as u64;
        }
        // Chunks are word-aligned; an odd size carries one pad byte. Checked
        // arithmetic keeps a malformed embedded header from wrapping around.
        let Some(next) = body
            .checked_add(size)
            .and_then(|value| value.checked_add(size & 1))
        else {
            return 0;
        };
        pos = next;
    }
    0
}

fn reminder_is_active(until: Option<Instant>, now: Instant) -> bool {
    until.is_some_and(|until| now < until)
}

fn should_suppress_click(kind: Sfx, reminder_until: Option<Instant>, now: Instant) -> bool {
    kind == Sfx::Click && reminder_is_active(reminder_until, now)
}

fn should_rebuild_output(error: &rodio::cpal::StreamError) -> bool {
    !matches!(error, rodio::cpal::StreamError::BufferUnderrun)
}

fn asset_for(kind: Sfx) -> Option<&'static [u8]> {
    let path = match kind {
        Sfx::Click => "sounds/click.wav",
        Sfx::Reminder => "sounds/reminder.wav",
    };
    asset_bytes(path)
}

/// Owns the native output stream for the lifetime of the worker. The sink must
/// outlive the current player; dropping it stops playback immediately.
struct AudioOutput {
    // Fields drop in declaration order: stop the player before the sink.
    current: Option<Player>,
    sink: MixerDeviceSink,
    device_id: rodio::cpal::DeviceId,
}

impl AudioOutput {
    fn open(inbox: &Inbox) -> Result<Self, String> {
        let device = rodio::cpal::default_host()
            .default_output_device()
            .ok_or_else(|| "没有默认音频输出设备".to_string())?;
        let device_id = device.id().map_err(|error| error.to_string())?;
        let callback_inbox = inbox.clone();
        let mut sink = DeviceSinkBuilder::from_device(device)
            .map_err(|error| error.to_string())?
            .with_buffer_size(rodio::cpal::BufferSize::Fixed(2048))
            .with_error_callback(move |error| {
                if should_rebuild_output(&error) {
                    callback_inbox.report_stream_fault(error.to_string());
                }
            })
            .open_sink_or_fallback()
            .map_err(|error| error.to_string())?;
        // A normal worker shutdown is not an application error; do not print
        // rodio's diagnostic drop message into the GUI's stderr.
        sink.log_on_drop(false);
        Ok(Self {
            current: None,
            sink,
            device_id,
        })
    }

    fn default_device_changed(&self) -> bool {
        rodio::cpal::default_host()
            .default_output_device()
            .and_then(|device| device.id().ok())
            .is_some_and(|device_id| device_id != self.device_id)
    }

    fn replace(&mut self, bytes: &'static [u8]) -> Result<(), rodio::decoder::DecoderError> {
        let source = Decoder::new_wav(Cursor::new(bytes))?;
        if let Some(previous) = self.current.take() {
            previous.stop();
        }
        let player = Player::connect_new(self.sink.mixer());
        player.append(source);
        self.current = Some(player);
        Ok(())
    }
}

struct AudioWorker {
    inbox: Inbox,
    output: Option<AudioOutput>,
    reminder_until: Option<Instant>,
    retry_after: Option<Instant>,
    retry_pending: Option<Sfx>,
    outage_reported: bool,
    decode_error_reported: bool,
}

impl AudioWorker {
    fn new(inbox: Inbox) -> Self {
        Self {
            inbox,
            output: None,
            reminder_until: None,
            retry_after: None,
            retry_pending: None,
            outage_reported: false,
            decode_error_reported: false,
        }
    }

    fn play(&mut self, kind: Sfx) {
        let now = Instant::now();
        if should_suppress_click(kind, self.reminder_until, now) {
            return;
        }
        if kind == Sfx::Click && self.retry_after.is_some_and(|retry_at| now < retry_at) {
            return;
        }
        if self
            .output
            .as_ref()
            .is_some_and(AudioOutput::default_device_changed)
        {
            self.output = None;
        }
        self.ensure_output();
        let Some(output) = self.output.as_mut() else {
            if kind == Sfx::Reminder {
                self.retry_pending = Some(kind);
            }
            return;
        };
        self.retry_pending = None;

        let Some(bytes) = asset_for(kind) else {
            return;
        };
        if let Err(error) = output.replace(bytes) {
            if !self.decode_error_reported {
                crashlog::log_warn(format!("内置提示音解码失败：{error}"));
                self.decode_error_reported = true;
            }
            return;
        }
        if kind == Sfx::Reminder {
            self.reminder_until = Some(
                Instant::now() + Duration::from_millis(wav_duration_ms(bytes)) + REMINDER_TAIL,
            );
        }
    }

    fn ensure_output(&mut self) {
        if self.output.is_some() {
            return;
        }
        match AudioOutput::open(&self.inbox) {
            Ok(output) => {
                self.output = Some(output);
                self.retry_after = None;
                if self.outage_reported {
                    crashlog::log_info("提示音音频输出已恢复");
                    self.outage_reported = false;
                }
            }
            Err(error) => {
                self.retry_after = Some(Instant::now() + OUTPUT_RETRY_DELAY);
                if !self.outage_reported {
                    crashlog::log_warn(format!("提示音输出设备不可用：{error}"));
                    self.outage_reported = true;
                }
                // A reminder is important enough to retry immediately after a
                // normal click backoff; the worker still remains non-blocking.
            }
        }
    }

    fn handle_stream_fault(&mut self, error: String) {
        let reminder_was_active = reminder_is_active(self.reminder_until, Instant::now());
        self.output = None;
        self.reminder_until = None;
        self.retry_after = Some(Instant::now() + OUTPUT_RETRY_DELAY);
        if reminder_was_active {
            self.retry_pending = Some(Sfx::Reminder);
        }
        if !self.outage_reported {
            crashlog::log_warn(format!("提示音音频流中断：{error}"));
            self.outage_reported = true;
        }
    }

    fn release_idle_output(&mut self) {
        if self.retry_pending.is_none() {
            self.output = None;
            self.reminder_until = None;
        }
    }

    fn next_timeout(&self) -> Duration {
        let Some(retry_at) = self.retry_after else {
            return IDLE_OUTPUT_TIMEOUT;
        };
        let remaining = retry_at.saturating_duration_since(Instant::now());
        IDLE_OUTPUT_TIMEOUT.min(remaining)
    }

    fn retry_due(&mut self) -> Option<Sfx> {
        if self.retry_after.is_some_and(|retry_at| Instant::now() >= retry_at) {
            self.retry_after = None;
            self.retry_pending.take()
        } else {
            None
        }
    }
}

fn run_audio_worker(inbox: Inbox) {
    let mut worker = AudioWorker::new(inbox);
    loop {
        match worker.inbox.next_event(worker.next_timeout()) {
            InboxEvent::Play(kind) => worker.play(kind),
            InboxEvent::StreamFault(error) => worker.handle_stream_fault(error),
            InboxEvent::Idle => {
                if let Some(kind) = worker.retry_due() {
                    worker.play(kind);
                } else {
                    worker.release_idle_output();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::time::{Duration, Instant};

    use super::{
        Inbox, InboxEvent, Sfx, asset_for, reminder_is_active, should_rebuild_output,
        should_suppress_click, wav_duration_ms,
    };

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
        // Truncated mid-header: stop, do not index past its end.
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
    fn an_unknown_byte_rate_is_reported_as_zero_duration() {
        let mut bytes = wav(44_100, 44_100);
        bytes[28..32].copy_from_slice(&0u32.to_le_bytes());
        assert_eq!(wav_duration_ms(&bytes), 0);
    }

    #[test]
    fn odd_sized_chunks_are_word_aligned() {
        // A 5-byte LIST chunk sits between WAVE and fmt, padded to 6 bytes.
        // Reading past its real size would misparse `fmt ` and return 0.
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

    #[test]
    fn embedded_sfx_files_are_decodable_without_an_audio_device() {
        for kind in [Sfx::Click, Sfx::Reminder] {
            let bytes = asset_for(kind).expect("embedded SFX asset");
            assert!(
                rodio::Decoder::new_wav(Cursor::new(bytes)).is_ok(),
                "embedded SFX must decode: {kind:?}"
            );
        }
    }

    #[test]
    fn mailbox_coalesces_clicks_and_prioritizes_reminder() {
        let inbox = Inbox::new();
        inbox.submit(Sfx::Click);
        inbox.submit(Sfx::Click);
        assert!(matches!(
            inbox.next_event(Duration::ZERO),
            InboxEvent::Play(Sfx::Click)
        ));

        inbox.submit(Sfx::Click);
        inbox.submit(Sfx::Reminder);
        assert!(matches!(
            inbox.next_event(Duration::ZERO),
            InboxEvent::Play(Sfx::Reminder)
        ));

        inbox.submit(Sfx::Reminder);
        inbox.submit(Sfx::Click);
        assert!(matches!(
            inbox.next_event(Duration::ZERO),
            InboxEvent::Play(Sfx::Reminder)
        ));
    }

    #[test]
    fn reminder_mute_policy_is_time_based() {
        let now = Instant::now();
        assert!(should_suppress_click(
            Sfx::Click,
            Some(now + Duration::from_secs(1)),
            now
        ));
        assert!(!should_suppress_click(
            Sfx::Reminder,
            Some(now + Duration::from_secs(1)),
            now
        ));
        assert!(!should_suppress_click(Sfx::Click, None, now));
    }

    #[test]
    fn expired_reminder_is_not_active() {
        let now = Instant::now();
        let expired = now - Duration::from_millis(1);
        assert!(!reminder_is_active(Some(expired), now));
        assert!(!should_suppress_click(Sfx::Click, Some(expired), now));
    }

    #[test]
    fn buffer_underrun_is_recoverable_but_device_loss_is_not() {
        assert!(!should_rebuild_output(
            &rodio::cpal::StreamError::BufferUnderrun
        ));
        assert!(should_rebuild_output(
            &rodio::cpal::StreamError::DeviceNotAvailable
        ));
        assert!(should_rebuild_output(
            &rodio::cpal::StreamError::StreamInvalidated
        ));
    }

    #[test]
    #[ignore = "requires a local audio output device"]
    fn native_audio_output_can_open_and_play_an_embedded_sfx() {
        let inbox = Inbox::new();
        let mut output = super::AudioOutput::open(&inbox).expect("default audio output");
        output
            .replace(asset_for(Sfx::Click).expect("embedded click asset"))
            .expect("play embedded click");
    }
}

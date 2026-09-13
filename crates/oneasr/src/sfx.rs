//! UI feedback sounds (Windows PlaySound from embedded WAV; no-op elsewhere).
//!
//! Four clips, four roles — `include_bytes!` via [`crate::assets`]:
//!
//! | Clip | Role | Target |
//! |------|------|--------|
//! | [`Sfx::Click`] | generic interaction tap | -14 dBFS |
//! | [`Sfx::Delete`] | removing a task, clearing the list | -12 dBFS |
//! | [`Sfx::TaskError`] | a task failed | -8 dBFS |
//! | [`Sfx::TaskDone`] | a task finished | -7 dBFS |
//!
//! [`Sfx::Click`] is the app's **original settings sound**, reused as the single
//! interaction tap instead of bolting on a second flavour of click.
//! [`Sfx::Delete`] is the "sweeping it away" gesture — it fits both a single
//! removal and clearing the list, since the latter is literally emptying a bin.
//!
//! **What earns a sound.** The line is *"did this change the queue, or commit to
//! disk?"* — not *"did the user click something?"*:
//!
//! - Yes: add / drop files, remove a task, clear the list, start a batch, the
//!   row-level separation toggle, the gear (navigation), an explicit save.
//! - No: toggles inside the settings drawer (instantly visible, freely
//!   repeatable, and only the *default* for future tasks), hover, scroll,
//!   selection, and the drawer's auto-save on close.
//!
//! One sound per operation, never one per item: dropping 20 files or clearing a
//! 20-row list must not stutter.
//!
//! **Levels live in the files, not in code.** `PlaySound` has no gain parameter,
//! so each clip is peak-normalised to its role's target and must stay that way.
//! Never add a runtime volume knob: the only lever is re-cutting the WAV.
//!
//! Windows: a single serial worker owns all `PlaySound` calls (API is process-global
//! and not concurrent-safe). Rapid clicks replace the previous clip cleanly.

use crate::assets::asset_bytes;

#[derive(Clone, Copy)]
pub enum Sfx {
    /// Interaction tap: buttons, list actions, and the settings drawer.
    Click,
    /// Sweep-away gesture: one task removed, or the whole list cleared.
    Delete,
    /// A task finished successfully.
    TaskDone,
    /// A task failed and needs attention.
    TaskError,
}

pub fn play(kind: Sfx) {
    let path = match kind {
        Sfx::Click => "sounds/click.wav",
        Sfx::Delete => "sounds/delete.wav",
        Sfx::TaskDone => "sounds/task-done.wav",
        Sfx::TaskError => "sounds/task-error.wav",
    };
    let Some(bytes) = asset_bytes(path) else {
        return;
    };
    play_wav_memory(bytes);
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

//! Quiet UI feedback sounds (Windows PlaySound from embedded WAV; no-op elsewhere).
//!
//! Clips are `include_bytes!` via [`crate::assets`]. Keep samples < 100ms.
//!
//! Windows: a single serial worker owns all `PlaySound` calls (API is process-global
//! and not concurrent-safe). Rapid clicks replace the previous clip cleanly.

use crate::assets::asset_bytes;

#[derive(Clone, Copy)]
pub enum Sfx {
    /// Settings drawer open.
    Drawer,
    /// Icon / primary action confirm.
    Click,
}

pub fn play(kind: Sfx) {
    let path = match kind {
        Sfx::Drawer => "sounds/drawer.wav",
        Sfx::Click => "sounds/click.wav",
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

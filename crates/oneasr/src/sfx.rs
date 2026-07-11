//! Quiet UI feedback sounds (Windows PlaySound; no-op elsewhere).
//!
//! Volume is baked into the WAV samples (~10–12% peak). Keep clips < 100ms.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Clone, Copy)]
pub enum Sfx {
    /// Settings drawer open — slightly lower “space open” click.
    Drawer,
    /// Icon / primary action confirm.
    Click,
}

fn resolve_sound(name: &str) -> Option<PathBuf> {
    static ROOTS: OnceLock<Vec<PathBuf>> = OnceLock::new();
    let roots = ROOTS.get_or_init(|| {
        let mut v = Vec::new();
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                v.push(dir.to_path_buf());
                // cargo run: target/debug → walk up to repo
                let mut p = dir.to_path_buf();
                for _ in 0..6 {
                    v.push(p.clone());
                    if !p.pop() {
                        break;
                    }
                }
            }
        }
        if let Ok(cwd) = std::env::current_dir() {
            v.push(cwd);
        }
        v
    });
    for root in roots {
        let p = root.join("assets").join("sounds").join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

pub fn play(kind: Sfx) {
    let name = match kind {
        Sfx::Drawer => "drawer.wav",
        Sfx::Click => "click.wav",
    };
    let Some(path) = resolve_sound(name) else {
        return;
    };
    play_path(&path);
}

#[cfg(windows)]
fn play_path(path: &Path) {
    use std::os::windows::ffi::OsStrExt;
    // SND_ASYNC | SND_FILENAME | SND_NODEFAULT
    const FLAGS: u32 = 0x0001 | 0x0002_0000 | 0x0002;
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // Prefer powershell one-liner only if winmm fails — keep UI non-blocking via thread.
    let path_owned = path.to_path_buf();
    let wide_len = wide.len();
    thread::spawn(move || {
        let ok = unsafe { play_sound_w(wide.as_ptr(), FLAGS) };
        if !ok {
            // Fallback: Media.SoundPlayer (still async from UI thread).
            let _ = std::process::Command::new("powershell")
                .args([
                    "-NoProfile",
                    "-WindowStyle",
                    "Hidden",
                    "-Command",
                    &format!(
                        "(New-Object Media.SoundPlayer '{}').PlaySync()",
                        path_owned.display().to_string().replace('\'', "''")
                    ),
                ])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
        }
        let _ = wide_len;
    });
}

#[cfg(windows)]
unsafe fn play_sound_w(path: *const u16, flags: u32) -> bool {
    #[link(name = "winmm")]
    unsafe extern "system" {
        fn PlaySoundW(
            psz_sound: *const u16,
            hmod: *mut core::ffi::c_void,
            fdw_sound: u32,
        ) -> i32;
    }
    unsafe { PlaySoundW(path, std::ptr::null_mut(), flags) != 0 }
}

#[cfg(windows)]
use std::thread;

#[cfg(not(windows))]
fn play_path(_path: &Path) {}

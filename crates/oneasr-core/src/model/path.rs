//! Install-layout paths.
//!
//! - **Models**: `{app_root}/models/{name}`
//! - **CUDA DLLs**: `{exe_dir}/dll/` (dedicated runtime folder)
//!
//! Call [`init_native_library_path`] **once** at process start, **before** any
//! worker threads (rayon, ASR, download). Windows load uses `SetDllDirectoryW`
//! only — never mutates `PATH` (not thread-safe once other threads exist).
//! After that, new files dropped into `dll/` are found without re-init.

use std::path::PathBuf;

use crate::media::resolve_app_root;

use super::catalog::{HTDEMUCS_FT, QWEN3_ASR_06B, QWEN_ALIGN_06B};

/// Directory of the running `oneasr.exe` (install dir or `target/*/`).
pub fn resolve_exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Install / project root (folder that contains `bin/ffmpeg`), else exe dir.
pub fn resolve_app_root_dir() -> PathBuf {
    if let Some(root) = resolve_app_root() {
        return root;
    }
    resolve_exe_dir()
}

/// `{app_root}/models`
pub fn resolve_models_root() -> PathBuf {
    resolve_app_root_dir().join("models")
}

pub fn resolve_model_dir(model_name: &str) -> PathBuf {
    resolve_models_root().join(model_name)
}

pub fn default_asr_model_dir() -> PathBuf {
    resolve_model_dir(QWEN3_ASR_06B)
}

pub fn default_aligner_model_dir() -> PathBuf {
    resolve_model_dir(QWEN_ALIGN_06B)
}

/// `{app_root}/models/htdemucs_ft` — optional vocal-separation weights.
pub fn default_demucs_model_dir() -> PathBuf {
    resolve_model_dir(HTDEMUCS_FT)
}

/// Dedicated folder for native runtime DLLs (CUDA user-mode libs): `{exe_dir}/dll`.
pub fn resolve_dll_dir() -> PathBuf {
    resolve_exe_dir().join("dll")
}

/// Register `{exe}/dll` as the process native library directory (Windows).
///
/// **Call once before spawning compute/download threads.** Safe if the directory
/// is empty — later CUDA downloads land in the same path and are loadable without
/// calling this again.
///
/// Uses `SetDllDirectoryW` only (no `PATH` mutation). Creates `dll/` if missing.
pub fn init_native_library_path() {
    let dll_dir = resolve_dll_dir();
    let _ = std::fs::create_dir_all(&dll_dir);

    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = dll_dir
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: process-wide DLL search path; intended only at single-threaded startup.
        let ok = unsafe { SetDllDirectoryW(wide.as_ptr()) };
        if ok == 0 {
            #[cfg(debug_assertions)]
            eprintln!(
                "oneasr: SetDllDirectoryW failed for {} — GPU DLL load may fail",
                dll_dir.display()
            );
        }
    }

    #[cfg(not(windows))]
    {
        let _ = dll_dir;
    }
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn SetDllDirectoryW(lp_path_name: *const u16) -> i32;
}

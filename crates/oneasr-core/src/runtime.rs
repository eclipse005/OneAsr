//! Process-wide runtime setup so heavy ASR never starves the UI thread.

/// Cap the global rayon pool **before** any model load / transcribe.
///
/// Model CPU kernels use rayon's global pool. Leaving every logical core to
/// rayon freezes the GPUI event loop. Always keep at least one core for UI/OS.
pub fn init_runtime() {
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    // ≤2 cores: still leave UI a share by using 1 compute thread.
    // More cores: reserve one logical CPU for the UI / OS message pump.
    let workers = if cores <= 2 { 1 } else { cores - 1 };
    // Only the first call succeeds; ignore if something else already built it.
    let _ = rayon::ThreadPoolBuilder::new()
        .num_threads(workers)
        .thread_name(|i| format!("oneasr-compute-{i}"))
        .build_global();
}

/// Drop this worker below normal priority so the UI thread wins under load.
pub fn demote_current_thread() {
    #[cfg(windows)]
    {
        // THREAD_PRIORITY_BELOW_NORMAL = -1
        unsafe extern "system" {
            fn GetCurrentThread() -> *mut core::ffi::c_void;
            fn SetThreadPriority(thread: *mut core::ffi::c_void, priority: i32) -> i32;
        }
        unsafe {
            let _ = SetThreadPriority(GetCurrentThread(), -1);
        }
    }
}

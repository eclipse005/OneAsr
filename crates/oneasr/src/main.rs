//! OneAsr 进程入口：限制计算线程，然后打开 Native Shell 窗口。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> std::process::ExitCode {
    oneasr_core::init_runtime();
    // 关掉窗口后，NSH 会在界面线程里等引擎线程结束，两边互相等就会卡住。
    // 窗口已经没了，进程却还占着控制台；再关页签时管道被拆掉，Windows 报
    // 0x800700E8（管道已关闭）。窗口消失就结束进程。
    #[cfg(windows)]
    exit_when_the_window_is_gone();
    oneasr::run()
}

/// 看到本进程的 OneAsr 顶层窗口之后，窗口句柄失效就 `exit`。
#[cfg(windows)]
fn exit_when_the_window_is_gone() {
    use std::sync::atomic::{AtomicIsize, Ordering};

    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsWindow,
        IsWindowVisible,
    };

    std::thread::spawn(|| {
        static HWND_BITS: AtomicIsize = AtomicIsize::new(0);

        unsafe extern "system" fn enum_proc(hwnd: HWND, pid: LPARAM) -> windows::core::BOOL {
            // SAFETY: 回调只在 `EnumWindows` 期间跑，`pid` 是本进程 id。
            unsafe {
                let mut owner = 0_u32;
                GetWindowThreadProcessId(hwnd, Some(&mut owner));
                if owner != pid.0 as u32 || !IsWindowVisible(hwnd).as_bool() {
                    return true.into();
                }
                let len = GetWindowTextLengthW(hwnd);
                if len == 6 {
                    let mut buf = [0_u16; 8];
                    let n = GetWindowTextW(hwnd, &mut buf);
                    if n == 6 && String::from_utf16_lossy(&buf[..6]) == "OneAsr" {
                        HWND_BITS.store(hwnd.0 as isize, Ordering::Relaxed);
                        return false.into();
                    }
                }
                true.into()
            }
        }

        let pid = std::process::id();
        loop {
            std::thread::sleep(std::time::Duration::from_millis(200));
            let bits = HWND_BITS.load(Ordering::Relaxed);
            if bits == 0 {
                // SAFETY: 枚举顶层窗口，只读标题和所属进程。
                unsafe {
                    let _ = EnumWindows(Some(enum_proc), LPARAM(pid as isize));
                }
                continue;
            }
            let hwnd = HWND(bits as *mut core::ffi::c_void);
            // SAFETY: 句柄来自刚才的枚举；失效时 `IsWindow` 返回 false。
            let alive = unsafe { IsWindow(Some(hwnd)).as_bool() };
            if !alive {
                // `process::exit` 会等界面线程跑完析构，而界面线程正卡在等引擎，
                // 两边谁也不放。窗口已经没了，直接结束进程，控制台才能一起关掉。
                // SAFETY: 结束的是当前进程；此时顶层窗口已经销毁。
                unsafe {
                    let _ = TerminateProcess(GetCurrentProcess(), 0);
                }
            }
        }
    });
}

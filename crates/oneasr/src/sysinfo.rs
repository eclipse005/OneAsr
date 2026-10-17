//! Hardware / OS snapshot for the error log.
//!
//! Support only ever sees the log the user pastes, so the log has to say what
//! kind of machine produced it. This module deliberately stops at "what kind":
//! no hostname, user name, SID, MAC or IP. Home paths are redacted by
//! [`crate::crashlog`]; media names stay (see its module docs).
//!
//! Everything here is best-effort: a failed probe omits the line, it never
//! fails the app. The formatting half is pure so it is tested on every CI OS.

use std::fmt::Write as _;

/// Plain data collected by the platform probe. Keeping it separate from the
/// formatter is what makes the report testable without a Windows API.
#[derive(Debug, Default)]
struct SystemFacts {
    os: String,
    cpu: String,
    cores: Option<usize>,
    /// (total, available) physical memory, bytes.
    memory: Option<(u64, u64)>,
    session: Option<String>,
    power: Option<String>,
    tdr: Option<String>,
    display: Option<String>,
}

/// The `system:` block written into the session context at startup.
pub fn system_report() -> String {
    format_report(&collect())
}

/// Current process memory: working set, peak, private commit.
///
/// Called at task start / failure; cheap enough to do on both sides.
pub fn process_memory() -> Option<String> {
    #[cfg(windows)]
    {
        let size = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        let mut counters = PROCESS_MEMORY_COUNTERS {
            cb: size,
            ..PROCESS_MEMORY_COUNTERS::default()
        };
        unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, size) }.ok()?;
        Some(format!(
            "working_set={} peak={} private={}",
            mib(counters.WorkingSetSize as u64),
            mib(counters.PeakWorkingSetSize as u64),
            mib(counters.PagefileUsage as u64),
        ))
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Per-adapter dedicated VRAM and this process's local usage / budget.
///
/// DXGI is the only cheap way to get the numbers Task Manager shows. A machine
/// whose DXGI enumeration fails simply omits the block.
pub fn gpu_memory() -> Option<String> {
    #[cfg(windows)]
    {
        let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }.ok()?;
        let mut lines = Vec::new();
        let mut index = 0u32;
        loop {
            let adapter = match unsafe { factory.EnumAdapters1(index) } {
                Ok(adapter) => adapter,
                Err(_) => break,
            };
            let desc = match unsafe { adapter.GetDesc1() } {
                Ok(desc) => desc,
                Err(_) => break,
            };
            let name = String::from_utf16_lossy(&desc.Description);
            let name = name.trim_end_matches('\0').trim();
            let mut line = format!(
                "{name}: dedicated {}",
                mib(desc.DedicatedVideoMemory as u64)
            );
            if let Ok(adapter3) = adapter.cast::<IDXGIAdapter3>() {
                let mut info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
                if unsafe {
                    adapter3.QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut info)
                }
                .is_ok()
                {
                    let _ = write!(
                        line,
                        ", process local {} / budget {}",
                        mib(info.CurrentUsage),
                        mib(info.Budget),
                    );
                }
            }
            lines.push(line);
            index += 1;
        }
        (!lines.is_empty()).then(|| lines.join("\n"))
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// `os: ...` etc., one line per fact; absent facts are omitted.
fn format_report(facts: &SystemFacts) -> String {
    let mut lines = Vec::new();
    lines.push(format!("os: {}", facts.os));
    let mut cpu = facts.cpu.clone();
    if let Some(cores) = facts.cores {
        let _ = write!(cpu, " ({cores} logical)");
    }
    lines.push(format!("cpu: {cpu}"));
    if let Some((total, free)) = facts.memory {
        lines.push(format!("memory: {} total, {} free", gib(total), gib(free)));
    }
    if let Some(session) = &facts.session {
        lines.push(format!("session: {session}"));
    }
    if let Some(power) = &facts.power {
        lines.push(format!("power: {power}"));
    }
    if let Some(tdr) = &facts.tdr {
        lines.push(format!("tdr: {tdr}"));
    }
    if let Some(display) = &facts.display {
        lines.push(format!("display: {display}"));
    }
    lines.join("\n")
}

fn gib(bytes: u64) -> String {
    format!("{:.1} GiB", bytes as f64 / (1u64 << 30) as f64)
}

#[cfg(any(windows, test))]
fn mib(bytes: u64) -> String {
    format!("{} MiB", bytes / (1u64 << 20))
}

#[cfg(windows)]
fn collect() -> SystemFacts {
    SystemFacts {
        os: os_label(),
        cpu: cpu_label(),
        cores: std::thread::available_parallelism().ok().map(|n| n.get()),
        memory: memory_facts(),
        session: session_label(),
        power: power_label(),
        tdr: tdr_label(),
        display: display_label(),
    }
}

#[cfg(not(windows))]
fn collect() -> SystemFacts {
    SystemFacts {
        os: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        cpu: std::env::consts::ARCH.to_string(),
        cores: std::thread::available_parallelism().ok().map(|n| n.get()),
        ..SystemFacts::default()
    }
}

#[cfg(windows)]
const VERSION_KEY: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
#[cfg(windows)]
const TDR_KEY: &str = r"SYSTEM\CurrentControlSet\Control\GraphicsDrivers";

#[cfg(windows)]
fn os_label() -> String {
    let build = reg_string(VERSION_KEY, "CurrentBuild").and_then(|value| value.parse::<u32>().ok());
    let revision = reg_dword(VERSION_KEY, "UBR");
    let display = reg_string(VERSION_KEY, "DisplayVersion");
    os_label_from(build, revision, display.as_deref())
}

/// Pure formatter for the version facts, so the Win10/Win11 split is testable.
#[cfg(windows)]
fn os_label_from(build: Option<u32>, revision: Option<u32>, display: Option<&str>) -> String {
    let Some(build) = build else {
        return format!("{} {}", std::env::consts::OS, std::env::consts::ARCH);
    };
    let family = if build >= 22000 {
        "Windows 11"
    } else {
        "Windows 10"
    };
    let mut label = match display {
        Some(display) if !display.is_empty() => format!("{family} {display}"),
        _ => family.to_string(),
    };
    match revision {
        Some(revision) => {
            let _ = write!(label, " (build {build}.{revision})");
        }
        None => {
            let _ = write!(label, " (build {build})");
        }
    }
    label
}

#[cfg(windows)]
fn cpu_label() -> String {
    #[cfg(target_arch = "x86_64")]
    if let Some(brand) = x86_cpu_brand() {
        return brand;
    }
    std::env::consts::ARCH.to_string()
}

/// CPUID leaves 0x8000_0002..4 carry the 48-byte brand string.
#[cfg(all(windows, target_arch = "x86_64"))]
fn x86_cpu_brand() -> Option<String> {
    use std::arch::x86_64::__cpuid;

    let mut bytes = [0u8; 48];
    for (index, leaf) in [0x8000_0002u32, 0x8000_0003, 0x8000_0004]
        .iter()
        .enumerate()
    {
        let result = __cpuid(*leaf);
        for (word_index, word) in [result.eax, result.ebx, result.ecx, result.edx]
            .iter()
            .enumerate()
        {
            let offset = index * 16 + word_index * 4;
            bytes[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
        }
    }
    let brand = String::from_utf8_lossy(&bytes);
    let brand = brand.trim_matches('\0').trim();
    (!brand.is_empty()).then(|| brand.to_string())
}

#[cfg(windows)]
fn memory_facts() -> Option<(u64, u64)> {
    let mut status = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..MEMORYSTATUSEX::default()
    };
    unsafe { GlobalMemoryStatusEx(&mut status) }.ok()?;
    Some((status.ullTotalPhys, status.ullAvailPhys))
}

#[cfg(windows)]
fn session_label() -> Option<String> {
    let remote = unsafe { GetSystemMetrics(SM_REMOTESESSION) } != 0;
    Some(if remote {
        "RDP".to_string()
    } else {
        "local".to_string()
    })
}

#[cfg(windows)]
fn power_label() -> Option<String> {
    let mut status = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut status) }.ok()?;
    match status.ACLineStatus {
        1 => Some("AC".to_string()),
        0 => {
            let percent = status.BatteryLifePercent;
            Some(if percent <= 100 {
                format!("battery {percent}%")
            } else {
                "battery".to_string()
            })
        }
        _ => None,
    }
}

#[cfg(windows)]
fn tdr_label() -> Option<String> {
    // Absent keys mean the OS defaults; printing them makes "user raised the
    // TDR delay" visible by comparison instead of by absence.
    let delay = reg_dword(TDR_KEY, "TdrDelay").unwrap_or(2);
    let ddi = reg_dword(TDR_KEY, "TdrDdiDelay").unwrap_or(5);
    let level = reg_dword(TDR_KEY, "TdrLevel").unwrap_or(3);
    Some(format!(
        "TdrDelay={delay} TdrDdiDelay={ddi} TdrLevel={level}"
    ))
}

#[cfg(windows)]
fn display_label() -> Option<String> {
    let width = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let height = unsafe { GetSystemMetrics(SM_CYSCREEN) };
    (width > 0 && height > 0).then(|| format!("{width}x{height}"))
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn reg_string(subkey: &str, value: &str) -> Option<String> {
    let subkey = wide(subkey);
    let value = wide(value);
    let mut buffer = [0u16; 256];
    let mut size = (buffer.len() * 2) as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(subkey.as_ptr()),
            PCWSTR(value.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr() as *mut std::ffi::c_void),
            Some(&mut size),
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }
    let len = ((size as usize) / 2).min(buffer.len());
    let text = String::from_utf16_lossy(&buffer[..len]);
    let text = text.trim_end_matches('\0').trim();
    (!text.is_empty()).then(|| text.to_string())
}

#[cfg(windows)]
fn reg_dword(subkey: &str, value: &str) -> Option<u32> {
    let subkey = wide(subkey);
    let value = wide(value);
    let mut data = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(subkey.as_ptr()),
            PCWSTR(value.as_ptr()),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut data as *mut u32 as *mut std::ffi::c_void),
            Some(&mut size),
        )
    };
    (status == ERROR_SUCCESS).then_some(data)
}

#[cfg(windows)]
use windows::Win32::Foundation::ERROR_SUCCESS;
#[cfg(windows)]
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, DXGI_QUERY_VIDEO_MEMORY_INFO,
    IDXGIAdapter3, IDXGIFactory1,
};
#[cfg(windows)]
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
#[cfg(windows)]
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
#[cfg(windows)]
use windows::Win32::System::Registry::{
    HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW,
};
#[cfg(windows)]
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
#[cfg(windows)]
use windows::Win32::System::Threading::GetCurrentProcess;
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN, SM_REMOTESESSION,
};
#[cfg(windows)]
use windows::core::{Interface as _, PCWSTR};

#[cfg(test)]
mod tests {
    use super::*;

    /// All facts present: every line appears in a stable order.
    #[test]
    fn report_contains_every_collected_fact() {
        let facts = SystemFacts {
            os: "Windows 11 23H2 (build 22631.4317)".into(),
            cpu: "AMD Ryzen 7 5800X".into(),
            cores: Some(16),
            memory: Some((32 << 30, 18 << 30)),
            session: Some("local".into()),
            power: Some("AC".into()),
            tdr: Some("TdrDelay=2 TdrDdiDelay=5 TdrLevel=3".into()),
            display: Some("2560x1440".into()),
        };
        let report = format_report(&facts);
        for expected in [
            "os: Windows 11 23H2 (build 22631.4317)",
            "cpu: AMD Ryzen 7 5800X (16 logical)",
            "memory: 32.0 GiB total, 18.0 GiB free",
            "session: local",
            "power: AC",
            "tdr: TdrDelay=2 TdrDdiDelay=5 TdrLevel=3",
            "display: 2560x1440",
        ] {
            assert!(
                report.contains(expected),
                "missing {expected:?} in {report}"
            );
        }
    }

    /// A probe that found nothing must not fabricate lines.
    #[test]
    fn report_omits_absent_facts() {
        let facts = SystemFacts {
            os: "linux x86_64".into(),
            cpu: "x86_64".into(),
            ..SystemFacts::default()
        };
        let report = format_report(&facts);
        assert_eq!(report, "os: linux x86_64\ncpu: x86_64");
    }

    #[test]
    fn byte_units_are_human_readable() {
        assert_eq!(gib(32 << 30), "32.0 GiB");
        assert_eq!(mib(12_288 << 20), "12288 MiB");
        assert_eq!(mib(5 << 20), "5 MiB");
    }

    /// Best-effort probes must never take the app down: a missing registry
    /// value, a failed DXGI call or a headless CI box just omits the block.
    #[test]
    fn probes_do_not_panic() {
        let report = system_report();
        assert!(!report.is_empty(), "the os/cpu lines are always present");
        let _ = process_memory();
        let _ = gpu_memory();
    }

    #[cfg(windows)]
    #[test]
    fn windows_build_number_picks_the_family() {
        let win11 = os_label_from(Some(22631), Some(4317), Some("23H2"));
        assert_eq!(win11, "Windows 11 23H2 (build 22631.4317)");
        let win10 = os_label_from(Some(19045), None, Some("22H2"));
        assert_eq!(win10, "Windows 10 22H2 (build 19045)");
        let unknown = os_label_from(None, None, None);
        assert!(unknown.contains(std::env::consts::OS), "{unknown}");
    }
}

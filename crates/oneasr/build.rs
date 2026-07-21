//! Embed Windows application icon into the .exe (taskbar / explorer / titlebar).
//!
//! GPUI on Windows loads resource ID 1 via LoadImageW — winresource puts the
//! ICO there. Debug and release both get the icon when building on Windows.
//!
//! **Subsystem policy (source of truth):**
//! - Debug: default CONSOLE (via no `windows_subsystem` attr) so `cargo run` logs work.
//! - Release: GUI only — `#![windows_subsystem = "windows"]` in main.rs **and**
//!   a profile-gated link flag here, so portable / Explorer never show a black box.
//! - `pack-release.ps1` asserts PE subsystem == GUI before shipping.

fn main() {
    #[cfg(windows)]
    {
        let profile = std::env::var("PROFILE").unwrap_or_default();
        if profile == "release" {
            let target = std::env::var("TARGET").unwrap_or_default();
            if target.contains("msvc") {
                println!("cargo:rustc-link-arg=/SUBSYSTEM:WINDOWS");
            } else if target.contains("windows-gnu") {
                println!("cargo:rustc-link-arg=-Wl,--subsystem,windows");
            }
        }

        let mut res = winresource::WindowsResource::new();
        // Path relative to this crate (crates/oneasr) → repo assets.
        let icon = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/icons/app-icon.ico");
        if icon.is_file() {
            res.set_icon(icon.to_str().expect("icon path utf-8"));
            if let Err(e) = res.compile() {
                println!("cargo:warning=winresource failed to embed icon: {e}");
            }
        } else {
            println!(
                "cargo:warning=app icon missing at {} — run scripts/gen-app-icon.py",
                icon.display()
            );
        }
        println!("cargo:rerun-if-changed={}", icon.display());
    }
}

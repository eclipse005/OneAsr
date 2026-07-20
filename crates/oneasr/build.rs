//! Embed Windows application icon into the .exe (taskbar / explorer / titlebar).
//!
//! GPUI on Windows loads resource ID 1 via LoadImageW — winresource puts the
//! ICO there. Debug and release both get the icon when building on Windows.

fn main() {
    #[cfg(windows)]
    {
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

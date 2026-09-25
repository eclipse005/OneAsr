//! Compile-time embedded UI assets (SVG icons + WAV sfx).
//!
//! No runtime `assets/` folder is required next to the executable. Source files
//! still live in the repo at `assets/` for editing; `include_bytes!` pulls them
//! into the binary at build time.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::OnceLock;

use anyhow::Result;
use gpui::{AssetSource, SharedString};

/// Paths used by `svg().path(...)` — must match keys in the embed table.
pub struct AppAssets;

impl AppAssets {
    pub fn new() -> Self {
        Self
    }
}

fn table() -> &'static HashMap<&'static str, &'static [u8]> {
    static T: OnceLock<HashMap<&'static str, &'static [u8]>> = OnceLock::new();
    T.get_or_init(|| {
        // Paths relative to this crate → repo `assets/` (../../assets from crates/oneasr).
        HashMap::from([
            (
                "icons/logo.svg",
                include_bytes!("../../../assets/icons/logo.svg").as_slice(),
            ),
            (
                "icons/gear.svg",
                include_bytes!("../../../assets/icons/gear.svg").as_slice(),
            ),
            (
                "icons/download.svg",
                include_bytes!("../../../assets/icons/download.svg").as_slice(),
            ),
            (
                "icons/redownload.svg",
                include_bytes!("../../../assets/icons/redownload.svg").as_slice(),
            ),
            (
                "icons/stop.svg",
                include_bytes!("../../../assets/icons/stop.svg").as_slice(),
            ),
            (
                "icons/win-min.svg",
                include_bytes!("../../../assets/icons/win-min.svg").as_slice(),
            ),
            (
                "icons/win-max.svg",
                include_bytes!("../../../assets/icons/win-max.svg").as_slice(),
            ),
            (
                "icons/win-close.svg",
                include_bytes!("../../../assets/icons/win-close.svg").as_slice(),
            ),
            (
                "icons/folder.svg",
                include_bytes!("../../../assets/icons/folder.svg").as_slice(),
            ),
            (
                "icons/play.svg",
                include_bytes!("../../../assets/icons/play.svg").as_slice(),
            ),
            (
                "icons/trash.svg",
                include_bytes!("../../../assets/icons/trash.svg").as_slice(),
            ),
            (
                "icons/audio.svg",
                include_bytes!("../../../assets/icons/audio.svg").as_slice(),
            ),
            (
                "icons/video.svg",
                include_bytes!("../../../assets/icons/video.svg").as_slice(),
            ),
            (
                "sounds/click.wav",
                include_bytes!("../../../assets/sounds/click.wav").as_slice(),
            ),
            (
                "sounds/reminder.wav",
                include_bytes!("../../../assets/sounds/reminder.wav").as_slice(),
            ),
        ])
    })
}

/// Raw bytes for an embedded asset path (e.g. `sounds/click.wav`).
pub fn asset_bytes(path: &str) -> Option<&'static [u8]> {
    table().get(path).copied()
}

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(asset_bytes(path).map(Cow::Borrowed))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let prefix = if path.is_empty() {
            String::new()
        } else {
            let mut p = path.to_string();
            if !p.ends_with('/') && !p.ends_with('\\') {
                p.push('/');
            }
            p.replace('\\', "/")
        };
        let mut names = Vec::new();
        for key in table().keys() {
            let k = key.replace('\\', "/");
            if prefix.is_empty() {
                // top-level segment only
                if let Some(seg) = k.split('/').next() {
                    let s = SharedString::from(seg.to_string());
                    if !names.iter().any(|x: &SharedString| x.as_ref() == s.as_ref()) {
                        names.push(s);
                    }
                }
            } else if let Some(rest) = k.strip_prefix(&prefix)
                && !rest.is_empty() && !rest.contains('/') {
                names.push(SharedString::from(rest.to_string()));
            }
        }
        names.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));
        Ok(names)
    }
}

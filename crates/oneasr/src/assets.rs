//! Load `assets/` next to the repo / install root for SVG icons and sfx paths.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use anyhow::Result;
use gpui::{AssetSource, SharedString};

pub struct AppAssets {
    base: PathBuf,
}

impl AppAssets {
    pub fn new() -> Self {
        Self {
            base: resolve_assets_dir().unwrap_or_else(|| PathBuf::from("assets")),
        }
    }
}

/// Prefer `{repo|install}/assets`.
pub fn resolve_assets_dir() -> Option<PathBuf> {
    let mut starts = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            starts.push(dir.to_path_buf());
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        starts.push(cwd);
    }
    for start in starts {
        let mut dir = start;
        for _ in 0..8 {
            let assets = dir.join("assets");
            if assets.is_dir() {
                return Some(assets);
            }
            if !dir.pop() {
                break;
            }
        }
    }
    None
}

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        let full = self.base.join(path);
        match std::fs::read(&full) {
            Ok(bytes) => Ok(Some(Cow::Owned(bytes))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let full = self.base.join(path);
        let mut out = Vec::new();
        if let Ok(rd) = std::fs::read_dir(full) {
            for entry in rd.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    out.push(SharedString::from(name.to_string()));
                }
            }
        }
        Ok(out)
    }
}

#[allow(dead_code)]
pub fn asset_path(rel: impl AsRef<Path>) -> PathBuf {
    resolve_assets_dir()
        .unwrap_or_else(|| PathBuf::from("assets"))
        .join(rel)
}

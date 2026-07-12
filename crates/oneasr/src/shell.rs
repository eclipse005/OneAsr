//! OS shell helpers (Windows-first). Never block the UI thread on completion.

use std::path::{Path, PathBuf};
use std::process::Command;

fn parent_dir(file: &Path) -> Option<PathBuf> {
    file.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.to_path_buf())
}

/// Open the folder containing `file` and select it when possible (Windows).
pub fn open_containing_folder(file: &Path) -> Result<(), String> {
    if !file.exists() {
        if let Some(parent) = parent_dir(file) {
            return open_folder(&parent);
        }
        return Err(format!("文件不存在: {}", file.display()));
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let path = file
            .canonicalize()
            .unwrap_or_else(|_| file.to_path_buf());
        let arg = format!("/select,\"{}\"", path.display());
        Command::new("explorer")
            .raw_arg(arg)
            .spawn()
            .map_err(|e| format!("无法打开资源管理器: {e}"))?;
        Ok(())
    }

    #[cfg(not(windows))]
    {
        if let Some(parent) = parent_dir(file) {
            return open_folder(&parent);
        }
        Err("无法解析输出目录".into())
    }
}

fn open_folder(dir: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        Command::new("explorer")
            .arg(dir.as_os_str())
            .spawn()
            .map_err(|e| format!("无法打开文件夹: {e}"))?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Command::new("xdg-open")
            .arg(dir.as_os_str())
            .spawn()
            .map_err(|e| format!("无法打开文件夹: {e}"))?;
        Ok(())
    }
}

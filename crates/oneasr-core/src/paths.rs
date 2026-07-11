//! Install-dir path policy (pure, no I/O).

use std::path::{Path, PathBuf};

/// File stem for a media path (`video.mp4` → `video`). Falls back to `"out"`.
pub fn media_stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "out".into())
}

/// User-facing deliverable: `{app_root}/output/{media_stem}.srt`.
pub fn output_srt_path(app_root: &Path, media_stem: &str) -> PathBuf {
    let stem = if media_stem.is_empty() {
        "out"
    } else {
        media_stem
    };
    app_root.join("output").join(format!("{stem}.srt"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn media_stem_from_video_name() {
        assert_eq!(media_stem(Path::new(r"D:\clips\my_video.mp4")), "my_video");
        assert_eq!(media_stem(Path::new("demo.wav")), "demo");
    }

    #[test]
    fn output_srt_under_install_output() {
        let root = PathBuf::from(r"D:\OneAsr");
        let p = output_srt_path(&root, "my_video");
        assert_eq!(p, PathBuf::from(r"D:\OneAsr\output\my_video.srt"));
    }

    #[test]
    fn empty_stem_falls_back() {
        let root = PathBuf::from("/app");
        assert_eq!(output_srt_path(&root, ""), PathBuf::from("/app/output/out.srt"));
    }
}

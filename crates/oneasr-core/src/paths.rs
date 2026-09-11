//! Install-dir path policy (pure, no I/O).

use std::path::{Path, PathBuf};

/// File stem for a media path (`video.mp4` → `video`). Falls back to `"out"`.
pub fn media_stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "out".into())
}

/// Default SRT folder under the install / portable root: `{app_root}/output`.
pub fn default_output_dir_for(app_root: &Path) -> PathBuf {
    app_root.join("output")
}

/// User-facing deliverable: `{output_dir}/{media_stem}.srt`.
pub fn output_srt_path(output_dir: &Path, media_stem: &str) -> PathBuf {
    output_path(output_dir, media_stem, "srt")
}

/// `{output_dir}/{media_stem}.{ext}` (`ext` without dot).
pub fn output_path(output_dir: &Path, media_stem: &str, ext: &str) -> PathBuf {
    let stem = if media_stem.is_empty() {
        "out"
    } else {
        media_stem
    };
    output_dir.join(format!("{stem}.{ext}"))
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
    fn output_srt_under_chosen_dir() {
        let dir = PathBuf::from(r"D:\OneAsr\output");
        let p = output_srt_path(&dir, "my_video");
        assert_eq!(p, PathBuf::from(r"D:\OneAsr\output\my_video.srt"));
        assert_eq!(
            output_path(&dir, "my_video", "txt"),
            PathBuf::from(r"D:\OneAsr\output\my_video.txt")
        );
    }

    #[test]
    fn empty_stem_falls_back() {
        let dir = PathBuf::from("/app/output");
        assert_eq!(output_srt_path(&dir, ""), PathBuf::from("/app/output/out.srt"));
    }

    #[test]
    fn default_output_dir_is_app_output() {
        let root = PathBuf::from(r"D:\OneAsr");
        assert_eq!(
            default_output_dir_for(&root),
            PathBuf::from(r"D:\OneAsr\output")
        );
    }
}

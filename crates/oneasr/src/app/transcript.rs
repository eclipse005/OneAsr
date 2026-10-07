//! 挑出来的文件怎么变成「媒体 + 挂好的文稿」。
//!
//! 添加对话框和拖拽共用这一份配对规则：只选一个媒体和一个文稿时直接配对；
//! 多文件时文本按**文件名**找同名媒体。配不上的**不静默忽略**——用户挑了一堆
//! 文件，最后靠数行数才知道哪份稿子没生效，那是最容易出岔子的地方。
//!
//! 纯函数，好测；不碰文件系统，所以「某个路径存不存在」不在它的判断里。

use std::path::{Path, PathBuf};

/// 媒体扩展名与添加对话框的过滤器一致。两处必须同步：这里决定「什么建任务」，
/// 那里决定「什么能被选到」。
const MEDIA_EXTS: &[&str] = &[
    "wav", "mp3", "m4a", "flac", "ogg", "opus", "aac", "wma", "mp4", "mkv", "mov", "webm", "avi",
    "m4v",
];

/// 选完文件之后的配对结果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Pairing {
    /// 按选中顺序排的媒体，每个建一条任务（无文稿的任务也在内）。
    pub media: Vec<PathBuf>,
    /// `(文稿, 它挂到的那条媒体)`，按文稿的选中顺序。
    pub transcripts: Vec<(PathBuf, PathBuf)>,
    /// 没找到同名媒体的文稿。
    pub unpaired_texts: Vec<PathBuf>,
}

/// 文件名去掉扩展名。用 `file_stem` 而不是自己切——`a.tar.gz`、`a.b.mp4` 这类
/// 名字只有它切得对。
fn stem_of(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

fn ext_of(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

pub fn is_media(path: &Path) -> bool {
    MEDIA_EXTS.contains(&ext_of(path).as_str())
}

pub fn is_transcript(path: &Path) -> bool {
    oneasr_core::transcript::TranscriptKind::from_path(path).is_some()
}

/// 配对。三条规则，都是为了「用户看得懂」：
///
/// * stem 比较**不区分大小写**——`杂谈分享.MP4` 与 `杂谈分享.txt` 是一对。
/// * 一条媒体只接**第一份**同名文稿（选择顺序在前的那份）。两份文稿同时出现时
///   第二份不是「也挂上」，那是无法解释的状态；放进 `unpaired_texts` 报出去。
/// * 媒体之间的先后、同一媒体被重复选中的次数都原样保留：前者是队列顺序，后者
///   是用户自己添了两条任务，不该在这里替他合并。
pub fn pair_picked(picked: &[PathBuf]) -> Pairing {
    let mut out = Pairing::default();
    // 先按选中顺序收集媒体：文稿要在**全部**媒体都登记之后才能配，
    // 否则「先选文稿后选视频」会配不上。
    for path in picked {
        if is_media(path) {
            out.media.push(path.clone());
        }
    }
    let texts: Vec<&PathBuf> = picked.iter().filter(|p| is_transcript(p)).collect();
    if out.media.len() == 1 && texts.len() == 1 {
        out.transcripts
            .push((texts[0].clone(), out.media[0].clone()));
        return out;
    }
    // 记录**哪条媒体**已经被接走了，而不是哪个 stem——两个同 stem 的媒体
    // (a.mp4 / a.wav) 各自该有自己的文稿，按 stem 去重会把第二份挤成「没配上」。
    let mut claimed: Vec<usize> = Vec::new();
    for text in texts {
        let key = stem_of(text);
        match out
            .media
            .iter()
            .enumerate()
            .find(|(i, m)| stem_of(m) == key && !claimed.contains(i))
            .map(|(_, m)| m.clone())
        {
            Some(media) => {
                claimed.push(out.media.iter().position(|m| m == &media).unwrap_or(0));
                out.transcripts.push((text.clone(), media));
            }
            None => out.unpaired_texts.push(text.clone()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(name: &str) -> PathBuf {
        // 用 join 拼、不写死分隔符：三端都能跑。
        Path::new("/videos").join(name)
    }

    #[test]
    fn a_matching_name_pairs_the_transcript_to_the_media() {
        let got = pair_picked(&[p("a.mp4"), p("a.txt")]);
        assert_eq!(got.media, vec![p("a.mp4")]);
        assert_eq!(got.transcripts, vec![(p("a.txt"), p("a.mp4"))]);
        assert!(got.unpaired_texts.is_empty());
    }

    /// 顺序无关：先选文稿后选视频也得配上，否则「先挑稿子再挑视频」是死路。
    #[test]
    fn pairing_does_not_depend_on_selection_order() {
        let got = pair_picked(&[p("a.txt"), p("a.mp4")]);
        assert_eq!(got.transcripts, vec![(p("a.txt"), p("a.mp4"))]);
    }

    #[test]
    fn one_media_and_one_transcript_pair_even_when_their_names_differ() {
        let got = pair_picked(&[p("meeting.mp4"), p("notes.txt")]);
        assert_eq!(got.media, vec![p("meeting.mp4")]);
        assert_eq!(got.transcripts, vec![(p("notes.txt"), p("meeting.mp4"))]);
        assert!(got.unpaired_texts.is_empty());
    }

    /// 大小写与扩展名：用户在 Windows 上双击得到 `A.MP4`，文稿是 `a.txt`。
    #[test]
    fn stem_comparison_ignores_case_and_extension_case() {
        let got = pair_picked(&[p("A.MP4"), p("a.TXT")]);
        assert_eq!(got.transcripts, vec![(p("a.TXT"), p("A.MP4"))]);
    }

    /// 真正的坑：`a.b.mp4` 的 stem 是 `a.b`，不是 `a`。自己切字符串会错。
    #[test]
    fn a_dotted_name_matches_on_its_full_stem() {
        let got = pair_picked(&[p("a.b.mp4"), p("a.b.srt")]);
        assert_eq!(got.transcripts, vec![(p("a.b.srt"), p("a.b.mp4"))]);
        // With multiple media files, a different stem must stay unmatched.
        let other = pair_picked(&[p("a.b.mp4"), p("extra.wav"), p("a.txt")]);
        assert!(other.unpaired_texts.contains(&p("a.txt")));
    }

    /// 一份媒体只接第一份同名文稿；第二份报出去，不静默丢弃。
    #[test]
    fn a_second_transcript_for_one_media_is_reported_not_taken() {
        let got = pair_picked(&[p("a.mp4"), p("a.txt"), p("a.srt")]);
        assert_eq!(got.transcripts, vec![(p("a.txt"), p("a.mp4"))]);
        assert_eq!(got.unpaired_texts, vec![p("a.srt")]);
    }

    #[test]
    fn unpaired_texts_are_named_so_the_hint_can_list_them() {
        let got = pair_picked(&[p("a.mp4"), p("b.txt"), p("c.srt"), p("d.webm")]);
        assert_eq!(got.media, vec![p("a.mp4"), p("d.webm")]);
        assert_eq!(got.unpaired_texts, vec![p("b.txt"), p("c.srt")]);
        assert!(got.transcripts.is_empty());
    }

    /// 只有文稿、没有媒体：一条任务都建不出来，所以必须报出来而不是建空任务。
    #[test]
    fn text_without_media_makes_no_task() {
        let got = pair_picked(&[p("a.txt")]);
        assert!(got.media.is_empty());
        assert_eq!(got.unpaired_texts, vec![p("a.txt")]);
    }

    /// 两个同 stem 的媒体各接一份文稿——不替用户合并任务。
    #[test]
    fn two_media_with_the_same_stem_each_get_their_own_transcript() {
        let got = pair_picked(&[p("a.mp4"), p("a.wav"), p("a.txt"), p("a.srt")]);
        assert_eq!(got.media, vec![p("a.mp4"), p("a.wav")]);
        assert_eq!(
            got.transcripts,
            vec![(p("a.txt"), p("a.mp4")), (p("a.srt"), p("a.wav"))]
        );
    }
}

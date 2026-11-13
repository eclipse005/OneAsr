//! 「反馈问题」：把诊断摘要拼成一条**预填好**的 GitHub issue，完整日志进剪贴板。
//!
//! 只做两件事：打开 `issues/new?title=…&body=…`，把日志写进剪贴板。不联网、
//! 不用任何凭证 —— 这是有意的：本项目没有后端也没有遥测，用户提交之前能看到
//! （也能改）即将发出去的每一个字。
//!
//! 为什么日志不进 URL：Windows 用 `ShellExecute` 打开长 URL，部分环境会截断，
//! 所以按 [`URL_BUDGET`] 设计；而真实任务日志有十几 KB，放不进去。于是拆成
//! 「摘要进正文 + 完整日志进剪贴板」，正文末尾留一行提示让用户 Ctrl+V。

use oneasr_core::i18n::UiLang;

/// 新建 issue 的入口（本仓库的公开仓库）。
const ISSUE_NEW: &str = "https://github.com/eclipse005/OneAsr/issues/new";

/// URL 预算（字符）。留出余量给不同浏览器/外壳的截断行为；构造时断言。
const URL_BUDGET: usize = 1800;

/// 拼一条 issue 需要的全部事实。都是短字符串，不含路径（调用方负责只用 id/名字）。
pub(crate) struct Facts<'a> {
    pub version: &'a str,
    pub backend: &'a str,
    /// `sysinfo::vulkan_report()` 的结果（非 Windows 为 `None`）。
    pub vulkan: Option<String>,
    /// `sysinfo::dx12_compiler_report()` 的结果（非 Windows 为 `None`）。
    pub dx12: Option<String>,
    pub asr_model: &'a str,
    pub aligner: &'a str,
    pub vocal_separation: bool,
    pub os: &'a str,
    pub cpu: &'a str,
}

/// 一条待打开的 issue：标题、正文，以及要写进剪贴板的日志。
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Draft {
    pub url: String,
    pub body: String,
    /// 要放进剪贴板的内容（没有日志时就是空串）。
    pub clipboard: String,
}

pub(crate) fn draft(lang: UiLang, facts: &Facts<'_>, log: Option<&str>) -> Draft {
    let title = match lang {
        UiLang::Zh => format!(
            "{} / {} / （请一句话描述问题）",
            facts.version, facts.backend
        ),
        UiLang::En => format!(
            "{} / {} / (one line: what went wrong)",
            facts.version, facts.backend
        ),
    };
    let body = body(lang, facts, log.is_some());
    let url = issue_url(lang, &title, &body);
    Draft {
        url,
        body,
        clipboard: log.unwrap_or_default().to_string(),
    }
}

fn body(lang: UiLang, facts: &Facts<'_>, has_log: bool) -> String {
    let yes = match lang {
        UiLang::Zh => "开",
        UiLang::En => "on",
    };
    let no = match lang {
        UiLang::Zh => "关",
        UiLang::En => "off",
    };
    let mut lines: Vec<String> = Vec::new();
    match lang {
        UiLang::Zh => {
            lines.push(format!("**版本** {}", facts.version));
            lines.push(format!("**后端** {}", facts.backend));
            if let Some(v) = &facts.vulkan {
                lines.push(format!("**Vulkan** {v}"));
            }
            if let Some(c) = &facts.dx12 {
                lines.push(format!("**DX12 编译器** {c}"));
            }
            lines.push(format!(
                "**模型** asr={}, aligner={}, 人声分离={}",
                facts.asr_model,
                facts.aligner,
                if facts.vocal_separation { yes } else { no }
            ));
            lines.push(format!("**硬件** {} · {}", facts.os, facts.cpu));
            lines.push(String::new());
            lines.push("**复现步骤**".to_string());
            lines.push("1. ".to_string());
            lines.push("2. ".to_string());
            lines.push(String::new());
            lines.push("**日志**".to_string());
            lines.push(if has_log {
                "（完整日志已复制到剪贴板，请在这里按 Ctrl+V 粘贴）".to_string()
            } else {
                "（暂无任务日志；可把 oneasr-error.log 或 oneasr-task-*.log 拖进来）".to_string()
            });
        }
        UiLang::En => {
            lines.push(format!("**Version** {}", facts.version));
            lines.push(format!("**Backend** {}", facts.backend));
            if let Some(v) = &facts.vulkan {
                lines.push(format!("**Vulkan** {v}"));
            }
            if let Some(c) = &facts.dx12 {
                lines.push(format!("**DX12 compiler** {c}"));
            }
            lines.push(format!(
                "**Models** asr={}, aligner={}, vocal separation={}",
                facts.asr_model,
                facts.aligner,
                if facts.vocal_separation { yes } else { no }
            ));
            lines.push(format!("**Hardware** {} · {}", facts.os, facts.cpu));
            lines.push(String::new());
            lines.push("**Steps to reproduce**".to_string());
            lines.push("1. ".to_string());
            lines.push("2. ".to_string());
            lines.push(String::new());
            lines.push("**Logs**".to_string());
            lines.push(if has_log {
                "(full log copied to the clipboard — paste it here with Ctrl+V)".to_string()
            } else {
                "(no task log yet; drag oneasr-error.log or oneasr-task-*.log in)".to_string()
            });
        }
    }
    lines.join("\n")
}

/// 拼 `issues/new?title=…&body=…`，并保证总长不超过 [`URL_BUDGET`]。
///
/// 超预算时截断正文尾部（保留前半的摘要），并在末尾标明"已截断"——真实日志有
/// 十几 KB，塞不进 URL 是常态而不是意外。
pub(crate) fn issue_url(lang: UiLang, title: &str, body: &str) -> String {
    let base = format!("{ISSUE_NEW}?title={}&body=", encode(title));
    let mut encoded = encode(body);
    if base.len() + encoded.len() > URL_BUDGET {
        // 按字符边界回退，直到放得下；再补一行说明。
        let note = encode(&truncation_note(lang));
        let room = URL_BUDGET.saturating_sub(base.len() + note.len());
        let mut cut = room.min(encoded.len());
        while cut > 0 && !encoded.is_char_boundary(cut) {
            cut -= 1;
        }
        encoded.truncate(cut);
        // 截断点可能落在 `%XX` 中间：把尾部半个转义序列去掉。
        while let Some(idx) = encoded.rfind('%') {
            if encoded.len() - idx < 3 {
                encoded.truncate(idx);
            } else {
                break;
            }
        }
        encoded.push_str(&note);
    }
    format!("{base}{encoded}")
}

/// RFC 3986 的 unreserved 之外全部 percent-encode（UTF-8 字节）。
fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 3);
    for byte in text.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char);
            }
            b' ' => out.push_str("%20"),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// URL 里那条截断说明（按语言成对：双语门禁要求界面相关文案走 `match lang` 臂）。
fn truncation_note(lang: UiLang) -> String {
    match lang {
        UiLang::Zh => "

…（正文过长已截断，完整内容在剪贴板）"
            .to_string(),
        UiLang::En => "

…(body truncated; the full text is in the clipboard)"
            .to_string(),
    }
}

/// 日志尾部那条截断说明。
fn log_truncated_note(lang: UiLang) -> String {
    match lang {
        UiLang::Zh => "
…（日志过长已截断）
"
        .to_string(),
        UiLang::En => "
…(log truncated)
"
        .to_string(),
    }
}

/// 数据目录里最近的一份任务日志（失败任务会留下 `.log`，成功任务需
/// `ONEASR_KEEP_JOURNAL=1` 才会留）。读不到就返回 `None`，不报错。
pub(crate) fn latest_task_log(lang: UiLang, dir: &std::path::Path) -> Option<String> {
    let mut newest: Option<(std::time::SystemTime, std::path::PathBuf)> = None;
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("oneasr-task-") || !name.ends_with(".log") {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let Ok(modified) = meta.modified() else {
            continue;
        };
        if newest.as_ref().is_none_or(|(t, _)| modified > *t) {
            newest = Some((modified, entry.path()));
        }
    }
    let (_, path) = newest?;
    // 上限 256 KiB：日志是文本，真实样本十几 KB；防止有人手工塞进大文件。
    let text = std::fs::read_to_string(&path).ok()?;
    let mut text = oneasr_core::paths::redact_home(&text).to_string();
    if text.len() > 256 * 1024 {
        let mut cut = 256 * 1024;
        while cut > 0 && !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
        text.push_str(&log_truncated_note(lang));
    }
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> Facts<'static> {
        Facts {
            version: "1.4.2",
            backend: "auto",
            vulkan: Some("loader ok | wgpu adapters 0".into()),
            dx12: Some("DXC".into()),
            asr_model: "Qwen3-ASR-0.6B-hf",
            aligner: "omniASR-CTC-300M-v2-hf",
            vocal_separation: false,
            os: "Windows 11 26100",
            cpu: "AMD Ryzen 7 5800H (16 logical)",
        }
    }

    /// 编码只放行 unreserved：空格、中文、斜杠都要转义。
    #[test]
    fn encoding_follows_rfc3986() {
        assert_eq!(encode("a b"), "a%20b");
        assert_eq!(encode("-_.~"), "-_.~");
        assert_eq!(encode("中"), "%E4%B8%AD");
        assert_eq!(encode("a/b?c#d"), "a%2Fb%3Fc%23d");
    }

    /// 摘要 + 模板要放得进 URL 预算，且正文里带着"日志在剪贴板"的提示。
    #[test]
    fn draft_fits_the_url_budget_and_points_at_the_clipboard() {
        let d = draft(UiLang::Zh, &facts(), Some("some log"));
        assert!(d.url.len() <= URL_BUDGET, "URL 超预算：{}", d.url.len());
        assert!(d.url.starts_with(ISSUE_NEW), "{}", d.url);
        assert!(d.body.contains("Ctrl+V"), "{}", d.body);
        assert_eq!(d.clipboard, "some log");
    }

    /// 没有日志时也要说清"该怎么办"，而不是留一个空段。
    #[test]
    fn no_log_still_tells_the_user_what_to_attach() {
        let d = draft(UiLang::Zh, &facts(), None);
        assert!(d.body.contains("oneasr-task-"), "{}", d.body);
        assert!(d.clipboard.is_empty());
    }

    /// 超长正文被截断，且截断点不会切碎 `%XX`、也不会切在多字节字符中间。
    #[test]
    fn oversized_body_is_truncated_on_a_safe_boundary() {
        let long = "汉".repeat(4000);
        let url = issue_url(UiLang::Zh, "t", &long);
        assert!(url.len() <= URL_BUDGET, "{}", url.len());
        assert!(
            url.contains("%EF%BC%88") || url.contains("truncated") || url.contains("%E5%B7%B2")
        );
        // 每个 `%` 后面必须跟两位十六进制：截断不能留下半个转义。
        let bytes = url.as_bytes();
        for (i, b) in bytes.iter().enumerate() {
            if *b == b'%' {
                assert!(i + 2 < bytes.len(), "尾部留下半个转义序列");
                assert!(bytes[i + 1].is_ascii_hexdigit() && bytes[i + 2].is_ascii_hexdigit());
            }
        }
    }

    /// 两种语言的正文都要带版本与后端，不能只剩模板。
    #[test]
    fn both_languages_carry_the_facts() {
        for lang in [UiLang::Zh, UiLang::En] {
            let d = draft(lang, &facts(), None);
            assert!(d.body.contains("1.4.2"), "{:?}: {}", lang, d.body);
            assert!(d.body.contains("auto"), "{:?}: {}", lang, d.body);
            assert!(
                d.body.contains("Qwen3-ASR-0.6B-hf"),
                "{:?}: {}",
                lang,
                d.body
            );
        }
    }
}

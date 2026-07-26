//! Resolve a UI font that still paints Chinese on stripped / customized Windows installs.
//!
//! Default was bare `Segoe UI` with system CJK fallback only. On lite Win10 images
//! (or after deleting 微软雅黑) that chain can break during heavy Chinese layout
//! (settings panel scroll). We:
//! 1. Prefer the first *installed* family from a sane candidate list
//! 2. Attach an explicit DirectWrite `FontFallbacks` list of every remaining CJK-capable face

use gpui::{font, App, Font, FontFallbacks};

/// Preferred UI primaries (first installed wins).
const PRIMARY_CANDIDATES: &[&str] = &[
    "Segoe UI",
    "Microsoft YaHei UI",
    "Microsoft YaHei",
    "微软雅黑",
    "Microsoft JhengHei UI",
    "Microsoft JhengHei",
    "DengXian",
    "等线",
    "NSimSun",
    "新宋体",
    "SimSun",
    "宋体",
    "Arial",
    "Tahoma",
];

/// Glyph fallback order for characters the primary face lacks (CJK etc.).
const FALLBACK_CANDIDATES: &[&str] = &[
    "Microsoft YaHei UI",
    "Microsoft YaHei",
    "微软雅黑",
    "Microsoft JhengHei UI",
    "Microsoft JhengHei",
    "DengXian",
    "等线",
    "NSimSun",
    "新宋体",
    "SimSun",
    "宋体",
    "Segoe UI",
    "Arial",
    "Tahoma",
    "Malgun Gothic",
    "MS Gothic",
    "Yu Gothic UI",
];

/// Result of probing the machine fonts (for UI + log).
#[derive(Clone)]
pub struct UiFontPlan {
    pub font: Font,
    pub primary: String,
    pub fallbacks: Vec<String>,
    pub installed_hits: Vec<String>,
}

impl Default for UiFontPlan {
    fn default() -> Self {
        // Pre-window placeholder; replaced in `main` once `App` exists.
        Self {
            font: font("Segoe UI"),
            primary: "Segoe UI".into(),
            fallbacks: Vec::new(),
            installed_hits: Vec::new(),
        }
    }
}

/// Probe installed families and build a primary + fallback stack.
pub fn resolve(cx: &App) -> UiFontPlan {
    let installed = cx.text_system().all_font_names();
    resolve_from_names(&installed)
}

fn resolve_from_names(installed: &[String]) -> UiFontPlan {
    let has = |want: &str| {
        installed
            .iter()
            .any(|n| n.eq_ignore_ascii_case(want) || n == want)
    };
    // Prefer the spelling returned by the OS when present (locale-specific names).
    let canonical = |want: &str| -> String {
        installed
            .iter()
            .find(|n| n.eq_ignore_ascii_case(want) || n.as_str() == want)
            .cloned()
            .unwrap_or_else(|| want.to_string())
    };

    let mut installed_hits = Vec::new();
    for name in PRIMARY_CANDIDATES
        .iter()
        .chain(FALLBACK_CANDIDATES.iter())
    {
        if has(name) {
            let c = canonical(name);
            if !installed_hits.iter().any(|h: &String| h == &c) {
                installed_hits.push(c);
            }
        }
    }

    let primary = PRIMARY_CANDIDATES
        .iter()
        .find(|n| has(n))
        .map(|n| canonical(n))
        .unwrap_or_else(|| "Segoe UI".into());

    let mut fallbacks = Vec::new();
    for name in FALLBACK_CANDIDATES {
        if !has(name) {
            continue;
        }
        let c = canonical(name);
        if c.eq_ignore_ascii_case(&primary) {
            continue;
        }
        if !fallbacks.iter().any(|f: &String| f == &c) {
            fallbacks.push(c);
        }
    }

    let font = Font {
        family: primary.clone().into(),
        features: Default::default(),
        fallbacks: if fallbacks.is_empty() {
            None
        } else {
            Some(FontFallbacks::from_fonts(fallbacks.clone()))
        },
        weight: Default::default(),
        style: Default::default(),
    };

    UiFontPlan {
        font,
        primary,
        fallbacks,
        installed_hits,
    }
}

/// Human-readable block for `oneasr-error.log`.
pub fn diagnose_text(plan: &UiFontPlan) -> String {
    let mut out = String::new();
    out.push_str(&format!("UI primary font: {}\n", plan.primary));
    if plan.fallbacks.is_empty() {
        out.push_str("UI CJK fallbacks: (none installed from candidate list)\n");
    } else {
        out.push_str("UI CJK fallbacks:\n");
        for f in &plan.fallbacks {
            out.push_str(&format!("  - {f}\n"));
        }
    }
    out.push_str("Installed candidates seen:\n");
    if plan.installed_hits.is_empty() {
        out.push_str("  (none — stripped font set; may be unstable)\n");
    } else {
        for h in &plan.installed_hits {
            out.push_str(&format!("  - {h}\n"));
        }
    }
    let yahei = plan
        .installed_hits
        .iter()
        .any(|h| h.contains("YaHei") || h.contains("雅黑"));
    if !yahei {
        out.push_str(
            "note: Microsoft YaHei not found — using alternate CJK face if available\n",
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_nsimsun_when_yahei_gone() {
        let names = vec![
            "Segoe UI".into(),
            "NSimSun".into(),
            "Arial".into(),
        ];
        let plan = resolve_from_names(&names);
        assert_eq!(plan.primary, "Segoe UI");
        assert!(plan.fallbacks.iter().any(|f| f == "NSimSun"));
        assert!(!plan.fallbacks.iter().any(|f| f.contains("YaHei")));
    }

    #[test]
    fn primary_falls_to_nsimsun_without_segoe() {
        let names = vec!["NSimSun".into(), "Arial".into()];
        let plan = resolve_from_names(&names);
        assert_eq!(plan.primary, "NSimSun");
    }
}

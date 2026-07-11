//! Official English prompts only — from MOSS `examples/prompts.md`.
//!
//! **Prompt A** (default, timestamped diarization):
//! ```text
//! Transcribe the audio. For each segment, start with the timestamp and speaker ID
//! ([S01], [S02], [S03], ...), then the spoken text, and end with the segment timestamp.
//! ```
//!
//! **Prompt B** (hotword hints) = Prompt A + ` Hotwords: {list}`  
//! (official English hotword recipe).

use thiserror::Error;

/// Official English Prompt A — timestamped diarization (no hotwords).
pub const PROMPT_BASE: &str = "Transcribe the audio. For each segment, start with the timestamp and speaker ID ([S01], [S02], [S03], ...), then the spoken text, and end with the segment timestamp.";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PromptError {
    #[error("hotwords are enabled but the hotword list is empty")]
    EmptyHotwords,
}

/// Build the official English prompt.
///
/// * `use_hotwords == false` → Prompt A  
/// * `use_hotwords == true`  → Prompt B (`… Hotwords: …`); non-empty list required
pub fn build_prompt(use_hotwords: bool, hotwords: &str) -> Result<String, PromptError> {
    if !use_hotwords {
        return Ok(PROMPT_BASE.to_string());
    }
    let hw = hotwords.trim();
    if hw.is_empty() {
        return Err(PromptError::EmptyHotwords);
    }
    // Official English hotword form from examples/prompts.md
    Ok(format!("{PROMPT_BASE} Hotwords: {hw}"))
}

/// Human-readable summary for UI (does not fail on empty hotwords when disabled).
pub fn prompt_preview(use_hotwords: bool, hotwords: &str) -> String {
    match build_prompt(use_hotwords, hotwords) {
        Ok(p) => p,
        Err(_) => format!("{PROMPT_BASE} Hotwords: (请填写热词)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_a_no_hotwords() {
        let p = build_prompt(false, "ignored").unwrap();
        assert_eq!(p, PROMPT_BASE);
        assert!(!p.contains("Hotwords"));
    }

    #[test]
    fn prompt_b_with_hotwords() {
        let p = build_prompt(true, " OpenAI , Sora ").unwrap();
        assert!(p.starts_with(PROMPT_BASE));
        assert!(p.ends_with("Hotwords: OpenAI , Sora"));
    }

    #[test]
    fn prompt_b_rejects_empty() {
        assert_eq!(build_prompt(true, "  "), Err(PromptError::EmptyHotwords));
        assert_eq!(build_prompt(true, ""), Err(PromptError::EmptyHotwords));
    }

    #[test]
    fn official_english_hotword_recipe() {
        let p = build_prompt(true, "OpenAI, Sora").unwrap();
        assert_eq!(
            p,
            "Transcribe the audio. For each segment, start with the timestamp and speaker ID ([S01], [S02], [S03], ...), then the spoken text, and end with the segment timestamp. Hotwords: OpenAI, Sora"
        );
    }
}

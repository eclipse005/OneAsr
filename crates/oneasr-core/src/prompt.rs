//! Official English prompts — from MOSS `examples/prompts.md`.

use thiserror::Error;

const PROMPT_BASE: &str = "Transcribe the audio. For each segment, start with the timestamp and speaker ID ([S01], [S02], [S03], ...), then the spoken text, and end with the segment timestamp.";

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
    Ok(format!("{PROMPT_BASE} Hotwords: {hw}"))
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

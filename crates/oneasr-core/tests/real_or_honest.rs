//! Settings defaults should point at real local Qwen model dirs when present.

use std::path::PathBuf;

use oneasr_core::{check_aligner_model_dir, check_asr_model_dir, Settings};

#[test]
fn default_settings_have_distinct_asr_and_aligner_dirs() {
    let s = Settings::default();
    assert_ne!(s.asr_model_dir, s.aligner_model_dir);
    assert!(!s.asr_model_dir.as_os_str().is_empty());
    assert!(!s.aligner_model_dir.as_os_str().is_empty());
}

#[test]
fn check_dirs_honest_on_missing() {
    let missing = PathBuf::from(r"D:\__oneasr_no_such_model__");
    assert!(check_asr_model_dir(&missing).is_err());
    assert!(check_aligner_model_dir(&missing).is_err());
}

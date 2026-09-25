//! Model-directory validation: does this folder hold a complete, loadable set of weights? Cached, because the settings drawer probes on every repaint.

use super::AsrError;
use crate::i18n::{self, ROLE_ALIGNER, ROLE_ASR, ROLE_DEMUCS};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::Deserialize;

// Ready-dir cache: sequential jobs hit the same model path; a poisoned lock
// is a miss (re-stat) rather than a worker panic. Failures are not cached —
// a download may complete between probes.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum ModelRole {
    Asr,
    Aligner,
    Demucs,
}

static MODEL_DIR_READY: OnceLock<Mutex<HashSet<(ModelRole, PathBuf)>>> = OnceLock::new();

fn model_dir_ready_cache() -> &'static Mutex<HashSet<(ModelRole, PathBuf)>> {
    MODEL_DIR_READY.get_or_init(|| Mutex::new(HashSet::new()))
}

fn cached_ready_hit(role: ModelRole, canonical: &Path) -> bool {
    match model_dir_ready_cache().lock() {
        Ok(guard) => guard.contains(&(role, canonical.to_path_buf())),
        Err(_) => false,
    }
}

fn cached_ready_store(role: ModelRole, canonical: PathBuf) {
    if let Ok(mut guard) = model_dir_ready_cache().lock() {
        guard.insert((role, canonical));
    }
}

/// Check ASR model directory with success-path caching.
///
/// Catalog-named install-layout dirs are validated against the catalog's exact
/// file sizes (same criterion used after download); custom user-picked dirs
/// fall back to an existence + non-trivial-size check so alternate copies of
/// the same model still work.
pub fn check_asr_model_dir(model_dir: &Path) -> Result<(), AsrError> {
    run_cached_model_check(ModelRole::Asr, model_dir, || {
        match crate::model::ModelId::try_from_asr_dir(model_dir) {
            Some(id) => check_model_dir_against_catalog(ROLE_ASR, model_dir, id),
            None => check_model_dir_inner(
                ROLE_ASR,
                model_dir,
                &["config.json", "tokenizer.json"],
            ),
        }
    })
}

/// Check Aligner model directory with success-path caching.
///
/// The install-layout dir (`Qwen3-ForcedAligner-0.6B`) is validated against
/// the catalog's exact sizes; custom user-picked dirs fall back to an
/// existence + non-trivial-size check.
pub fn check_aligner_model_dir(model_dir: &Path) -> Result<(), AsrError> {
    run_cached_model_check(ModelRole::Aligner, model_dir, || {
        match crate::model::ModelId::try_from_aligner_dir(model_dir) {
            Some(id) => check_model_dir_against_catalog(ROLE_ALIGNER, model_dir, id),
            None => check_model_dir_inner(
                ROLE_ALIGNER,
                model_dir,
                &["config.json", "tokenizer.json", "tokenizer_config.json"],
            ),
        }
    })
}

/// Check the optional HTDemucs weights directory (vocal separation).
///
/// Same rule as the ASR / aligner dirs: the install-layout folder is validated
/// against the catalog's exact size; a custom user-picked folder only has to
/// hold the one weights file the loader expects.
pub fn check_demucs_model_dir(model_dir: &Path) -> Result<(), AsrError> {
    run_cached_model_check(ModelRole::Demucs, model_dir, || {
        let is_install_layout = model_dir
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.eq_ignore_ascii_case(crate::model::HTDEMUCS_FT));
        if is_install_layout {
            return check_model_dir_against_catalog(
                ROLE_DEMUCS,
                model_dir,
                crate::model::ModelId::HtdemucsFt,
            );
        }
        if !model_dir.is_dir() {
            return Err(AsrError::Other(i18n::model_dir_missing(
                ROLE_DEMUCS,
                model_dir,
            )));
        }
        let mut missing = Vec::new();
        check_weight_file(
            model_dir,
            crate::engine::local::DEMUCS_WEIGHTS_FILE,
            &mut missing,
        );
        if missing.is_empty() {
            Ok(())
        } else {
            Err(AsrError::ModelIncomplete {
                role: ROLE_DEMUCS,
                missing: join_missing(&missing),
                dir: model_dir.display().to_string(),
            })
        }
    })
}

fn run_cached_model_check(
    role: ModelRole,
    model_dir: &Path,
    check: impl FnOnce() -> Result<(), AsrError>,
) -> Result<(), AsrError> {
    if let Ok(canonical) = std::fs::canonicalize(model_dir) {
        if cached_ready_hit(role, &canonical) {
            return Ok(());
        }
        check()?;
        cached_ready_store(role, canonical);
        return Ok(());
    }
    check()
}

/// Validate a directory against a catalog definition (present + size contract).
fn check_model_dir_against_catalog(
    role: i18n::Str,
    model_dir: &Path,
    id: crate::model::ModelId,
) -> Result<(), AsrError> {
    if !model_dir.is_dir() {
        return Err(AsrError::Other(i18n::model_dir_missing(role, model_dir)));
    }
    let definition = crate::model::model_definition(id);
    let missing: Vec<String> = definition
        .download_files
        .iter()
        .filter_map(|file| {
            let path = model_dir.join(&file.file_name);
            if crate::model::file_meets_ready_threshold(&path, file.expected_size) {
                return None;
            }
            let actual = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            Some(if actual == 0 {
                i18n::file_missing(&file.file_name)
            } else {
                i18n::file_size_mismatch(&file.file_name, actual, file.expected_size)
            })
        })
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(AsrError::ModelIncomplete {
            role,
            missing: join_missing(&missing),
            dir: model_dir.display().to_string(),
        })
    }
}

fn check_model_dir_inner(
    role: i18n::Str,
    model_dir: &Path,
    required: &[&str],
) -> Result<(), AsrError> {
    if !model_dir.is_dir() {
        return Err(AsrError::Other(i18n::model_dir_missing(role, model_dir)));
    }
    let mut missing = Vec::new();
    for name in required {
        if !model_dir.join(name).is_file() {
            missing.push((*name).to_string());
        }
    }
    match weight_filenames(model_dir) {
        Ok(files) => {
            for name in files {
                check_weight_file(model_dir, &name, &mut missing);
            }
        }
        Err(item) => missing.push(item),
    }
    if missing.is_empty() {
        Ok(())
    } else {
        Err(AsrError::ModelIncomplete {
            role,
            missing: join_missing(&missing),
            dir: model_dir.display().to_string(),
        })
    }
}

/// Hugging Face `weight_map` is tensor-name → shard filename.
#[derive(Deserialize)]
struct SafetensorsIndex {
    weight_map: HashMap<String, String>,
}

/// Filenames that must exist as weight payloads (unique shard names, or the
/// single `model.safetensors`).
fn weight_filenames(model_dir: &Path) -> Result<Vec<String>, String> {
    let index = model_dir.join("model.safetensors.index.json");
    let single = model_dir.join("model.safetensors");
    if index.is_file() {
        shard_names_from_index(&index)
    } else if single.is_file() {
        Ok(vec!["model.safetensors".into()])
    } else {
        Err(i18n::weight_file_missing())
    }
}

fn shard_names_from_index(index: &Path) -> Result<Vec<String>, String> {
    let text = std::fs::read_to_string(index)
        .map_err(|e| format!("model.safetensors.index.json ({e})"))?;
    let parsed: SafetensorsIndex =
        serde_json::from_str(&text).map_err(|e| format!("model.safetensors.index.json ({e})"))?;
    let mut names: Vec<String> = parsed.weight_map.into_values().collect();
    names.sort();
    names.dedup();
    if names.is_empty() {
        Err(i18n::weight_map_empty())
    } else {
        Ok(names)
    }
}

/// Truncated / placeholder downloads are typically a few hundred bytes.
const MIN_WEIGHT_FILE_BYTES: u64 = 1024;

fn check_weight_file(dir: &Path, name: &str, missing: &mut Vec<String>) {
    let path = dir.join(name);
    match std::fs::metadata(&path) {
        Ok(meta) if meta.is_file() && meta.len() >= MIN_WEIGHT_FILE_BYTES => {}
        Ok(meta) if meta.is_file() => missing.push(i18n::file_too_small(name, meta.len())),
        _ => missing.push(i18n::file_absent(name)),
    }
}

fn join_missing(names: &[String]) -> String {
    const SHOW: usize = 12;
    if names.len() <= SHOW {
        names.join(", ")
    } else {
        i18n::missing_summary(&names[..SHOW].join(", "), names.len())
    }
}

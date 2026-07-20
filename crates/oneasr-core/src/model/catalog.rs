//! File lists + ModelScope URLs (same pattern as VoxTrans).

use std::path::PathBuf;

use super::path::resolve_model_dir;

pub const QWEN3_ASR_06B: &str = "Qwen3-ASR-0.6B";
pub const QWEN3_ASR_17B: &str = "Qwen3-ASR-1.7B";
pub const QWEN_ALIGN_06B: &str = "Qwen3-ForcedAligner-0.6B";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelKind {
    Asr,
    Align,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModelId {
    Qwen3Asr06B,
    Qwen3Asr17B,
    QwenAlign06B,
}

impl ModelId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Qwen3Asr06B => QWEN3_ASR_06B,
            Self::Qwen3Asr17B => QWEN3_ASR_17B,
            Self::QwenAlign06B => QWEN_ALIGN_06B,
        }
    }

    pub fn kind(self) -> ModelKind {
        match self {
            Self::Qwen3Asr06B | Self::Qwen3Asr17B => ModelKind::Asr,
            Self::QwenAlign06B => ModelKind::Align,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Qwen3Asr06B => "Qwen3-ASR 0.6B",
            Self::Qwen3Asr17B => "Qwen3-ASR 1.7B",
            Self::QwenAlign06B => "ForcedAligner 0.6B",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModelDownloadFile {
    pub file_name: String,
    pub url: String,
    pub expected_size: u64,
}

#[derive(Debug, Clone)]
pub struct ModelDefinition {
    pub id: ModelId,
    pub model_dir: PathBuf,
    pub required_files: Vec<String>,
    pub download_files: Vec<ModelDownloadFile>,
}

pub fn model_definition(id: ModelId) -> ModelDefinition {
    let name = id.as_str();
    let files = match id {
        ModelId::Qwen3Asr06B => qwen3_asr_06b_files(),
        ModelId::Qwen3Asr17B => qwen3_asr_17b_files(),
        ModelId::QwenAlign06B => qwen_align_files(),
    };
    ModelDefinition {
        id,
        model_dir: resolve_model_dir(name),
        required_files: files.iter().map(|(n, _)| (*n).to_string()).collect(),
        download_files: files
            .iter()
            .map(|(file_name, expected_size)| ModelDownloadFile {
                file_name: (*file_name).to_string(),
                url: modelscope_url(name, file_name),
                expected_size: *expected_size,
            })
            .collect(),
    }
}

fn modelscope_url(model: &str, file_name: &str) -> String {
    format!("https://modelscope.cn/models/eclipse005/{model}/resolve/master/{file_name}")
}

fn qwen3_asr_06b_files() -> Vec<(&'static str, u64)> {
    vec![
        ("config.json", 6_413),
        ("model.safetensors", 1_876_091_704),
        ("tokenizer.json", 4_760_186),
    ]
}

fn qwen3_asr_17b_files() -> Vec<(&'static str, u64)> {
    vec![
        ("config.json", 6_194),
        ("model-00001-of-00002.safetensors", 4_220_320_824),
        ("model-00002-of-00002.safetensors", 478_200_688),
        ("model.safetensors.index.json", 64_821),
        ("tokenizer.json", 4_760_186),
    ]
}

fn qwen_align_files() -> Vec<(&'static str, u64)> {
    // Same required set as VoxTrans (Japanese nagisa is optional at load time).
    vec![
        ("config.json", 5_982),
        ("merges.txt", 1_671_853),
        ("model.safetensors", 1_835_544_544),
        ("tokenizer_config.json", 12_666),
        ("vocab.json", 2_776_833),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asr_and_align_catalogs_are_distinct() {
        let asr = model_definition(ModelId::Qwen3Asr06B);
        let align = model_definition(ModelId::QwenAlign06B);
        assert_eq!(asr.id.as_str(), QWEN3_ASR_06B);
        assert_eq!(align.id.as_str(), QWEN_ALIGN_06B);
        assert!(!asr.download_files.is_empty());
        assert!(!align.download_files.is_empty());
        assert!(asr.download_files.iter().all(|f| f.url.contains("Qwen3-ASR")));
        assert!(align
            .download_files
            .iter()
            .all(|f| f.url.contains("ForcedAligner")));
    }
}

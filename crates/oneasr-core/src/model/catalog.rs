//! File lists + ModelScope URLs (VoxTrans-aligned).

use std::path::PathBuf;

use super::path::{resolve_dll_dir, resolve_model_dir};

pub const QWEN3_ASR_06B: &str = "Qwen3-ASR-0.6B";
pub const QWEN3_ASR_17B: &str = "Qwen3-ASR-1.7B";
pub const QWEN_ALIGN_06B: &str = "Qwen3-ForcedAligner-0.6B";

/// Same ModelScope repo as VoxTrans install-time CUDA runtime download.
const CUDA_RUNTIME_REPO: &str = "eclipse005/cuda-runtime-12.8";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelKind {
    Asr,
    Align,
    /// User-mode CUDA 12.x DLLs next to the app (not under models/).
    CudaRuntime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModelId {
    Qwen3Asr06B,
    Qwen3Asr17B,
    QwenAlign06B,
    /// cudart / cublas / cublasLt / curand (scheme B, no NVRTC).
    CudaRuntime,
}

impl ModelId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Qwen3Asr06B => QWEN3_ASR_06B,
            Self::Qwen3Asr17B => QWEN3_ASR_17B,
            Self::QwenAlign06B => QWEN_ALIGN_06B,
            Self::CudaRuntime => "cuda-runtime-12.8",
        }
    }

    pub fn kind(self) -> ModelKind {
        match self {
            Self::Qwen3Asr06B | Self::Qwen3Asr17B => ModelKind::Asr,
            Self::QwenAlign06B => ModelKind::Align,
            Self::CudaRuntime => ModelKind::CudaRuntime,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Qwen3Asr06B => "Qwen3-ASR 0.6B",
            Self::Qwen3Asr17B => "Qwen3-ASR 1.7B",
            Self::QwenAlign06B => "ForcedAligner 0.6B",
            Self::CudaRuntime => "CUDA 运行库",
        }
    }

    /// Short chip label for ASR size picker.
    pub fn short_label(self) -> &'static str {
        match self {
            Self::Qwen3Asr06B => "0.6B",
            Self::Qwen3Asr17B => "1.7B",
            Self::QwenAlign06B => "0.6B",
            Self::CudaRuntime => "CUDA",
        }
    }

    pub const ASR_CHOICES: [ModelId; 2] = [ModelId::Qwen3Asr06B, ModelId::Qwen3Asr17B];

    /// Parse a catalog name / settings field into an ASR model id.
    ///
    /// Only exact folder names match (`Qwen3-ASR-0.6B` / `Qwen3-ASR-1.7B`);
    /// unknown strings fall back to 0.6B.
    pub fn parse_asr(raw: &str) -> Self {
        Self::try_parse_asr(raw).unwrap_or(Self::Qwen3Asr06B)
    }

    /// Exact catalog-name match only (no substring heuristics).
    pub fn try_parse_asr(raw: &str) -> Option<Self> {
        let s = raw.trim();
        if s.eq_ignore_ascii_case(QWEN3_ASR_17B) {
            Some(Self::Qwen3Asr17B)
        } else if s.eq_ignore_ascii_case(QWEN3_ASR_06B) {
            Some(Self::Qwen3Asr06B)
        } else {
            None
        }
    }

    /// Infer ASR model from install-layout folder name, if recognized.
    pub fn try_from_asr_dir(path: &std::path::Path) -> Option<Self> {
        path.file_name()
            .and_then(|n| n.to_str())
            .and_then(Self::try_parse_asr)
    }

    /// Infer ASR model from directory folder name (default 0.6B if not catalog).
    pub fn from_asr_dir(path: &std::path::Path) -> Self {
        Self::try_from_asr_dir(path).unwrap_or(Self::Qwen3Asr06B)
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
    match id {
        ModelId::Qwen3Asr06B => model_def(id, resolve_model_dir(QWEN3_ASR_06B), qwen3_asr_06b_files(), modelscope_model_url),
        ModelId::Qwen3Asr17B => model_def(id, resolve_model_dir(QWEN3_ASR_17B), qwen3_asr_17b_files(), modelscope_model_url),
        ModelId::QwenAlign06B => model_def(id, resolve_model_dir(QWEN_ALIGN_06B), qwen_align_files(), modelscope_model_url),
        ModelId::CudaRuntime => model_def(
            id,
            resolve_dll_dir(),
            cuda_runtime_files(),
            cuda_runtime_url,
        ),
    }
}

fn model_def(
    id: ModelId,
    model_dir: PathBuf,
    files: Vec<(&'static str, u64)>,
    url_fn: fn(&str, &str) -> String,
) -> ModelDefinition {
    let name = id.as_str();
    ModelDefinition {
        id,
        model_dir,
        required_files: files.iter().map(|(n, _)| (*n).to_string()).collect(),
        download_files: files
            .iter()
            .map(|(file_name, expected_size)| ModelDownloadFile {
                file_name: (*file_name).to_string(),
                url: url_fn(name, file_name),
                expected_size: *expected_size,
            })
            .collect(),
    }
}

fn modelscope_model_url(model: &str, file_name: &str) -> String {
    format!("https://modelscope.cn/models/eclipse005/{model}/resolve/master/{file_name}")
}

/// VoxTrans install-time URLs (apply-cuda-runtime.ps1).
fn cuda_runtime_url(_model: &str, file_name: &str) -> String {
    format!("https://modelscope.cn/models/{CUDA_RUNTIME_REPO}/resolve/master/{file_name}")
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
    vec![
        ("config.json", 5_982),
        ("merges.txt", 1_671_853),
        ("model.safetensors", 1_835_544_544),
        ("tokenizer_config.json", 12_666),
        ("vocab.json", 2_776_833),
    ]
}

/// Same four DLLs as VoxTrans (no NVRTC — engines use precompiled PTX).
/// `expected_size` drives progress UI and readiness low-watermark (50% of size).
/// Content-Length overrides progress totals when present during download.
fn cuda_runtime_files() -> Vec<(&'static str, u64)> {
    vec![
        ("cudart64_12.dll", 539_648),
        ("cublas64_12.dll", 97_000_000),
        ("cublasLt64_12.dll", 450_000_000),
        ("curand64_10.dll", 59_000_000),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asr_and_align_catalogs_are_distinct() {
        let asr = model_definition(ModelId::Qwen3Asr06B);
        let align = model_definition(ModelId::QwenAlign06B);
        assert!(asr.download_files.iter().all(|f| f.url.contains("Qwen3-ASR")));
        assert!(align
            .download_files
            .iter()
            .all(|f| f.url.contains("ForcedAligner")));
    }

    #[test]
    fn cuda_runtime_uses_voxtrans_modelscope_repo() {
        let c = model_definition(ModelId::CudaRuntime);
        assert_eq!(c.required_files.len(), 4);
        assert!(c
            .download_files
            .iter()
            .all(|f| f.url.contains("cuda-runtime-12.8")));
        assert!(c.download_files.iter().any(|f| f.file_name == "cudart64_12.dll"));
        assert!(c
            .download_files
            .iter()
            .any(|f| f.file_name == "cublasLt64_12.dll"));
    }

    #[test]
    fn cuda_runtime_dir_is_dll_subdir_not_models() {
        use crate::model::path::{resolve_dll_dir, resolve_model_dir};
        let c = model_definition(ModelId::CudaRuntime);
        let models = resolve_model_dir("Qwen3-ASR-0.6B");
        assert_eq!(c.model_dir, resolve_dll_dir());
        assert!(c.model_dir.ends_with("dll"));
        assert_ne!(c.model_dir, models);
    }

    #[test]
    fn parse_asr_is_exact_catalog_name_only() {
        assert_eq!(ModelId::parse_asr(QWEN3_ASR_17B), ModelId::Qwen3Asr17B);
        assert_eq!(ModelId::parse_asr(QWEN3_ASR_06B), ModelId::Qwen3Asr06B);
        // No substring heuristics — "1.7" alone is not a catalog id.
        assert_eq!(ModelId::try_parse_asr("1.7"), None);
        assert_eq!(ModelId::parse_asr("something-1.7-else"), ModelId::Qwen3Asr06B);
        assert_eq!(
            ModelId::try_from_asr_dir(std::path::Path::new(r"C:\m\Qwen3-ASR-1.7B")),
            Some(ModelId::Qwen3Asr17B)
        );
    }
}

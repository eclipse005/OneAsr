//! File lists + ModelScope URLs (VoxTrans-aligned).

use std::path::PathBuf;

use super::path::{resolve_dll_dir, resolve_model_dir};

pub const QWEN3_ASR_06B: &str = "Qwen3-ASR-0.6B";
pub const QWEN3_ASR_17B: &str = "Qwen3-ASR-1.7B";
pub const QWEN_ALIGN_06B: &str = "Qwen3-ForcedAligner-0.6B";

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

    /// Infer the aligner model from an install-layout folder name only
    /// (`Qwen3-ForcedAligner-0.6B`); custom dir names return `None`.
    pub fn try_from_aligner_dir(path: &std::path::Path) -> Option<Self> {
        path.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.eq_ignore_ascii_case(QWEN_ALIGN_06B))
            .then_some(Self::QwenAlign06B)
    }
}

#[derive(Debug, Clone)]
pub struct ModelDownloadFile {
    pub file_name: String,
    pub url: String,
    /// Byte-exact size at the pinned ModelScope revision.
    pub expected_size: u64,
    /// Lowercase hex SHA-256 of the exact file content, verified after download.
    pub sha256: &'static str,
}

#[derive(Debug, Clone)]
pub struct ModelDefinition {
    pub id: ModelId,
    pub model_dir: PathBuf,
    pub required_files: Vec<String>,
    pub download_files: Vec<ModelDownloadFile>,
}

/// One downloadable artifact pinned to the ModelScope revision (commit) that
/// last changed the file; `size` + `sha256` come from the hub's file listing.
struct CatalogFile {
    file_name: &'static str,
    size: u64,
    sha256: &'static str,
    revision: &'static str,
}

pub fn model_definition(id: ModelId) -> ModelDefinition {
    match id {
        ModelId::Qwen3Asr06B => {
            model_def(id, resolve_model_dir(QWEN3_ASR_06B), qwen3_asr_06b_files())
        }
        ModelId::Qwen3Asr17B => {
            model_def(id, resolve_model_dir(QWEN3_ASR_17B), qwen3_asr_17b_files())
        }
        ModelId::QwenAlign06B => {
            model_def(id, resolve_model_dir(QWEN_ALIGN_06B), qwen_align_files())
        }
        ModelId::CudaRuntime => model_def(id, resolve_dll_dir(), cuda_runtime_files()),
    }
}

fn model_def(id: ModelId, model_dir: PathBuf, files: Vec<CatalogFile>) -> ModelDefinition {
    let repo = id.as_str();
    ModelDefinition {
        id,
        model_dir,
        required_files: files.iter().map(|f| f.file_name.to_string()).collect(),
        download_files: files
            .iter()
            .map(|f| ModelDownloadFile {
                file_name: f.file_name.to_string(),
                // Revision-pinned: content cannot drift with the `master` branch.
                url: format!(
                    "https://modelscope.cn/models/eclipse005/{repo}/resolve/{}/{file}",
                    f.revision,
                    file = f.file_name
                ),
                expected_size: f.size,
                sha256: f.sha256,
            })
            .collect(),
    }
}

fn qwen3_asr_06b_files() -> Vec<CatalogFile> {
    const REV: &str = "4b5b4c660dbfad71602d7bf63d4fc5139857a42e";
    vec![
        CatalogFile {
            file_name: "config.json",
            size: 6_193,
            sha256: "76d3ae4601ce939830b2517f4a6cadb86cc51316c3900af6b020b051c21a478c",
            revision: REV,
        },
        CatalogFile {
            file_name: "model.safetensors",
            size: 1_876_091_704,
            sha256: "79d6cbd4c98c7bbffe9db2edac07f56cd6637d0d5944b27f6c2b8353840323ea",
            revision: "c278b22bb38f6e3bd93b30693865569fd8632d25",
        },
        CatalogFile {
            file_name: "tokenizer.json",
            size: 4_760_186,
            sha256: "2b8bff9d37f1cac4599d1cbef31ea00dc7bc00ecd2a6a33df569d8370ac52c74",
            revision: REV,
        },
    ]
}

fn qwen3_asr_17b_files() -> Vec<CatalogFile> {
    const REV: &str = "4ccc9bcc722a92fc5a7ac7388ea8afe269816728";
    vec![
        CatalogFile {
            file_name: "config.json",
            size: 6_194,
            sha256: "2e74a751548b8ad7d7526d29365ad8144c345d8b412b1152d25dc6698452712f",
            revision: REV,
        },
        CatalogFile {
            file_name: "model-00001-of-00002.safetensors",
            size: 4_220_320_824,
            sha256: "a4cd1f1a04d90b757dc7f7dd26254e69a013b19e80efe590a83c6a3bde8608d6",
            revision: REV,
        },
        CatalogFile {
            file_name: "model-00002-of-00002.safetensors",
            size: 478_200_688,
            sha256: "6e0b9d9e09e2e0238e7ef3cc8a484ab387e91b90f1900bedf88bc92d7929ccfc",
            revision: REV,
        },
        CatalogFile {
            file_name: "model.safetensors.index.json",
            size: 64_821,
            sha256: "f994739fe38e5210b9e3e8ce6c6307315e2ceac3cb630e7b7414d69dce520f60",
            revision: REV,
        },
        CatalogFile {
            file_name: "tokenizer.json",
            size: 4_760_186,
            sha256: "2b8bff9d37f1cac4599d1cbef31ea00dc7bc00ecd2a6a33df569d8370ac52c74",
            revision: REV,
        },
    ]
}

fn qwen_align_files() -> Vec<CatalogFile> {
    const REV: &str = "9500b256a7f795c532e6d28e9a6670c0298b2544";
    vec![
        CatalogFile {
            file_name: "config.json",
            size: 5_982,
            sha256: "d616c65d46c4b90bdc651b0a0963ea932732241140f337f9bb6b0335a9c8ef09",
            revision: REV,
        },
        CatalogFile {
            file_name: "merges.txt",
            size: 1_671_853,
            sha256: "8831e4f1a044471340f7c0a83d7bd71306a5b867e95fd870f74d0c5308a904d5",
            revision: REV,
        },
        CatalogFile {
            file_name: "model.safetensors",
            size: 1_835_544_544,
            sha256: "47831d0e82f96b20e9034dba01a075ee06436654719f6a68289e49f1b65ce0e7",
            revision: REV,
        },
        CatalogFile {
            file_name: "tokenizer_config.json",
            size: 12_666,
            sha256: "3ab80063f8511deb9566e6ad438d17b7a6277fcffd52d92854112f19d36bd81c",
            revision: REV,
        },
        CatalogFile {
            file_name: "vocab.json",
            size: 2_776_833,
            sha256: "ca10d7e9fb3ed18575dd1e277a2579c16d108e32f27439684afa0e10b1440910",
            revision: REV,
        },
    ]
}

/// Same four DLLs as VoxTrans (no NVRTC — engines use precompiled PTX),
/// pinned to the revision the local installs were verified against.
fn cuda_runtime_files() -> Vec<CatalogFile> {
    const REV: &str = "c466fe32e0095f42a8a764f8df7ae8067dd7e590";
    vec![
        CatalogFile {
            file_name: "cudart64_12.dll",
            size: 573_952,
            sha256: "c2c9a9c22a9bcba90e261825968836787b331038047a26770cffb7a583c28344",
            revision: REV,
        },
        CatalogFile {
            file_name: "cublas64_12.dll",
            size: 113_716_224,
            sha256: "9513540e4ec4c51ee9e7304138c2cc255c29a8c181f9e80c38efa25738becd99",
            revision: REV,
        },
        CatalogFile {
            file_name: "cublasLt64_12.dll",
            size: 674_667_520,
            sha256: "b199d1ff892a81b7fd3d57ba1781549609b41500b36008fef326038393ad46c7",
            revision: REV,
        },
        CatalogFile {
            file_name: "curand64_10.dll",
            size: 71_955_968,
            sha256: "3465fd1b46e551339b8f44c455756a0f2cba8bd846562eb659040d48edb7aaac",
            revision: REV,
        },
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
    fn every_catalog_file_is_revision_pinned_and_hashed() {
        for id in [
            ModelId::Qwen3Asr06B,
            ModelId::Qwen3Asr17B,
            ModelId::QwenAlign06B,
            ModelId::CudaRuntime,
        ] {
            let def = model_definition(id);
            assert!(!def.download_files.is_empty(), "{id:?}");
            for file in &def.download_files {
                assert!(
                    file.url.contains("/resolve/") && !file.url.contains("/resolve/master/"),
                    "{id:?} {} is not revision-pinned: {}",
                    file.file_name,
                    file.url
                );
                assert_eq!(
                    file.sha256.len(),
                    64,
                    "{id:?} {} sha256 must be 64 hex chars",
                    file.file_name
                );
                assert!(
                    file.sha256.chars().all(|c| c.is_ascii_hexdigit()),
                    "{id:?} {} sha256 must be hex",
                    file.file_name
                );
                assert!(file.expected_size > 0, "{id:?} {}", file.file_name);
            }
        }
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

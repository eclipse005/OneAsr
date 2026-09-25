//! File lists + ModelScope URLs for the official Qwen `-hf` checkpoints.

use std::path::PathBuf;

use super::path::resolve_model_dir;

pub const QWEN3_ASR_06B: &str = "Qwen3-ASR-0.6B-hf";
pub const QWEN3_ASR_17B: &str = "Qwen3-ASR-1.7B-hf";
/// Int8-quantized checkpoints of the two ASR sizes, published by the same
/// account as the Demucs shard. The wgpu loader auto-detects the quantized
/// weights from the safetensors, so directory layout and load call are
/// identical to the fp16 checkpoints.
pub const QWEN3_ASR_06B_INT8: &str = "Qwen3-ASR-0.6B-int8";
pub const QWEN3_ASR_17B_INT8: &str = "Qwen3-ASR-1.7B-int8";
pub const QWEN_ALIGN_06B: &str = "Qwen3-ForcedAligner-0.6B-hf";
/// HTDemucs v4 vocals weights (optional vocal separation stage).
pub const HTDEMUCS_FT: &str = "htdemucs_ft";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelKind {
    Asr,
    Align,
    /// Optional HTDemucs vocal-separation weights.
    Demucs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModelId {
    Qwen3Asr06B,
    Qwen3Asr17B,
    Qwen3Asr06BInt8,
    Qwen3Asr17BInt8,
    QwenAlign06B,
    /// HTDemucs v4 vocals shard (`htdemucs_ft_vocals.safetensors`).
    HtdemucsFt,
}

impl ModelId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Qwen3Asr06B => QWEN3_ASR_06B,
            Self::Qwen3Asr17B => QWEN3_ASR_17B,
            Self::Qwen3Asr06BInt8 => QWEN3_ASR_06B_INT8,
            Self::Qwen3Asr17BInt8 => QWEN3_ASR_17B_INT8,
            Self::QwenAlign06B => QWEN_ALIGN_06B,
            Self::HtdemucsFt => HTDEMUCS_FT,
        }
    }

    pub fn kind(self) -> ModelKind {
        match self {
            Self::Qwen3Asr06B
            | Self::Qwen3Asr17B
            | Self::Qwen3Asr06BInt8
            | Self::Qwen3Asr17BInt8 => ModelKind::Asr,
            Self::QwenAlign06B => ModelKind::Align,
            Self::HtdemucsFt => ModelKind::Demucs,
        }
    }

    pub fn label(self, lang: crate::i18n::UiLang) -> &'static str {
        match self {
            Self::Qwen3Asr06B => "Qwen3-ASR 0.6B",
            Self::Qwen3Asr17B => "Qwen3-ASR 1.7B",
            Self::Qwen3Asr06BInt8 => "Qwen3-ASR 0.6B int8",
            Self::Qwen3Asr17BInt8 => "Qwen3-ASR 1.7B int8",
            Self::QwenAlign06B => "ForcedAligner 0.6B",
            Self::HtdemucsFt => match lang {
                crate::i18n::UiLang::Zh => "人声分离 HTDemucs",
                crate::i18n::UiLang::En => "Vocal separation (HTDemucs)",
            },
        }
    }

    /// Short chip label for ASR size picker.
    pub fn short_label(self) -> &'static str {
        match self {
            Self::Qwen3Asr06B | Self::Qwen3Asr06BInt8 => "0.6B",
            Self::Qwen3Asr17B | Self::Qwen3Asr17BInt8 => "1.7B",
            Self::QwenAlign06B => "0.6B",
            Self::HtdemucsFt => "Demucs",
        }
    }

    /// The fp16 size this ASR id belongs to; fp16 ids map to themselves.
    pub fn asr_base(self) -> ModelId {
        match self {
            Self::Qwen3Asr06BInt8 => Self::Qwen3Asr06B,
            Self::Qwen3Asr17BInt8 => Self::Qwen3Asr17B,
            other => other,
        }
    }

    /// True when this id names an int8-quantized checkpoint.
    pub fn is_quantized(self) -> bool {
        matches!(self, Self::Qwen3Asr06BInt8 | Self::Qwen3Asr17BInt8)
    }

    /// The same ASR size in fp16 or int8 form (no-op for non-ASR ids).
    pub fn with_quant(self, quantized: bool) -> ModelId {
        if !quantized {
            return self.asr_base();
        }
        match self.asr_base() {
            Self::Qwen3Asr06B => Self::Qwen3Asr06BInt8,
            Self::Qwen3Asr17B => Self::Qwen3Asr17BInt8,
            other => other,
        }
    }

    pub const ASR_CHOICES: [ModelId; 2] = [ModelId::Qwen3Asr06B, ModelId::Qwen3Asr17B];

    /// Parse a catalog name / settings field into an ASR model id.
    ///
    /// Only the install-layout folder names match (`Qwen3-ASR-0.6B-hf` /
    /// `1.7B-hf` and their `-int8` siblings). Unknown strings fall back to 0.6B.
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
        } else if s.eq_ignore_ascii_case(QWEN3_ASR_17B_INT8) {
            Some(Self::Qwen3Asr17BInt8)
        } else if s.eq_ignore_ascii_case(QWEN3_ASR_06B_INT8) {
            Some(Self::Qwen3Asr06BInt8)
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
    /// (`Qwen3-ForcedAligner-0.6B-hf`); custom dir names return `None`.
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
        ModelId::Qwen3Asr06B => model_def(
            id,
            "Qwen",
            QWEN3_ASR_06B,
            resolve_model_dir(QWEN3_ASR_06B),
            qwen3_asr_06b_files(),
        ),
        ModelId::Qwen3Asr17B => model_def(
            id,
            "Qwen",
            QWEN3_ASR_17B,
            resolve_model_dir(QWEN3_ASR_17B),
            qwen3_asr_17b_files(),
        ),
        // The int8 checkpoints are published by the same account as the Demucs
        // shard; loader treats them identically to the fp16 ones.
        ModelId::Qwen3Asr06BInt8 => model_def(
            id,
            "eclipse005",
            QWEN3_ASR_06B_INT8,
            resolve_model_dir(QWEN3_ASR_06B_INT8),
            qwen3_asr_06b_int8_files(),
        ),
        ModelId::Qwen3Asr17BInt8 => model_def(
            id,
            "eclipse005",
            QWEN3_ASR_17B_INT8,
            resolve_model_dir(QWEN3_ASR_17B_INT8),
            qwen3_asr_17b_int8_files(),
        ),
        ModelId::QwenAlign06B => model_def(
            id,
            "Qwen",
            QWEN_ALIGN_06B,
            resolve_model_dir(QWEN_ALIGN_06B),
            qwen_align_files(),
        ),
        // ModelScope repo name differs from the install-layout folder name.
        ModelId::HtdemucsFt => model_def(
            id,
            "eclipse005",
            "htdemucs",
            resolve_model_dir(HTDEMUCS_FT),
            htdemucs_ft_files(),
        ),
    }
}

fn model_def(
    id: ModelId,
    owner: &str,
    repo: &str,
    model_dir: PathBuf,
    files: Vec<CatalogFile>,
) -> ModelDefinition {
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
                    "https://modelscope.cn/models/{owner}/{repo}/resolve/{}/{file}",
                    f.revision,
                    file = f.file_name
                ),
                expected_size: f.size,
                sha256: f.sha256,
            })
            .collect(),
    }
}

/// HTDemucs v4 vocals-only shard (~84 MB). The fine-tuned bag's vocals
/// specialist loaded as a single FourStem network — same output as the
/// full bag's vocals lane, ~4× smaller. Revision-pinned to the ModelScope
/// commit that uploaded `htdemucs_ft_vocals.safetensors`.
fn htdemucs_ft_files() -> Vec<CatalogFile> {
    vec![CatalogFile {
        file_name: crate::engine::local::DEMUCS_WEIGHTS_FILE,
        size: 84_025_440,
        sha256: "68854b0d7c2b3274723b5761f6fd9f5aec5f1bcd3f0de7c1669546fdb7871b7c",
        revision: "d7057b07a0432fede79326e7d56f50c031d9cb50",
    }]
}

fn qwen3_asr_06b_files() -> Vec<CatalogFile> {
    const REV: &str = "c4650525d0d40f32f1517abe624a89be04386bef";
    vec![
        CatalogFile {
            file_name: "config.json",
            size: 2_398,
            sha256: "9eecf6f1b383e343889c2e6010e632590fa57d4bc678e151c7d6a160a0dfb04a",
            revision: REV,
        },
        CatalogFile {
            file_name: "model.safetensors",
            size: 1_564_928_088,
            sha256: "d3f212dd20abecd315d830bc54ae3865e56ebfc3276484e57b771288ba27fd35",
            revision: REV,
        },
        CatalogFile {
            file_name: "tokenizer.json",
            size: 11_429_653,
            sha256: "fe1fad59be22a41ee293363fcf95fdedbc7c93f3b49270b1d2e18bd1399a7a05",
            revision: REV,
        },
    ]
}

fn qwen3_asr_17b_files() -> Vec<CatalogFile> {
    const REV: &str = "d4c6c75bbebe9a9730445a33b6cc105dd9873c90";
    vec![
        CatalogFile {
            file_name: "config.json",
            size: 2_399,
            sha256: "117ac8e63e2af7cae3665e5a632d6eb03f5f384915519ceb6403c15ec6533f63",
            revision: REV,
        },
        CatalogFile {
            file_name: "model.safetensors",
            size: 4_076_193_080,
            sha256: "2db53c7d81bd9b8cbc6a074e89be2c968a0d373fb4ee68bb1b1e14f7042dfee1",
            revision: REV,
        },
        CatalogFile {
            file_name: "tokenizer.json",
            size: 11_429_653,
            sha256: "fe1fad59be22a41ee293363fcf95fdedbc7c93f3b49270b1d2e18bd1399a7a05",
            revision: REV,
        },
    ]
}

/// Int8-quantized 0.6B. The loader needs the same three files as fp16 — the
/// quantized tensors are auto-detected from the safetensors content, so
/// `quant_config.json` (informational) is not part of the ready contract.
fn qwen3_asr_06b_int8_files() -> Vec<CatalogFile> {
    const REV: &str = "54da0afa3013767ac2b2f2ad69845510e5a099e2";
    vec![
        CatalogFile {
            file_name: "config.json",
            size: 2_398,
            sha256: "9eecf6f1b383e343889c2e6010e632590fa57d4bc678e151c7d6a160a0dfb04a",
            revision: REV,
        },
        CatalogFile {
            file_name: "model.safetensors",
            size: 1_125_925_928,
            sha256: "bdcf3ec8bb76d3a6f28943ef65867002f6843e7727ce5f528eb70bf738537e3b",
            revision: REV,
        },
        CatalogFile {
            file_name: "tokenizer.json",
            size: 11_429_653,
            sha256: "fe1fad59be22a41ee293363fcf95fdedbc7c93f3b49270b1d2e18bd1399a7a05",
            revision: REV,
        },
    ]
}

/// Int8-quantized 1.7B — same file contract as [`qwen3_asr_06b_int8_files`].
fn qwen3_asr_17b_int8_files() -> Vec<CatalogFile> {
    const REV: &str = "333ad56f31ba89e158c083a61bfb51d1381c0848";
    vec![
        CatalogFile {
            file_name: "config.json",
            size: 2_399,
            sha256: "117ac8e63e2af7cae3665e5a632d6eb03f5f384915519ceb6403c15ec6533f63",
            revision: REV,
        },
        CatalogFile {
            file_name: "model.safetensors",
            size: 2_669_224_624,
            sha256: "7b015318ff478ba90472fcc831763f8659c5e04ea466d96c92e45232addce010",
            revision: REV,
        },
        CatalogFile {
            file_name: "tokenizer.json",
            size: 11_429_653,
            sha256: "fe1fad59be22a41ee293363fcf95fdedbc7c93f3b49270b1d2e18bd1399a7a05",
            revision: REV,
        },
    ]
}

fn qwen_align_files() -> Vec<CatalogFile> {
    const REV: &str = "053506a54ad1c9deea6105050330d7c500b9f098";
    vec![
        CatalogFile {
            file_name: "config.json",
            size: 248_029,
            sha256: "82ac390b36a8f80c5a0c4e202367debd149e5d8c30abe4cf5eed439101b1c5ce",
            revision: REV,
        },
        CatalogFile {
            file_name: "model.safetensors",
            size: 1_835_545_960,
            sha256: "00568245ceca5af1991d28562a75fe1ddc9bfeb041c27fda66947ea05c47fb86",
            revision: REV,
        },
        CatalogFile {
            file_name: "tokenizer.json",
            size: 11_429_842,
            sha256: "cadfb9cbd9ff8f4e309075ec6a71b063295ff796afb4b5dbc0fd2873fbb301f2",
            revision: REV,
        },
        CatalogFile {
            file_name: "tokenizer_config.json",
            size: 998,
            sha256: "945e980986de2ca7768f3326bfdbb4fbea3406f972b8ae0be233089f2b253c11",
            revision: REV,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asr_and_align_catalogs_are_official_qwen_hf() {
        let asr = model_definition(ModelId::Qwen3Asr06B);
        let align = model_definition(ModelId::QwenAlign06B);
        assert!(asr
            .download_files
            .iter()
            .all(|f| f.url.contains("models/Qwen/Qwen3-ASR-0.6B-hf/")));
        assert!(align
            .download_files
            .iter()
            .all(|f| f.url.contains("models/Qwen/Qwen3-ForcedAligner-0.6B-hf/")));
        let asr17 = model_definition(ModelId::Qwen3Asr17B);
        assert!(asr17
            .download_files
            .iter()
            .all(|f| f.url.contains("models/Qwen/Qwen3-ASR-1.7B-hf/")));
        // The int8 checkpoints ship from the same account as the Demucs shard.
        let asr06_int8 = model_definition(ModelId::Qwen3Asr06BInt8);
        assert!(asr06_int8
            .download_files
            .iter()
            .all(|f| f.url.contains("models/eclipse005/Qwen3-ASR-0.6B-int8/")));
        let asr17_int8 = model_definition(ModelId::Qwen3Asr17BInt8);
        assert!(asr17_int8
            .download_files
            .iter()
            .all(|f| f.url.contains("models/eclipse005/Qwen3-ASR-1.7B-int8/")));
        // int8 weights are smaller than their fp16 counterparts.
        let fp16 = model_definition(ModelId::Qwen3Asr06B);
        let int8_size = asr06_int8
            .download_files
            .iter()
            .find(|f| f.file_name == "model.safetensors")
            .unwrap()
            .expected_size;
        let fp16_size = fp16
            .download_files
            .iter()
            .find(|f| f.file_name == "model.safetensors")
            .unwrap()
            .expected_size;
        assert!(int8_size < fp16_size, "{int8_size} vs {fp16_size}");
    }

    #[test]
    fn every_catalog_file_is_revision_pinned_and_hashed() {
        for id in [
            ModelId::Qwen3Asr06B,
            ModelId::Qwen3Asr17B,
            ModelId::Qwen3Asr06BInt8,
            ModelId::Qwen3Asr17BInt8,
            ModelId::QwenAlign06B,
            ModelId::HtdemucsFt,
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
    fn demucs_catalog_is_vocals_shard() {
        let def = model_definition(ModelId::HtdemucsFt);
        let weights = crate::engine::local::DEMUCS_WEIGHTS_FILE;
        assert_eq!(def.required_files, [weights]);
        let file = &def.download_files[0];
        assert_eq!(file.file_name, weights);
        assert!(file.url.contains("models/eclipse005/htdemucs/"));
        assert!(file.url.contains("/resolve/d7057b07a0432fede79326e7d56f50c031d9cb50/"));
        assert!(
            file.url.ends_with(&format!("/{weights}")),
            "{}",
            file.url
        );
        assert_eq!(file.expected_size, 84_025_440);
        assert_eq!(
            file.sha256,
            "68854b0d7c2b3274723b5761f6fd9f5aec5f1bcd3f0de7c1669546fdb7871b7c"
        );
    }

    #[test]
    fn parse_asr_is_exact_catalog_name_only() {
        assert_eq!(ModelId::parse_asr(QWEN3_ASR_17B), ModelId::Qwen3Asr17B);
        assert_eq!(ModelId::parse_asr(QWEN3_ASR_06B), ModelId::Qwen3Asr06B);
        assert_eq!(
            ModelId::parse_asr(QWEN3_ASR_06B_INT8),
            ModelId::Qwen3Asr06BInt8
        );
        assert_eq!(
            ModelId::parse_asr(QWEN3_ASR_17B_INT8),
            ModelId::Qwen3Asr17BInt8
        );
        // No substring heuristics — "1.7" alone is not a catalog id.
        assert_eq!(ModelId::try_parse_asr("1.7"), None);
        assert_eq!(
            ModelId::parse_asr("something-1.7-else"),
            ModelId::Qwen3Asr06B
        );
        assert_eq!(
            ModelId::try_from_asr_dir(std::path::Path::new(r"C:\m\Qwen3-ASR-1.7B-hf")),
            Some(ModelId::Qwen3Asr17B)
        );
        assert_eq!(
            ModelId::try_from_asr_dir(std::path::Path::new(
                r"C:\m\Qwen3-ASR-0.6B-int8"
            )),
            Some(ModelId::Qwen3Asr06BInt8)
        );
        assert_eq!(
            ModelId::try_from_aligner_dir(std::path::Path::new(
                r"C:\m\Qwen3-ForcedAligner-0.6B-hf"
            )),
            Some(ModelId::QwenAlign06B)
        );
    }

    #[test]
    fn quant_helpers_map_between_fp16_and_int8() {
        assert!(!ModelId::Qwen3Asr06B.is_quantized());
        assert!(ModelId::Qwen3Asr06BInt8.is_quantized());
        assert_eq!(ModelId::Qwen3Asr06BInt8.asr_base(), ModelId::Qwen3Asr06B);
        assert_eq!(ModelId::Qwen3Asr17BInt8.asr_base(), ModelId::Qwen3Asr17B);
        // The size is preserved across the fp16/int8 round trip.
        assert_eq!(
            ModelId::Qwen3Asr17B.with_quant(true),
            ModelId::Qwen3Asr17BInt8
        );
        assert_eq!(
            ModelId::Qwen3Asr17BInt8.with_quant(false),
            ModelId::Qwen3Asr17B
        );
        assert_eq!(ModelId::Qwen3Asr06B.with_quant(true), ModelId::Qwen3Asr06BInt8);
        // Non-ASR ids are unaffected.
        assert_eq!(ModelId::QwenAlign06B.with_quant(true), ModelId::QwenAlign06B);
        assert_eq!(ModelId::QwenAlign06B.asr_base(), ModelId::QwenAlign06B);
        // The four ASR ids carry their own size label.
        assert_eq!(ModelId::Qwen3Asr17BInt8.short_label(), "1.7B");
    }
}

//! File lists + ModelScope URLs for the official Qwen `-hf` checkpoints.

pub const QWEN3_ASR_06B: &str = "Qwen3-ASR-0.6B-hf";
pub const QWEN3_ASR_17B: &str = "Qwen3-ASR-1.7B-hf";
/// Int8-quantized checkpoints of the two ASR sizes, published by the same
/// account as the Demucs shard. The wgpu loader auto-detects the quantized
/// weights from the safetensors, so directory layout and load call are
/// identical to the fp16 checkpoints.
pub const QWEN3_ASR_06B_INT8: &str = "Qwen3-ASR-0.6B-int8";
pub const QWEN3_ASR_17B_INT8: &str = "Qwen3-ASR-1.7B-int8";
/// Qwen forced aligner (Ultra fine-tune of the official 0.6B checkpoint).
/// The install-layout folder and the ModelScope repo share this name.
pub const QWEN_ALIGN_ULTRA: &str = "Qwen3-ForcedAligner-Ultra";
/// Pre-Ultra install-layout name. Kept only for `settings.json` and on-disk
/// migration: old configs still carry this string, and an old custom pick may
/// sit in a folder with this name. New downloads always use
/// [`QWEN_ALIGN_ULTRA`].
pub const QWEN_ALIGN_LEGACY: &str = "Qwen3-ForcedAligner-0.6B-hf";
/// CTC forced-alignment checkpoint (Meta Omnilingual family, Apache-2.0
/// upstream). The ModelScope repo name omits the `-hf` install-layout suffix.
pub const OMNI_ASR_CTC_300M: &str = "omniASR-CTC-300M-v2-hf";
/// HTDemucs v4 vocals weights (optional vocal separation stage).
pub const HTDEMUCS_FT: &str = "htdemucs_ft";
/// 热词粗转用的 SenseVoiceSmall（可选阶段：只服务热词召回，不参与正式转写）。
pub const SENSEVOICE_SMALL: &str = "SenseVoiceSmall";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelKind {
    Asr,
    Align,
    /// Optional HTDemucs vocal-separation weights.
    Demucs,
    /// 热词粗转（SenseVoiceSmall，可选阶段）。
    Hotword,
}

impl ModelKind {
    /// 机器可读的稳定标识（`status --json` 的 catalog `kind` 字段）。
    ///
    /// 与枚举名分开写死：接口上的值不跟着 Rust 标识符改名漂。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Asr => "asr",
            Self::Align => "aligner",
            Self::Demucs => "demucs",
            Self::Hotword => "sensevoice",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModelId {
    Qwen3Asr06B,
    Qwen3Asr17B,
    Qwen3Asr06BInt8,
    Qwen3Asr17BInt8,
    QwenAlignUltra,
    OmniAsrCtc300M,
    /// HTDemucs v4 vocals shard (`htdemucs_ft_vocals.safetensors`).
    HtdemucsFt,
    /// SenseVoiceSmall —— 热词的第一趟粗转（可选）。
    SenseVoiceSmall,
}

impl ModelId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Qwen3Asr06B => QWEN3_ASR_06B,
            Self::Qwen3Asr17B => QWEN3_ASR_17B,
            Self::Qwen3Asr06BInt8 => QWEN3_ASR_06B_INT8,
            Self::Qwen3Asr17BInt8 => QWEN3_ASR_17B_INT8,
            Self::QwenAlignUltra => QWEN_ALIGN_ULTRA,
            Self::OmniAsrCtc300M => OMNI_ASR_CTC_300M,
            Self::HtdemucsFt => HTDEMUCS_FT,
            Self::SenseVoiceSmall => SENSEVOICE_SMALL,
        }
    }

    pub fn kind(self) -> ModelKind {
        match self {
            Self::Qwen3Asr06B
            | Self::Qwen3Asr17B
            | Self::Qwen3Asr06BInt8
            | Self::Qwen3Asr17BInt8 => ModelKind::Asr,
            Self::QwenAlignUltra | Self::OmniAsrCtc300M => ModelKind::Align,
            Self::HtdemucsFt => ModelKind::Demucs,
            Self::SenseVoiceSmall => ModelKind::Hotword,
        }
    }

    pub fn label(self, lang: crate::i18n::UiLang) -> &'static str {
        match self {
            Self::Qwen3Asr06B => "Qwen3-ASR 0.6B",
            Self::Qwen3Asr17B => "Qwen3-ASR 1.7B",
            Self::Qwen3Asr06BInt8 => "Qwen3-ASR 0.6B int8",
            Self::Qwen3Asr17BInt8 => "Qwen3-ASR 1.7B int8",
            Self::QwenAlignUltra => "ForcedAligner Ultra",
            Self::OmniAsrCtc300M => "omniASR-CTC 300M",
            Self::HtdemucsFt => match lang {
                crate::i18n::UiLang::Zh => "人声分离 HTDemucs",
                crate::i18n::UiLang::En => "Vocal separation (HTDemucs)",
            },
            Self::SenseVoiceSmall => match lang {
                crate::i18n::UiLang::Zh => "热词模型 SenseVoice",
                crate::i18n::UiLang::En => "Hotword model (SenseVoice)",
            },
        }
    }

    /// Short chip label for the model pickers. The aligner picker shows the
    /// engine family (`CTC` / `Qwen`), not a size — the two aligners differ
    /// in behavior, not in "size".
    pub fn short_label(self) -> &'static str {
        match self {
            Self::Qwen3Asr06B | Self::Qwen3Asr06BInt8 => "0.6B",
            Self::Qwen3Asr17B | Self::Qwen3Asr17BInt8 => "1.7B",
            Self::QwenAlignUltra => "Qwen",
            Self::OmniAsrCtc300M => "CTC",
            Self::HtdemucsFt => "Demucs",
            Self::SenseVoiceSmall => "SenseVoice",
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

    /// 全部 8 个 catalog 条目 —— **唯一**的完整清单。
    ///
    /// 顺序 = `status --json` 的 catalog 顺序（同尺寸的 fp16 / int8 相邻，再对齐器、
    /// 人声分离、热词）。新增模型必须加在这里：清单只写一份，界面清单、status 与
    /// 下载解析的完整性测试都从它枚举。
    pub const ALL: [ModelId; 8] = [
        Self::Qwen3Asr06B,
        Self::Qwen3Asr06BInt8,
        Self::Qwen3Asr17B,
        Self::Qwen3Asr17BInt8,
        Self::QwenAlignUltra,
        Self::OmniAsrCtc300M,
        Self::HtdemucsFt,
        Self::SenseVoiceSmall,
    ];

    pub const ASR_CHOICES: [ModelId; 2] = [ModelId::Qwen3Asr06B, ModelId::Qwen3Asr17B];

    /// Aligner picker order: CTC first (the order users already know), Qwen
    /// second. **Picker order is not the default** — [`Self::default_aligner`]
    /// is Qwen; the CTC chip stays first so the Settings drawer does not move.
    pub const ALIGNER_CHOICES: [ModelId; 2] = [ModelId::OmniAsrCtc300M, ModelId::QwenAlignUltra];

    /// Default aligner for a fresh install / an unknown settings value.
    /// Qwen (Ultra) is the default; CTC stays available in Settings and is
    /// still what transcript matching uses.
    pub fn default_aligner() -> Self {
        Self::QwenAlignUltra
    }

    /// Parse a catalog name / settings field into an aligner model id.
    /// Unknown strings fall back to the default (Qwen).
    pub fn parse_aligner(raw: &str) -> Self {
        Self::try_parse_aligner(raw).unwrap_or(Self::default_aligner())
    }

    /// Exact catalog-name match only (no substring heuristics).
    pub fn try_parse_aligner(raw: &str) -> Option<Self> {
        let s = raw.trim();
        if s.eq_ignore_ascii_case(OMNI_ASR_CTC_300M) {
            Some(Self::OmniAsrCtc300M)
        } else if s.eq_ignore_ascii_case(QWEN_ALIGN_ULTRA)
            || s.eq_ignore_ascii_case(QWEN_ALIGN_LEGACY)
        {
            Some(Self::QwenAlignUltra)
        } else {
            None
        }
    }

    /// True for the pre-Ultra install-layout name. Only configuration and
    /// directory migration ask this; new selections always use the Ultra name.
    pub fn is_legacy_aligner_name(raw: &str) -> bool {
        raw.trim().eq_ignore_ascii_case(QWEN_ALIGN_LEGACY)
    }

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
    /// (`Qwen3-ForcedAligner-Ultra` / `omniASR-CTC-300M-v2-hf`). The pre-Ultra
    /// folder name is accepted as a migration alias; custom dir names return
    /// `None`.
    pub fn try_from_aligner_dir(path: &std::path::Path) -> Option<Self> {
        let name = path.file_name().and_then(|n| n.to_str())?;
        if name.eq_ignore_ascii_case(OMNI_ASR_CTC_300M) {
            Some(Self::OmniAsrCtc300M)
        } else if name.eq_ignore_ascii_case(QWEN_ALIGN_ULTRA)
            || name.eq_ignore_ascii_case(QWEN_ALIGN_LEGACY)
        {
            Some(Self::QwenAlignUltra)
        } else {
            None
        }
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

/// 一个模型"要下什么、要查什么"——**不含落点**。
///
/// 落点由调用方给出（[`super::DownloadHandle::new`] 的 `model_dir`，也就是设置
/// 里为这个组件配的那个目录）；目录策略只在 `crate::paths` / `resolve_model_dir`
/// 判定一次。这里不留第二份 `model_dir`，否则"定义自带的目录"和"调用方要的目录"
/// 迟早会分叉——那正是下载落到别处却报"完成"的根因。
#[derive(Debug, Clone)]
pub struct ModelDefinition {
    pub id: ModelId,
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
        ModelId::Qwen3Asr06B => model_def(id, "Qwen", QWEN3_ASR_06B, qwen3_asr_06b_files()),
        ModelId::Qwen3Asr17B => model_def(id, "Qwen", QWEN3_ASR_17B, qwen3_asr_17b_files()),
        // The int8 checkpoints are published by the same account as the Demucs
        // shard; loader treats them identically to the fp16 ones.
        ModelId::Qwen3Asr06BInt8 => model_def(
            id,
            "eclipse005",
            QWEN3_ASR_06B_INT8,
            qwen3_asr_06b_int8_files(),
        ),
        ModelId::Qwen3Asr17BInt8 => model_def(
            id,
            "eclipse005",
            QWEN3_ASR_17B_INT8,
            qwen3_asr_17b_int8_files(),
        ),
        ModelId::QwenAlignUltra => model_def(
            id,
            "eclipse005",
            "Qwen3-ForcedAligner-Ultra",
            qwen_align_files(),
        ),
        // ModelScope repo name differs from the install-layout folder name.
        ModelId::HtdemucsFt => model_def(id, "eclipse005", "htdemucs", htdemucs_ft_files()),
        ModelId::OmniAsrCtc300M => model_def(
            id,
            "eclipse005",
            // ModelScope repo name (no `-hf` suffix).
            "omniASR-CTC-300M-v2",
            omni_asr_ctc_300m_files(),
        ),
        // 官方仓库（iic/SenseVoiceSmall），不自己转存。
        ModelId::SenseVoiceSmall => {
            model_def(id, "iic", "SenseVoiceSmall", sensevoice_small_files())
        }
    }
}

fn model_def(id: ModelId, owner: &str, repo: &str, files: Vec<CatalogFile>) -> ModelDefinition {
    ModelDefinition {
        id,
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

/// SenseVoiceSmall（热词粗转）。下面这四个文件合计 937,031,977 字节（≈ 937 MB /
/// 0.94 GB），就是用户实际要下的量。官方 `iic/SenseVoiceSmall` 仓库，
/// **只取加载器真正会打开的那 4 个文件**：仓库里还有 `model.safetensors`（同一份
/// 权重的另一种格式）、示例音频与配图，那些不下载。
///
/// 逐文件钉 revision：`tokens.json` 与其余三个不在同一次提交上（后者更晚）。
fn sensevoice_small_files() -> Vec<CatalogFile> {
    const WEIGHTS_REV: &str = "70514a3da51f1160f51d18449dab6128bbd4928b";
    const TOKENS_REV: &str = "43d0ed61231c41f8393fa347b838a1f6e2d264f6";
    vec![
        CatalogFile {
            file_name: "model.pt",
            size: 936_291_369,
            sha256: "833ca2dcfdf8ec91bd4f31cfac36d6124e0c459074d5e909aec9cabe6204a3ea",
            revision: WEIGHTS_REV,
        },
        CatalogFile {
            file_name: "chn_jpn_yue_eng_ko_spectok.bpe.model",
            size: 377_341,
            sha256: "aa87f86064c3730d799ddf7af3c04659151102cba548bce325cf06ba4da4e6a8",
            revision: WEIGHTS_REV,
        },
        CatalogFile {
            file_name: "am.mvn",
            size: 11_203,
            sha256: "29b3c740a2c0cfc6b308126d31d7f265fa2be74f3bb095cd2f143ea970896ae5",
            revision: WEIGHTS_REV,
        },
        CatalogFile {
            file_name: "tokens.json",
            size: 352_064,
            sha256: "a2594fc1474e78973149cba8cd1f603ebed8c39c7decb470631f66e70ce58e97",
            revision: TOKENS_REV,
        },
    ]
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

/// omniASR-CTC-300M-v2 (Meta Omnilingual family, Apache-2.0 upstream) — the
/// CTC forced aligner. Sizes + hashes verified against the ModelScope file
/// listing and the local copy of the weights; revision is the upload commit.
fn omni_asr_ctc_300m_files() -> Vec<CatalogFile> {
    const REV: &str = "ecd67cd0a814c13b3a0e9314c4a12037f60bc052";
    vec![
        CatalogFile {
            file_name: "config.json",
            size: 1_968,
            sha256: "a78b50a52e4e067bd5e502c33447dc556db1dd08f5286bb5ece49b61d522b194",
            revision: REV,
        },
        CatalogFile {
            file_name: "preprocessor_config.json",
            size: 187,
            sha256: "06cd0728688a41b0d4ff87a7eb08479b2fdae9a9521fb1c328bc8c4230a4f6f7",
            revision: REV,
        },
        CatalogFile {
            file_name: "special_tokens_map.json",
            size: 93,
            sha256: "d9a757b89920ad7c0cd8329f2320709943c21d96c1ab7bce56c258930f6b07b7",
            revision: REV,
        },
        CatalogFile {
            file_name: "tokenizer_config.json",
            size: 233,
            sha256: "413ddc3802665f3b11a96161b565e38644750fdb592c325564ddd75d6cf6b8bf",
            revision: REV,
        },
        CatalogFile {
            file_name: "vocab.json",
            size: 152_362,
            sha256: "4568040d826f2584ca2b76ed10c679dd051e7e1df9742acb5285370cd16a331f",
            revision: REV,
        },
        CatalogFile {
            file_name: "model.safetensors",
            size: 1_303_988_352,
            sha256: "606d849a55b746e698976b8a979b597f4902be7a799ecf723dd283d6210e5871",
            revision: REV,
        },
    ]
}

/// Qwen3-ForcedAligner-Ultra (eclipse005's fine-tune of the official 0.6B
/// aligner) plus its optional Local Head package.
///
/// The install-layout folder and the hub repo share the Ultra name. Seven
/// files: the four the main checkpoint's loader opens, and the three
/// `local_head/` files the crate auto-loads when the directory is present.
/// Sizes + hashes are the ModelScope file listing at the upload commit.
fn qwen_align_files() -> Vec<CatalogFile> {
    const REV: &str = "2088fd62a25efd6d0949ed1ffe08356596742f18";
    vec![
        CatalogFile {
            file_name: "config.json",
            size: 5_632,
            sha256: "a2521a05badbd1ab064dbc1645579b99e3ed5e31a317650753ded6a0bd4d1298",
            revision: REV,
        },
        CatalogFile {
            file_name: "model.safetensors",
            size: 1_835_544_544,
            sha256: "e6fcf5be2c11b265f7217ba6102917a7fe64438506357be6999f6ad5a707cb5f",
            revision: REV,
        },
        CatalogFile {
            file_name: "tokenizer.json",
            size: 11_429_688,
            sha256: "90934377d8ceace196d3c8f872df96c8b48c6206f019b65e4ac631c04ee18f59",
            revision: REV,
        },
        CatalogFile {
            file_name: "tokenizer_config.json",
            size: 12_696,
            sha256: "bee4786e25db33d3e8acb4c7a37c4ccf8d5a3d843b9cafb75a91d4ba774ad38f",
            revision: REV,
        },
        CatalogFile {
            file_name: "local_head/config.json",
            size: 1_150,
            sha256: "d3bb91b1c29742f4d5e71507d61e52754d8ee237dc3b2e92500e61c4aa0e3f8e",
            revision: REV,
        },
        CatalogFile {
            file_name: "local_head/model.safetensors",
            size: 1_623_376,
            sha256: "50a70d6ed969446770fb9dc1063f5f4e6e4c86eac05fd3efb637492407d0e2ac",
            revision: REV,
        },
        CatalogFile {
            file_name: "local_head/normalizer.json",
            size: 4_370,
            sha256: "24f6d4fa08dd78e12f7d15a75b0d8b0cd3172bbdfdaa7ae055ca2c64db660ae7",
            revision: REV,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asr_and_align_catalogs_pin_their_repos() {
        let asr = model_definition(ModelId::Qwen3Asr06B);
        let align = model_definition(ModelId::QwenAlignUltra);
        assert!(
            asr.download_files
                .iter()
                .all(|f| f.url.contains("models/Qwen/Qwen3-ASR-0.6B-hf/"))
        );
        assert!(align.download_files.iter().all(|f| {
            f.url
                .contains("models/eclipse005/Qwen3-ForcedAligner-Ultra/")
        }));
        let asr17 = model_definition(ModelId::Qwen3Asr17B);
        assert!(
            asr17
                .download_files
                .iter()
                .all(|f| f.url.contains("models/Qwen/Qwen3-ASR-1.7B-hf/"))
        );
        // The int8 checkpoints ship from the same account as the Demucs shard.
        let asr06_int8 = model_definition(ModelId::Qwen3Asr06BInt8);
        assert!(
            asr06_int8
                .download_files
                .iter()
                .all(|f| f.url.contains("models/eclipse005/Qwen3-ASR-0.6B-int8/"))
        );
        let asr17_int8 = model_definition(ModelId::Qwen3Asr17BInt8);
        assert!(
            asr17_int8
                .download_files
                .iter()
                .all(|f| f.url.contains("models/eclipse005/Qwen3-ASR-1.7B-int8/"))
        );
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
        // 遍历唯一清单：新增模型漏了哈希 / revision，这条测试直接变红。
        for id in ModelId::ALL {
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
    fn ctc_aligner_catalog_is_the_local_upload() {
        let def = model_definition(ModelId::OmniAsrCtc300M);
        assert_eq!(
            def.required_files,
            [
                "config.json",
                "preprocessor_config.json",
                "special_tokens_map.json",
                "tokenizer_config.json",
                "vocab.json",
                "model.safetensors",
            ]
        );
        // The ModelScope repo name omits the `-hf` install-layout suffix.
        assert!(
            def.download_files
                .iter()
                .all(|f| f.url.contains("models/eclipse005/omniASR-CTC-300M-v2/")),
            "{:?}",
            def.download_files[0].url
        );
        let weights = def
            .download_files
            .iter()
            .find(|f| f.file_name == "model.safetensors")
            .unwrap();
        assert_eq!(weights.expected_size, 1_303_988_352);
        assert_eq!(
            weights.sha256,
            "606d849a55b746e698976b8a979b597f4902be7a799ecf723dd283d6210e5871"
        );
    }

    #[test]
    fn parse_aligner_is_exact_catalog_name_only() {
        // Picker order stays CTC-then-Qwen; the **default selection** is Qwen.
        assert_eq!(ModelId::ALIGNER_CHOICES[0], ModelId::OmniAsrCtc300M);
        assert_eq!(ModelId::default_aligner(), ModelId::QwenAlignUltra);
        assert_eq!(
            ModelId::parse_aligner(OMNI_ASR_CTC_300M),
            ModelId::OmniAsrCtc300M
        );
        assert_eq!(
            ModelId::parse_aligner(QWEN_ALIGN_ULTRA),
            ModelId::QwenAlignUltra
        );
        // 旧版写进 settings.json / 目录名的名字仍然认，并归到 Ultra。
        assert_eq!(
            ModelId::parse_aligner(QWEN_ALIGN_LEGACY),
            ModelId::QwenAlignUltra
        );
        assert!(ModelId::is_legacy_aligner_name(QWEN_ALIGN_LEGACY));
        assert!(!ModelId::is_legacy_aligner_name(QWEN_ALIGN_ULTRA));
        assert_eq!(ModelId::try_parse_aligner("ctc"), None);
        assert_eq!(
            ModelId::parse_aligner("something-else"),
            ModelId::QwenAlignUltra
        );
        assert_eq!(
            ModelId::try_from_aligner_dir(
                &std::path::Path::new("m").join("omniASR-CTC-300M-v2-hf")
            ),
            Some(ModelId::OmniAsrCtc300M)
        );
        assert_eq!(
            ModelId::try_from_aligner_dir(&std::path::Path::new("m").join(QWEN_ALIGN_ULTRA)),
            Some(ModelId::QwenAlignUltra)
        );
        assert_eq!(
            ModelId::try_from_aligner_dir(&std::path::Path::new("m").join(QWEN_ALIGN_LEGACY)),
            Some(ModelId::QwenAlignUltra)
        );
        assert_eq!(
            ModelId::try_from_aligner_dir(&std::path::Path::new("m").join("custom")),
            None
        );
    }

    /// Ultra 的清单是「主检查点 4 个文件 + Local Head 3 个文件」，七份都在
    /// 同一个上传提交上；少一份 `local_head/`，对齐器会退回 80 ms 网格。
    #[test]
    fn qwen_align_catalog_is_the_ultra_checkpoint() {
        let def = model_definition(ModelId::QwenAlignUltra);
        assert_eq!(
            def.required_files,
            [
                "config.json",
                "model.safetensors",
                "tokenizer.json",
                "tokenizer_config.json",
                "local_head/config.json",
                "local_head/model.safetensors",
                "local_head/normalizer.json",
            ]
        );
        // 安装目录与 hub 仓库同名，URL 必须指向 Ultra 仓库，而不是旧名。
        assert!(def.download_files.iter().all(|f| {
            f.url
                .contains("models/eclipse005/Qwen3-ForcedAligner-Ultra/")
        }));
        assert!(def.download_files.iter().all(|f| {
            f.url
                .contains("/resolve/2088fd62a25efd6d0949ed1ffe08356596742f18/")
        }));
        let weights = def
            .download_files
            .iter()
            .find(|f| f.file_name == "model.safetensors")
            .unwrap();
        assert_eq!(weights.expected_size, 1_835_544_544);
        assert_eq!(
            weights.sha256,
            "e6fcf5be2c11b265f7217ba6102917a7fe64438506357be6999f6ad5a707cb5f"
        );
        let head = def
            .download_files
            .iter()
            .find(|f| f.file_name == "local_head/model.safetensors")
            .unwrap();
        assert_eq!(head.expected_size, 1_623_376);
        assert_eq!(
            head.sha256,
            "50a70d6ed969446770fb9dc1063f5f4e6e4c86eac05fd3efb637492407d0e2ac"
        );
    }

    #[test]
    fn demucs_catalog_is_vocals_shard() {
        let def = model_definition(ModelId::HtdemucsFt);
        let weights = crate::engine::local::DEMUCS_WEIGHTS_FILE;
        assert_eq!(def.required_files, [weights]);
        let file = &def.download_files[0];
        assert_eq!(file.file_name, weights);
        assert!(file.url.contains("models/eclipse005/htdemucs/"));
        assert!(
            file.url
                .contains("/resolve/d7057b07a0432fede79326e7d56f50c031d9cb50/")
        );
        assert!(file.url.ends_with(&format!("/{weights}")), "{}", file.url);
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
            ModelId::try_from_asr_dir(&std::path::Path::new("m").join("Qwen3-ASR-1.7B-hf")),
            Some(ModelId::Qwen3Asr17B)
        );
        assert_eq!(
            ModelId::try_from_asr_dir(&std::path::Path::new("m").join("Qwen3-ASR-0.6B-int8")),
            Some(ModelId::Qwen3Asr06BInt8)
        );
        assert_eq!(
            ModelId::try_from_aligner_dir(&std::path::Path::new("m").join(QWEN_ALIGN_ULTRA)),
            Some(ModelId::QwenAlignUltra)
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
        assert_eq!(
            ModelId::Qwen3Asr06B.with_quant(true),
            ModelId::Qwen3Asr06BInt8
        );
        // Non-ASR ids are unaffected.
        assert_eq!(
            ModelId::QwenAlignUltra.with_quant(true),
            ModelId::QwenAlignUltra
        );
        assert_eq!(ModelId::QwenAlignUltra.asr_base(), ModelId::QwenAlignUltra);
        // The four ASR ids carry their own size label.
        assert_eq!(ModelId::Qwen3Asr17BInt8.short_label(), "1.7B");
    }
}

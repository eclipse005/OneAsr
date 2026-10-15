//! SenseVoiceSmall（wgpu）粗转录适配器：给热词召回提供一趟便宜的第一遍文本。
//!
//! 与 [`super::qwen_asr`] 的差别在**调用形状**：粗转一次吃下全部 VAD 分块
//! （GPU 上多段会拼成一次 encoder 前向），而正式 ASR 是逐块调用。两趟必须共用
//! 同一批分块，否则召回出来的热词会跨块错位——分块由 `crate::vad::plan_chunks`
//! 给出，这里只负责按毫秒把它交给 SenseVoice。

use std::path::Path;

use sensevoice_small_wgpu::{Model, Options as SenseOptions, VadSegment};

use super::backend::ComputeBackend;
use crate::engine::{EngineError, RoughAsr};

/// 16 kHz mono 下 1 ms 的采样数。`chunks` 给的是秒，SenseVoice 要毫秒。
const SAMPLES_PER_MS: f64 = 16.0;

/// `Model::load` 真正会打开的文件（权重 / 分词 / 前端 CMVN 三件套）。
///
/// 模型目录的「就绪」判据就是这四个文件在不在——见 `crate::asr::model_check`。
pub const REQUIRED_FILES: [&str; 4] = [
    "model.pt",
    "chn_jpn_yue_eng_ko_spectok.bpe.model",
    "tokens.json",
    "am.mvn",
];

pub(super) struct SenseVoiceRoughAdapter {
    inner: Model,
}

impl SenseVoiceRoughAdapter {
    pub(super) fn load(model_dir: &Path, backend: ComputeBackend) -> Result<Self, EngineError> {
        // 与正式 ASR 共用一条后端策略：本机若已定成 CPU（比如正式 ASR 的 GPU
        // 加载回落过），这趟也走 CPU；否则用 ONEASR_DEVICE 的钉卡 / 默认 auto。
        let device = match backend {
            ComputeBackend::Cpu => "cpu".to_string(),
            _ => super::device::sensevoice_device_from_env(),
        };
        Model::load(model_dir, &device)
            .map(|inner| Self { inner })
            .map_err(|e| EngineError::new(format!("{e:#}")))
    }

    pub(super) fn device_description(&self) -> String {
        self.inner.device().to_string()
    }
}

impl RoughAsr for SenseVoiceRoughAdapter {
    fn transcribe_chunks(
        &mut self,
        wav: &Path,
        chunks: &[(f32, f32)],
    ) -> Result<Vec<String>, EngineError> {
        if chunks.is_empty() {
            return Ok(Vec::new());
        }
        let samples = sensevoice_small_wgpu::load_audio(wav)
            .map_err(|e| EngineError::new(format!("{e:#}")))?;
        let segments: Vec<VadSegment> = chunks
            .iter()
            .map(|(start, end)| VadSegment {
                start_ms: f64::from(*start) * 1000.0,
                end_ms: f64::from(*end) * 1000.0,
            })
            .collect();
        // 引擎内部的注意力缓冲按最长段预留（与上游 example 同款做法）。
        let longest = segments
            .iter()
            .map(|seg| ((seg.end_ms - seg.start_ms) * SAMPLES_PER_MS) as usize)
            .max()
            .unwrap_or(0);
        self.inner
            .reserve_samples(longest)
            .map_err(|e| EngineError::new(format!("{e:#}")))?;
        // `merge_ms = 0`：分块已经是我们自己按静音切好的，让 `segments` 与入参
        // 严格 1:1，才能把每段文本对回它自己的时间轴。语言交给模型自己判
        // （`auto`）——粗转只要「捞得出词」，不需要准。
        let report = self
            .inner
            .transcribe(
                &samples,
                &SenseOptions {
                    language: "auto".to_string(),
                    use_itn: true,
                    ban_emo_unk: false,
                    segments: segments.clone(),
                    merge_ms: 0.0,
                },
            )
            .map_err(|e| EngineError::new(format!("{e:#}")))?;

        let mut texts = vec![String::new(); segments.len()];
        for out in &report.segments {
            // 过短（不足一个 fbank 窗）的段会被引擎静默跳过，所以按 start_ms
            // 反查而不是按下标对齐。
            if let Some(i) = segments
                .iter()
                .position(|seg| (seg.start_ms - out.start_ms).abs() < 1e-6)
                && let Some(slot) = texts.get_mut(i)
            {
                *slot = strip_tags(&out.raw);
            }
        }
        Ok(texts)
    }
}

/// 剥掉 SenseVoice 的前缀标签（语言 / 情绪 / 事件 / ITN 四个 `<|…|>`）。
///
/// `raw` 形如 `<|zh|><|NEUTRAL|><|Speech|><|woitn|>正文`；热词召回只看字面文本，
/// 标签是纯噪声。偏移量相对**剥掉前缀之后**的串，所以从 `stripped` 上切。
fn strip_tags(raw: &str) -> String {
    let mut rest = raw;
    for _ in 0..4 {
        let Some(stripped) = rest.strip_prefix("<|") else {
            break;
        };
        let Some(end) = stripped.find("|>") else {
            break;
        };
        rest = &stripped[end + 2..];
    }
    rest.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::strip_tags;

    #[test]
    fn strips_the_four_sensevoice_tags() {
        assert_eq!(
            strip_tags("<|zh|><|NEUTRAL|><|Speech|><|woitn|>今天讲窦神归来"),
            "今天讲窦神归来"
        );
    }

    #[test]
    fn tolerates_fewer_tags_and_plain_text() {
        assert_eq!(strip_tags("<|zh|>你好"), "你好");
        assert_eq!(strip_tags("没有标签"), "没有标签");
        assert_eq!(strip_tags(""), "");
    }
}

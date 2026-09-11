//! In-memory engine doubles.
//!
//! Pipeline behaviour — stage order, engine lifetime (the "one model resident
//! at a time" rule), export formats, script conversion, warnings — can be
//! exercised end to end with these, without weights, a GPU or ffmpeg. See
//! `tests/pipeline_behaviour.rs`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::engine::{
    AlignRequest, AlignedToken, Aligner, AsrEngine, EngineError, EngineProvider, SeparateRequest,
    SeparationEvent, Separator, TranscribeRequest, Transcript,
};

/// Shared, ordered record of engine lifecycle events (`asr.load`, `asr.drop`, …).
#[derive(Clone, Default)]
pub struct EngineLog(Arc<Mutex<Vec<String>>>);

impl EngineLog {
    pub fn push(&self, event: impl Into<String>) {
        self.0.lock().unwrap().push(event.into());
    }

    pub fn entries(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }

    /// Index of the first entry equal to `event`, if any.
    pub fn position(&self, event: &str) -> Option<usize> {
        self.entries().iter().position(|e| e == event)
    }
}

/// Behaviour of [`FakeProvider`]'s separator.
#[derive(Clone, Debug)]
pub struct FakeSeparation {
    /// Emit `Progress { done: 1..=total, total }` while separating.
    pub progress_total: usize,
    /// Report a GPU→CPU fallback (what the real adapter does when the GPU
    /// cannot load under 「自动」).
    pub fall_back_to_cpu: bool,
    /// Duration of the vocals WAV written into the scratch dir.
    pub vocals_seconds: f32,
}

impl Default for FakeSeparation {
    fn default() -> Self {
        Self {
            progress_total: 0,
            fall_back_to_cpu: false,
            vocals_seconds: 4.0,
        }
    }
}

/// Engine provider returning recording fakes.
pub struct FakeProvider {
    log: EngineLog,
    transcript: String,
    tokens: Vec<AlignedToken>,
    separation: Option<FakeSeparation>,
    align_inputs: Arc<Mutex<Vec<String>>>,
    separator_loads: Arc<AtomicUsize>,
}

impl FakeProvider {
    /// Fake ASR always returns `transcript`; fake aligner always returns `tokens`.
    pub fn new(transcript: impl Into<String>, tokens: Vec<AlignedToken>) -> Self {
        Self {
            log: EngineLog::default(),
            transcript: transcript.into(),
            tokens,
            separation: None,
            align_inputs: Arc::new(Mutex::new(Vec::new())),
            separator_loads: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Enable the fake separator (only called by runs with separation enabled).
    pub fn with_separation(mut self, separation: FakeSeparation) -> Self {
        self.separation = Some(separation);
        self
    }

    pub fn log(&self) -> &EngineLog {
        &self.log
    }

    /// Texts handed to the aligner, in call order — lets a test prove that
    /// alignment saw the *raw* transcript even when the subtitle is converted.
    pub fn align_inputs(&self) -> Vec<String> {
        self.align_inputs.lock().unwrap().clone()
    }

    /// How often the pipeline asked for a separator engine.
    pub fn separator_loads(&self) -> usize {
        self.separator_loads.load(Ordering::SeqCst)
    }
}

impl EngineProvider for FakeProvider {
    fn load_asr(&self) -> Result<Box<dyn AsrEngine>, EngineError> {
        self.log.push("asr.load");
        Ok(Box::new(FakeAsr {
            log: self.log.clone(),
            text: self.transcript.clone(),
        }))
    }

    fn load_aligner(&self) -> Result<Box<dyn Aligner>, EngineError> {
        self.log.push("aligner.load");
        Ok(Box::new(FakeAligner {
            log: self.log.clone(),
            tokens: self.tokens.clone(),
            inputs: self.align_inputs.clone(),
        }))
    }

    fn load_separator(&self) -> Result<Box<dyn Separator>, EngineError> {
        let Some(config) = self.separation.clone() else {
            return Err(EngineError::new(
                "fake provider has no separator configured",
            ));
        };
        self.separator_loads.fetch_add(1, Ordering::SeqCst);
        self.log.push("separator.load");
        Ok(Box::new(FakeSeparator {
            log: self.log.clone(),
            config,
        }))
    }
}

struct FakeAsr {
    log: EngineLog,
    text: String,
}

impl AsrEngine for FakeAsr {
    fn transcribe(&self, _req: TranscribeRequest<'_>) -> Result<Transcript, EngineError> {
        self.log.push("asr.transcribe");
        Ok(Transcript {
            text: self.text.clone(),
        })
    }
}

impl Drop for FakeAsr {
    fn drop(&mut self) {
        self.log.push("asr.drop");
    }
}

struct FakeAligner {
    log: EngineLog,
    tokens: Vec<AlignedToken>,
    inputs: Arc<Mutex<Vec<String>>>,
}

impl Aligner for FakeAligner {
    fn align(&self, req: AlignRequest<'_>) -> Result<Vec<AlignedToken>, EngineError> {
        self.log.push("aligner.align");
        self.inputs.lock().unwrap().push(req.text.to_string());
        Ok(self.tokens.clone())
    }
}

impl Drop for FakeAligner {
    fn drop(&mut self) {
        self.log.push("aligner.drop");
    }
}

struct FakeSeparator {
    log: EngineLog,
    config: FakeSeparation,
}

impl Separator for FakeSeparator {
    fn separate(
        &self,
        req: SeparateRequest<'_>,
        on_event: &mut dyn FnMut(SeparationEvent),
    ) -> Result<PathBuf, EngineError> {
        self.log.push("separator.separate");
        if self.config.fall_back_to_cpu {
            on_event(SeparationEvent::FellBackToCpu {
                reason: "fake GPU failure".into(),
            });
        }
        for done in 1..=self.config.progress_total {
            on_event(SeparationEvent::Progress {
                done,
                total: self.config.progress_total,
            });
        }
        let out = req.out_dir.join("vocals.wav");
        write_silence_16k_mono(&out, self.config.vocals_seconds)
            .map_err(|e| EngineError::new(format!("fake separator: {e}")))?;
        Ok(out)
    }
}

impl Drop for FakeSeparator {
    fn drop(&mut self) {
        self.log.push("separator.drop");
    }
}

/// Write a valid 16 kHz mono PCM WAV (the pipeline's own master format, so the
/// transcode step takes its copy fast-path and needs no ffmpeg).
fn write_silence_16k_mono(path: &Path, seconds: f32) -> Result<(), String> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).map_err(|e| e.to_string())?;
    let samples = (seconds.max(0.0) * 16_000.0) as usize;
    for _ in 0..samples {
        writer.write_sample(0i16).map_err(|e| e.to_string())?;
    }
    writer.finalize().map_err(|e| e.to_string())
}

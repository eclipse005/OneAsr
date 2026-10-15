//! Local engine adapters: Qwen3-ASR, Qwen3-ForcedAligner, HTDemucs.
//!
//! Everything that knows about concrete model crates lives here — the pipeline
//! only sees [`crate::engine`] ports. Backend resolution (CPU/GPU, the
//! GPU→CPU fallback policy, probing) is also owned here, because that is
//! engine knowledge rather than pipeline policy.
//!
//! The public paths are re-exported below, so splitting the adapters into
//! submodules did not change `engine::local::*`.

mod aligner;
mod backend;
mod ctc_aligner;
mod demucs;
mod device;
mod ja_units;
mod provider;
mod qwen_asr;
mod sensevoice;
#[cfg(test)]
mod tests;

pub use demucs::DEMUCS_WEIGHTS_FILE;
pub use provider::LocalEngineProvider;
pub use sensevoice::REQUIRED_FILES as SENSEVOICE_REQUIRED_FILES;

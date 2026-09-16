//! Local engine adapters: Qwen3-ASR, Qwen3-ForcedAligner, HTDemucs.
//!
//! Everything that knows about concrete model crates lives here — the pipeline
//! only sees [`crate::engine`] ports. Backend resolution (CPU/GPU, the
//! CUDA→CPU fallback policy, probing) is also owned here, because that is
//! engine knowledge rather than pipeline policy.
//!
//! The public paths are re-exported below, so splitting the adapters into
//! submodules did not change `engine::local::*`.

mod aligner;
mod cuda;
mod demucs;
mod provider;
mod qwen_asr;
#[cfg(test)]
mod tests;

pub use demucs::DEMUCS_WEIGHTS_FILE;
pub use provider::LocalEngineProvider;

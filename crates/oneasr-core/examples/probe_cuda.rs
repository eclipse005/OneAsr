//! Probe which compute backend this machine can actually run (CUDA vs CPU).
//!
//! Run with `cargo run --example probe_cuda`. Prints one line per check so a
//! support request can be answered from the output alone.

fn main() {
    println!("cuda_ready={}", oneasr_core::is_cuda_runtime_ready());
    println!("cuda_dir={:?}", oneasr_core::resolve_cuda_runtime_dir());
}

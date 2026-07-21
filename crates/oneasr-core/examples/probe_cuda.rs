fn main() {
    println!("cuda_ready={}", oneasr_core::is_cuda_runtime_ready());
    println!("cuda_dir={:?}", oneasr_core::resolve_cuda_runtime_dir());
}

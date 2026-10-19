# gpu-allocator patches (vs crates.io 0.28.0)

- `windows` dependency on Windows targets: `">=0.53, <=0.62"` → `"0.62"`.

Why: wgpu-hal 30.0.1 uses `windows`/`windows-core` 0.62. The loose range let
Cargo unify gpu-allocator onto an older `windows` release, which made DX12
`ID3D12*` types disagree across crate boundaries and failed `cargo check` on
Windows CI. The pin keeps the engine crates on the same ABI.

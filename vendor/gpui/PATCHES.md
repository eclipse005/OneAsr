# Vendored `gpui` 0.2.2 — local patches

This directory is the crates.io `gpui` 0.2.2 package with the changes below.
`Cargo.toml` is the normalized (auto-generated) manifest; `Cargo.toml.orig` is
the upstream original.

## Code changes

1. `src/platform/windows/events.rs` — Windows custom-titlebar NC buttons.
   Upstream Zed PR #48330: `default_prevented` no longer makes GPUI swallow
   `WM_NCLBUTTONDOWN`, which previously killed titlebar drag / min / max / close.
2. `src/platform/windows/direct_write.rs` — font fallback.
   `select_font` no longer panics on a missing font; it walks a candidate list
   and warns once instead.
3. `src/taffy.rs` — grid repeat shorthand.
   `minmax(length(0.0), fr(1.0))` spells its literals `0.0_f32` / `1.0_f32`.
   Without the suffix they fall back to `f32` through the
   `float_literal_f32_fallback` lint (`f32: From<f64>` is not satisfied), which
   warns on every build — and because `gpui` is a path dependency, cargo does
   not cap its lints, so the warning shows up in OneAsr's own build log. The
   lint is future-incompatible: it becomes a hard error in a later Rust release.

Revert to crates.io `gpui = "0.2.2"` (and drop the `[patch.crates-io]` entry in
the workspace `Cargo.toml`) once a released version contains all three fixes.

## Removed from the package

`examples/`, `tests/`, `docs/`, the nested `Cargo.lock`, and the matching
`[[example]]` / `[[test]]` manifest entries were deleted to keep the repository
smaller (~5 MB); they are not needed to build OneAsr. `src/`, `resources/`,
`build.rs`, `Cargo.toml(.orig)`, `README.md`, and `LICENSE-APACHE` are
required/retained.

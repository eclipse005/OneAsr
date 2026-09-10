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

Revert to crates.io `gpui = "0.2.2"` (and drop the `[patch.crates-io]` entry in
the workspace `Cargo.toml`) once a released version contains both fixes.

## Removed from the package

`examples/`, `tests/`, `docs/`, the nested `Cargo.lock`, and the matching
`[[example]]` / `[[test]]` manifest entries were deleted to keep the repository
smaller (~5 MB); they are not needed to build OneAsr. `src/`, `resources/`,
`build.rs`, `Cargo.toml(.orig)`, `README.md`, and `LICENSE-APACHE` are
required/retained.

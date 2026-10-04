//! Comline's standard library: the schemas of the `std` package in
//! `packages/std`, embedded so every consumer of `comline-core` (the CLI, the
//! language server, the wasm playground) can resolve `use std::…` with no files
//! on disk and no network. Its version is the toolchain's.

/// Every schema of the `std` package: its path under `src/` without the
/// extension (`/`-separated when nested), and its source.
pub const SCHEMAS: &[(&str, &str)] = &[
    ("http", include_str!("../packages/std/src/http.ids")),
    ("validators", include_str!("../packages/std/src/validators.ids")),
];

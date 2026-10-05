//! Comline's standard library: the schemas of the `std` package in
//! `packages/std`, embedded so every consumer of `comline-core` (the CLI, the
//! language server, the wasm playground) can resolve `use std::…` with no files
//! on disk and no network. Its version is the toolchain's.

/// The `std` package's manifest. Its `//!` header documents the package
/// itself (see `comline_core::schema::idl::module_docs`).
pub const MANIFEST: &str = include_str!("../packages/std/config.idp");

/// Every schema of the `std` package: its path under `src/` without the
/// extension (`/`-separated when nested), and its source.
pub const SCHEMAS: &[(&str, &str)] = &[
    ("http", include_str!("../packages/std/src/http.ids")),
    ("validators", include_str!("../packages/std/src/validators.ids")),
];

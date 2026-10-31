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

/// The official encodings package (`packages/comline`): its manifest, which
/// declares `packed@1`, `datagram@2` and the rest under `encodings`. Every
/// package sees its declarations, as it sees std.
pub const OFFICIAL_MANIFEST: &str = include_str!("../packages/comline/config.idp");

/// The official encodings' profiles: each one's path as the manifest names
/// it, and its contents.
pub const OFFICIAL_PROFILES: &[(&str, &str)] = &[
    ("profiles/packed@1.json", include_str!("../packages/comline/profiles/packed@1.json")),
    ("profiles/json@1.json", include_str!("../packages/comline/profiles/json@1.json")),
    ("profiles/msgpack@1.json", include_str!("../packages/comline/profiles/msgpack@1.json")),
    ("profiles/jsonrpc@1.json", include_str!("../packages/comline/profiles/jsonrpc@1.json")),
    ("profiles/datagram@1.json", include_str!("../packages/comline/profiles/datagram@1.json")),
    ("profiles/datagram@2.json", include_str!("../packages/comline/profiles/datagram@2.json")),
    ("profiles/comline@1.json", include_str!("../packages/comline/profiles/comline@1.json")),
];

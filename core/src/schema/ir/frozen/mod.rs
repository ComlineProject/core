// Relative Modules
pub mod unit;
// Removed: basic_storage - replaced by CAS
pub mod cas;  // CAS module (public for tests and build)

/// Canonical frozen-schema digest for the connection handshake's `ir_hash`.
pub use cas::blob::schema_ir_hash;

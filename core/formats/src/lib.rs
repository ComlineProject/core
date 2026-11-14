//! The IR formats comline has stored under `.comline/`, frozen (core#268).
//!
//! One module per format, `v<N>`, each with its `SCHEMA.md`, `types`, `codec`
//! and `ledger.toml`. Nothing here depends on `comline-core`: a frozen type
//! can't reach for today's. Reading only; nothing in this crate writes a
//! store.
//!
//! A format is held in place by the gate (core#268, step 1.2): the current
//! one by core's `live == frozen(current)` test, and one that a later format
//! superseded by its seal in `formats/MANIFEST`.

pub mod v3;

/// The newest format this crate holds. Core's `CURRENT_IR_FORMAT_VERSION`
/// is tested to be this number: a bump that doesn't add the format here
/// fails.
pub const CURRENT: u32 = v3::FORMAT;

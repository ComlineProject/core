//! Everything settings-related that needs more than one schema's own
//! `&[FrozenUnit]` in isolation - resolution (dotted-key/`mode`
//! validation, pre-freeze) and enforcement (checking real declarations
//! against effective settings, post-freeze). Grouped here rather than as
//! flat siblings in `compiler/` since both grow from the same feature and
//! share no code with `import_resolver`/`alias_resolution` beyond the
//! calling convention.

pub mod enforcement;
pub mod resolution;

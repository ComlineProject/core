//! Package-wide authoring policy: a shared, nested value/merge model used
//! by both the `.ids` schema grammar's `settings` block and `.idp`'s
//! package-level `settings` field. The two grammars stay separate
//! (`schema::idl::grammar` vs. `package::config::idl::grammar`) - this
//! module is where both sides' frozen IR already converges, so it's the
//! natural place to share one value type and one merge algorithm rather
//! than duplicating them per grammar.
//!
//! Fully wired up: layering/merge, enforcement
//! (`schema::ir::compiler::settings::enforcement`), cross-package sharing,
//! and editor hover/completion (`catalog`) all consume this module's
//! types for real. `defaults` holds the one built-in policy a package
//! gets when it writes no `settings` at all.

pub mod catalog;
pub mod defaults;
pub mod desugar;
#[cfg(feature = "compiler")]
pub mod effective;
pub mod keys;
pub mod merge;
pub mod value;

pub use value::{SettingsDict, SettingsSpans, SettingsValue};

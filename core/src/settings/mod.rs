//! Package-wide authoring policy: a shared, nested value/merge model used
//! by both the `.ids` schema grammar's `settings` block and `.idp`'s
//! package-level `settings` field. The two grammars stay separate
//! (`schema::idl::grammar` vs. `package::config::idl::grammar`) - this
//! module is where both sides' frozen IR already converges, so it's the
//! natural place to share one value type and one merge algorithm rather
//! than duplicating them per grammar.
//!
//! Single-package slice only: no cross-package references, no
//! enforcement. This computes "effective settings" correctly as a tested
//! library capability with no live caller yet - mirrors how today's
//! `.ids` settings block is already parsed and frozen but inert.

pub mod desugar;
pub mod effective;
pub mod merge;
pub mod value;

pub use value::{SettingsDict, SettingsValue};

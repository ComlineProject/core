//! IR format 3: `.comline/FORMAT_VERSION` holding `3`.
//!
//! [`types`] are the frozen copies of what the format stores, [`codec`] is
//! the only code that reads and writes its bytes, `SCHEMA.md` documents it
//! and `ledger.toml` lists what was added to it without a bump. The format
//! is not sealed while it is the current one; core's tests hold the live
//! types to it instead.

pub mod codec;
pub mod types;

/// The format's number.
pub const FORMAT: u32 = 3;

/// The names of the entries a version's trees can hold.
pub mod entry {
    /// A root tree's `schema_<n>` subtrees, one per schema file, and a
    /// dependency tree's.
    pub const SCHEMA_PREFIX: &str = "schema_";
    /// A root tree's subtree for a vendored dependency, `dep_<name>`.
    pub const DEPENDENCY_PREFIX: &str = "dep_";
    /// The package's frozen config, a list of config units.
    pub const CONFIG: &str = "config";
    /// The `//!` headers ([`FrozenDocs`](super::types::FrozenDocs)).
    pub const DOCS: &str = "docs";
    /// The protocols' history ([`History`](super::types::History)).
    pub const FACTS: &str = "facts";
    /// What the steps that made the version could not know (reserved).
    pub const UNKNOWN: &str = "unknown";
    /// The core and the specification that wrote the version (reserved).
    pub const PRODUCED_BY: &str = "produced_by";
    /// In a `dep_<name>` tree: the content hash verified when vendored, as text.
    pub const MANIFEST: &str = "manifest";
    /// In a `dep_<name>` tree: the dependency's own frozen config.
    pub const DEPENDENCY_CONFIG: &str = "config";
    /// In a `dep_<name>` tree: the configs of the dependency's dependencies.
    pub const DEPENDENCY_CONFIGS: &str = "dependency_configs";
}

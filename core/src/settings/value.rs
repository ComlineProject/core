use std::collections::BTreeMap;

use serde_derive::{Deserialize, Serialize};

/// A leaf value or nested dictionary inside a `settings` block, after
/// dotted-key desugaring (see [`super::desugar`]). `Bool`/`Integer`/`Str`
/// mirror `.ids`'s `SettingValue` grammar exactly; `Identifier` is for the
/// small set of reserved bare keywords (today, only `mode`'s `replace` -
/// see [`super::merge`]); `Dict` is the nesting itself, which lives here
/// rather than in either grammar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SettingsValue {
    Bool(bool),
    Integer(i64),
    Str(String),
    Identifier(String),
    Dict(SettingsDict),
}

/// A `BTreeMap`, not a `HashMap`: this tree is bincode-serialized into CAS
/// blobs, and deterministic iteration order is load-bearing for
/// content-addressing.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SettingsDict(pub BTreeMap<String, SettingsValue>);

impl SettingsDict {
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }

    pub fn get(&self, key: &str) -> Option<&SettingsValue> {
        self.0.get(key)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

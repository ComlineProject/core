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

    /// Walk a dotted path into nested dicts - the read-side counterpart
    /// to [`super::desugar`]'s write side. `None` for a missing key at
    /// any segment, or for hitting a leaf value before the path is fully
    /// consumed (a leaf can't be walked into any further).
    pub fn get_path(&self, path: &str) -> Option<&SettingsValue> {
        let mut current = self;
        let mut segments = path.split('.').peekable();
        while let Some(segment) = segments.next() {
            let value = current.get(segment)?;
            if segments.peek().is_none() {
                return Some(value);
            }
            match value {
                SettingsValue::Dict(sub) => current = sub,
                _ => return None,
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dict(entries: &[(&str, SettingsValue)]) -> SettingsDict {
        let mut d = SettingsDict::new();
        for (k, v) in entries {
            d.0.insert(k.to_string(), v.clone());
        }
        d
    }

    #[test]
    fn get_path_single_segment_hit() {
        let d = dict(&[("a", SettingsValue::Bool(true))]);
        assert_eq!(d.get_path("a"), Some(&SettingsValue::Bool(true)));
    }

    #[test]
    fn get_path_nested_hit() {
        let inner = dict(&[("b", SettingsValue::Bool(true))]);
        let d = dict(&[("a", SettingsValue::Dict(inner))]);
        assert_eq!(d.get_path("a.b"), Some(&SettingsValue::Bool(true)));
    }

    #[test]
    fn get_path_miss_at_intermediate_segment() {
        let d = dict(&[("a", SettingsValue::Bool(true))]);
        assert_eq!(d.get_path("a.b"), None);
    }

    #[test]
    fn get_path_leaf_before_path_consumed_is_none() {
        let d = dict(&[("a", SettingsValue::Bool(true))]);
        assert_eq!(d.get_path("a.b.c"), None);
    }

    #[test]
    fn get_path_empty_string_is_none() {
        let d = dict(&[("", SettingsValue::Bool(true))]);
        // `"".split('.')` yields one empty segment, so this actually
        // looks up the key `""` - documenting the behavior, not just
        // asserting None blindly.
        assert_eq!(d.get_path(""), Some(&SettingsValue::Bool(true)));

        let empty = SettingsDict::new();
        assert_eq!(empty.get_path(""), None);
    }

    #[test]
    fn get_path_missing_top_level_key_is_none() {
        let d = SettingsDict::new();
        assert_eq!(d.get_path("a"), None);
    }
}

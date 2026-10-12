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

impl std::fmt::Display for SettingsValue {
    /// As the user would have written it, not as Rust debug-prints it.
    ///
    /// Anything shown to a user goes through this: a `Debug` dump leaks
    /// the variant name, so a hover said `Bool(false)` and a diagnostic
    /// said `Identifier("Tru")` where the source said `false` and `Tru`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SettingsValue::Bool(b) => write!(f, "{b}"),
            SettingsValue::Integer(i) => write!(f, "{i}"),
            SettingsValue::Str(s) => write!(f, "{s:?}"),
            // A bare keyword, so no quotes - that is the distinction.
            SettingsValue::Identifier(id) => write!(f, "{id}"),
            SettingsValue::Dict(_) => write!(f, "{{...}}"),
        }
    }
}

/// A `BTreeMap`, not a `HashMap`: this tree is serialized into CAS blobs,
/// and deterministic iteration order is load-bearing for content-addressing
/// (a `HashMap`'s iteration order isn't stable across runs, which would
/// make the same logical settings hash differently from one build to the
/// next).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SettingsDict(pub BTreeMap<String, SettingsValue>);

/// Per-leaf origin span for one `settings` block, keyed by the same dotted
/// path [`SettingsDict::get_path`] takes - flat, not nested like
/// [`SettingsValue::Dict`]. Diagnostics-only: not part of `SettingsValue`'s
/// own shape, so it carries no weight in the value tree's own identity. A
/// stale entry left behind under a path a later `mode = replace` deleted
/// from the *value* side is harmless - every lookup is gated by the value
/// dict resolving first, so an orphaned span is simply never consulted.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SettingsSpans(pub BTreeMap<String, (usize, usize)>);

impl SettingsSpans {
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }

    pub fn get(&self, path: &str) -> Option<(usize, usize)> {
        self.0.get(path).copied()
    }
}

impl SettingsValue {
    /// This value's syntactic form, for [`crate::config_value::check`].
    /// The bridge from this surface's own value enum onto the shared
    /// vocabulary - `Identifier` is a bare word (`mode = replace`), `Str`
    /// is a quoted one, and the two must stay distinguishable.
    pub fn form(&self) -> crate::config_value::ValueForm<'_> {
        use crate::config_value::ValueForm;
        match self {
            SettingsValue::Bool(b) => ValueForm::Bool(*b),
            SettingsValue::Integer(i) => ValueForm::Integer(*i),
            SettingsValue::Str(s) => ValueForm::Str(s),
            SettingsValue::Identifier(id) => ValueForm::Name(id),
            SettingsValue::Dict(_) => ValueForm::Dict,
        }
    }
}

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

#[cfg(test)]
mod display_tests {
    use super::*;

    #[test]
    fn values_render_as_written_not_as_rust_debug() {
        assert_eq!(SettingsValue::Bool(false).to_string(), "false");
        assert_eq!(SettingsValue::Integer(8).to_string(), "8");
        assert_eq!(SettingsValue::Str("warn".into()).to_string(), "\"warn\"");
        // The one that distinguishes a bare keyword from a string.
        assert_eq!(SettingsValue::Identifier("replace".into()).to_string(), "replace");
        assert_eq!(SettingsValue::Dict(SettingsDict::default()).to_string(), "{...}");
    }
}

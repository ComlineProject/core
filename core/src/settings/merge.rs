use super::value::{SettingsDict, SettingsValue};

/// The reserved key that opts a dictionary out of merging, at whichever
/// nesting level it's written. Its value must be the bare keyword
/// `replace` (`SettingsValue::Identifier("replace")`, not a quoted
/// string) - checked eagerly by [`validate_mode_keys`] at freeze time, so
/// [`merge`] itself never has to handle an invalid one.
pub const MODE_KEY: &str = "mode";
const MODE_REPLACE: &str = "replace";

/// One `mode` key, somewhere in a settings tree, whose value isn't the
/// bare keyword `replace`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidModeValue {
    /// Dotted path to the dictionary carrying the bad `mode` key (empty
    /// string for the root).
    pub path: String,
    /// What was actually written there.
    pub found: SettingsValue,
}

/// Walk `dict` and every dictionary nested inside it, checking that
/// wherever a `mode` key appears, its value is exactly the bare keyword
/// `replace`. Collects every violation found. Run once, right after
/// desugaring, in the same freeze-time pre-pass as the dotted-key
/// conflict check - this makes an invalid `mode` a real build error today,
/// even though nothing else about settings is enforced yet.
pub fn validate_mode_keys(dict: &SettingsDict) -> Result<(), Vec<InvalidModeValue>> {
    let mut errors = Vec::new();
    walk_mode_keys(dict, "", &mut errors);
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn walk_mode_keys(dict: &SettingsDict, path: &str, errors: &mut Vec<InvalidModeValue>) {
    if let Some(value) = dict.get(MODE_KEY) {
        if !matches!(value, SettingsValue::Identifier(s) if s == MODE_REPLACE) {
            errors.push(InvalidModeValue {
                path: path.to_string(),
                found: value.clone(),
            });
        }
    }
    for (key, value) in &dict.0 {
        if key == MODE_KEY {
            continue;
        }
        if let SettingsValue::Dict(sub) = value {
            let sub_path = if path.is_empty() {
                key.clone()
            } else {
                format!("{path}.{key}")
            };
            walk_mode_keys(sub, &sub_path, errors);
        }
    }
}

/// Merge `overlay` onto `base`. Key-by-key: a key in only one side passes
/// through; a key in both recurses if both sides are dicts there,
/// otherwise `overlay` wins (it's always the more specific layer). A
/// `mode = replace` key in `overlay`, at any depth, short-circuits
/// recursion at that level: `overlay`'s own content there (minus `mode`
/// itself) becomes the result wholesale, as if merged onto an empty base.
/// That's implemented literally (a real recursive call onto a fresh empty
/// dict), so a `mode = replace` nested *inside* a replacing subtree is
/// still honored and stripped, recursively.
///
/// Infallible: both inputs are assumed already checked by
/// [`validate_mode_keys`] (a freeze-time pre-pass), so a `mode` key, if
/// present, is always exactly `replace` by the time this runs.
pub fn merge(base: &SettingsDict, overlay: &SettingsDict) -> SettingsDict {
    if is_replace_mode(overlay) {
        return merge(&SettingsDict::default(), &without_mode_key(overlay));
    }
    let mut result = base.clone();
    for (key, overlay_value) in &overlay.0 {
        if key == MODE_KEY {
            continue;
        }
        result
            .0
            .insert(key.clone(), merge_value(base.0.get(key), overlay_value));
    }
    result
}

fn merge_value(base_value: Option<&SettingsValue>, overlay_value: &SettingsValue) -> SettingsValue {
    match (base_value, overlay_value) {
        (Some(SettingsValue::Dict(b)), SettingsValue::Dict(o)) => SettingsValue::Dict(merge(b, o)),
        // No dict on the base side to merge against - still recurse
        // (not a plain clone) so any `mode` nested inside `o` is still
        // processed and stripped, the same as at the top level.
        (_, SettingsValue::Dict(o)) => SettingsValue::Dict(merge(&SettingsDict::default(), o)),
        (_, leaf) => leaf.clone(),
    }
}

fn is_replace_mode(dict: &SettingsDict) -> bool {
    matches!(dict.get(MODE_KEY), Some(SettingsValue::Identifier(s)) if s == MODE_REPLACE)
}

fn without_mode_key(dict: &SettingsDict) -> SettingsDict {
    let mut d = dict.clone();
    d.0.remove(MODE_KEY);
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(b: bool) -> SettingsValue {
        SettingsValue::Bool(b)
    }

    fn dict(entries: &[(&str, SettingsValue)]) -> SettingsDict {
        let mut d = SettingsDict::new();
        for (k, v) in entries {
            d.0.insert(k.to_string(), v.clone());
        }
        d
    }

    #[test]
    fn disjoint_keys_union() {
        let base = dict(&[("a", leaf(true))]);
        let overlay = dict(&[("b", leaf(false))]);
        let result = merge(&base, &overlay);
        assert_eq!(result.get("a"), Some(&leaf(true)));
        assert_eq!(result.get("b"), Some(&leaf(false)));
    }

    #[test]
    fn shared_leaf_overlay_wins() {
        let base = dict(&[("a", leaf(true))]);
        let overlay = dict(&[("a", leaf(false))]);
        assert_eq!(merge(&base, &overlay).get("a"), Some(&leaf(false)));
    }

    #[test]
    fn shared_dict_recurses() {
        let base = dict(&[("a", SettingsValue::Dict(dict(&[("x", leaf(true)), ("y", leaf(true))])))]);
        let overlay = dict(&[("a", SettingsValue::Dict(dict(&[("y", leaf(false))])))]);
        let result = merge(&base, &overlay);
        let SettingsValue::Dict(a) = result.get("a").unwrap() else {
            panic!("expected dict");
        };
        // base's untouched nested leaf survives...
        assert_eq!(a.get("x"), Some(&leaf(true)));
        // ...overlay's nested leaf wins where both set it.
        assert_eq!(a.get("y"), Some(&leaf(false)));
    }

    #[test]
    fn base_dict_overlay_leaf_overlay_wins_outright() {
        let base = dict(&[("a", SettingsValue::Dict(dict(&[("x", leaf(true))])))]);
        let overlay = dict(&[("a", leaf(false))]);
        assert_eq!(merge(&base, &overlay).get("a"), Some(&leaf(false)));
    }

    fn replace_marker() -> SettingsValue {
        SettingsValue::Identifier("replace".to_string())
    }

    #[test]
    fn mode_replace_at_top_level_replaces_wholesale() {
        let base = dict(&[("a", leaf(true)), ("b", leaf(true))]);
        let overlay = dict(&[("mode", replace_marker()), ("c", leaf(false))]);
        let result = merge(&base, &overlay);
        assert_eq!(result.get("a"), None);
        assert_eq!(result.get("b"), None);
        assert_eq!(result.get("c"), Some(&leaf(false)));
        assert_eq!(result.get("mode"), None, "the mode directive itself is stripped");
    }

    #[test]
    fn mode_replace_nested_two_levels_deep_only_replaces_its_own_subtree() {
        let base = dict(&[
            ("sibling", leaf(true)),
            ("a", SettingsValue::Dict(dict(&[("x", leaf(true)), ("y", leaf(true))]))),
        ]);
        let overlay = dict(&[(
            "a",
            SettingsValue::Dict(dict(&[("mode", replace_marker()), ("z", leaf(false))])),
        )]);
        let result = merge(&base, &overlay);
        // Sibling of the replaced subtree is unaffected.
        assert_eq!(result.get("sibling"), Some(&leaf(true)));
        let SettingsValue::Dict(a) = result.get("a").unwrap() else {
            panic!("expected dict");
        };
        assert_eq!(a.get("x"), None, "base content under the replaced key is gone");
        assert_eq!(a.get("y"), None);
        assert_eq!(a.get("z"), Some(&leaf(false)));
    }

    #[test]
    fn mode_replace_nested_inside_a_replaced_subtree_is_still_honored() {
        let base = dict(&[
            ("a", SettingsValue::Dict(dict(&[("b", leaf(true))]))),
            ("sibling", leaf(true)),
        ]);
        let overlay = dict(&[
            ("mode", replace_marker()),
            (
                "a",
                SettingsValue::Dict(dict(&[("mode", replace_marker()), ("c", leaf(false))])),
            ),
        ]);
        let result = merge(&base, &overlay);
        assert_eq!(result.get("sibling"), None, "outer replace drops the whole base");
        let SettingsValue::Dict(a) = result.get("a").unwrap() else {
            panic!("expected dict");
        };
        assert_eq!(a.get("b"), None);
        assert_eq!(a.get("c"), Some(&leaf(false)));
        assert_eq!(a.get("mode"), None, "inner mode directive is also stripped");
    }

    #[test]
    fn validate_mode_keys_accepts_replace() {
        let d = dict(&[("mode", replace_marker())]);
        assert!(validate_mode_keys(&d).is_ok());
    }

    #[test]
    fn validate_mode_keys_rejects_wrong_identifier() {
        let d = dict(&[("mode", SettingsValue::Identifier("merge".to_string()))]);
        let err = validate_mode_keys(&d).unwrap_err();
        assert_eq!(err.len(), 1);
        assert_eq!(err[0].path, "");
    }

    #[test]
    fn validate_mode_keys_rejects_a_string() {
        let d = dict(&[("mode", SettingsValue::Str("replace".to_string()))]);
        assert!(validate_mode_keys(&d).is_err());
    }

    #[test]
    fn validate_mode_keys_rejects_a_bool() {
        let d = dict(&[("mode", leaf(true))]);
        assert!(validate_mode_keys(&d).is_err());
    }

    #[test]
    fn validate_mode_keys_checks_nested_dicts_and_names_their_path() {
        let d = dict(&[(
            "a",
            SettingsValue::Dict(dict(&[("mode", leaf(true))])),
        )]);
        let err = validate_mode_keys(&d).unwrap_err();
        assert_eq!(err.len(), 1);
        assert_eq!(err[0].path, "a");
    }
}

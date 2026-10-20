use super::desugar::desugar_lenient;
use super::keys;
use super::value::{SettingsDict, SettingsValue};

/// The annotation Comline forbids out of the box, and why.
///
/// `@framing` changes a protocol's wire-level framing (`jsonrpc` vs
/// `datagram`), which an organization should opt into deliberately rather
/// than have any protocol silently flip on its own. `@unverified` gives up
/// guarantees peers rely on, which is the same kind of deliberate choice.
const FORBIDDEN_BY_DEFAULT: &[&str] = &["framing", "unverified"];

/// Comline's built-in default policy: `@framing` and `@unverified` are
/// forbidden until some level explicitly sets
/// `annotations.<key>.allowed = true`. Every other annotation and validator
/// stays allowed by default.
///
/// Built through the registry rather than by hand-nesting dictionaries, so
/// the key cannot drift from the one `enforcement` actually looks up -
/// `built_in_keys_are_all_recognised` pins that.
pub fn built_in() -> SettingsDict {
    let entries: Vec<(Vec<String>, SettingsValue)> = FORBIDDEN_BY_DEFAULT
        .iter()
        .map(|name| {
            let key = keys::annotations_allowed().key(None, Some(name));
            (key.split('.').map(str::to_string).collect(), SettingsValue::Bool(false))
        })
        .collect();
    desugar_lenient(&entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framing_is_forbidden_by_default() {
        let dict = built_in();
        assert_eq!(
            dict.get_path("annotations.framing.allowed"),
            Some(&SettingsValue::Bool(false))
        );
    }

    #[test]
    fn unverified_is_forbidden_by_default() {
        assert_eq!(
            built_in().get_path("annotations.unverified.allowed"),
            Some(&SettingsValue::Bool(false))
        );
    }

    #[test]
    fn nothing_else_is_mentioned() {
        let dict = built_in();
        assert_eq!(dict.get_path("annotations.timeout_ms.allowed"), None);
        assert_eq!(dict.get_path("validators.allowed"), None);
    }

    #[test]
    fn built_in_keys_are_all_recognised() {
        // Core's own defaults must satisfy core's own registry - otherwise
        // the forthcoming unknown-key pass would reject the policy Comline
        // ships with.
        let mut leaves = Vec::new();
        collect_leaf_paths(&built_in(), &mut Vec::new(), &mut leaves);
        assert!(!leaves.is_empty(), "the built-in policy should not be empty");
        for path in leaves {
            let segments: Vec<&str> = path.iter().map(String::as_str).collect();
            assert!(
                keys::is_known(&segments),
                "built-in default key '{}' is not a recognised shape",
                path.join(".")
            );
        }
    }

    #[test]
    fn built_in_values_match_their_keys_declared_types() {
        let mut leaves = Vec::new();
        collect_leaf_paths(&built_in(), &mut Vec::new(), &mut leaves);
        for path in leaves {
            let segments: Vec<&str> = path.iter().map(String::as_str).collect();
            let m = keys::match_path(&segments).expect("covered by the test above");
            let value = built_in().get_path(&path.join(".")).unwrap().clone();
            assert!(
                crate::config_value::check(m.value_type(), value.form()).is_ok(),
                "built-in default '{}' has a value its own key does not accept",
                path.join(".")
            );
        }
    }

    fn collect_leaf_paths(
        dict: &SettingsDict,
        prefix: &mut Vec<String>,
        out: &mut Vec<Vec<String>>,
    ) {
        for (key, value) in &dict.0 {
            prefix.push(key.clone());
            match value {
                SettingsValue::Dict(sub) => collect_leaf_paths(sub, prefix, out),
                _ => out.push(prefix.clone()),
            }
            prefix.pop();
        }
    }
}

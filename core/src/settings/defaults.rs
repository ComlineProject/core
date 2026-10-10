use std::collections::BTreeMap;

use super::value::{SettingsDict, SettingsValue};

/// Comline's one built-in default policy: `@framing` changes a protocol's
/// wire-level framing (`jsonrpc` vs `datagram`), which an organization
/// should opt into deliberately rather than have any protocol silently
/// flip on its own - forbidden until some level explicitly sets
/// `annotations.framing.allowed = true`. Every other annotation and
/// validator stays allowed by default, same as before this existed.
pub fn built_in() -> SettingsDict {
    let framing = SettingsDict(BTreeMap::from([(
        "allowed".to_string(),
        SettingsValue::Bool(false),
    )]));
    let annotations = SettingsDict(BTreeMap::from([(
        "framing".to_string(),
        SettingsValue::Dict(framing),
    )]));
    SettingsDict(BTreeMap::from([(
        "annotations".to_string(),
        SettingsValue::Dict(annotations),
    )]))
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
    fn nothing_else_is_mentioned() {
        let dict = built_in();
        assert_eq!(dict.get_path("annotations.timeout_ms.allowed"), None);
        assert_eq!(dict.get_path("validators.allowed"), None);
    }
}

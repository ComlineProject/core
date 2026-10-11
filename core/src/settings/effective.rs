use std::collections::BTreeMap;

use crate::package::config::ir::context::ProjectContext;
use crate::package::config::ir::frozen::FrozenUnit as ConfigFrozenUnit;
use crate::schema::ir::frozen::unit::FrozenUnit as SchemaFrozenUnit;

use super::merge::{merge, merge_spans};
use super::value::{SettingsDict, SettingsSpans};

/// Every settings block one schema declares: its unnamed, file-wide block
/// (if any), and every named, opt-in preset, keyed by name - plus each
/// one's origin-span sidecar, for diagnostics that need to point at the
/// real `settings { ... }` entry rather than wherever it's used.
#[derive(Debug, Clone, Default)]
pub struct SchemaSettings {
    pub unnamed: Option<SettingsDict>,
    pub named: BTreeMap<String, SettingsDict>,
    pub unnamed_spans: Option<SettingsSpans>,
    pub named_spans: BTreeMap<String, SettingsSpans>,
}

/// Collect every `settings` block a schema's frozen units contain.
pub fn schema_settings(frozen_schema_units: &[SchemaFrozenUnit]) -> SchemaSettings {
    let mut out = SchemaSettings::default();
    for unit in frozen_schema_units {
        if let SchemaFrozenUnit::Settings { name, values, spans, .. } = unit {
            match name {
                None => {
                    out.unnamed = Some(values.clone());
                    out.unnamed_spans = Some(spans.clone());
                }
                Some(n) => {
                    out.named.insert(n.clone(), values.clone());
                    out.named_spans.insert(n.clone(), spans.clone());
                }
            }
        }
    }
    out
}

/// The package default, from `project_context.config_frozen`, merged with
/// one schema's own unnamed block (if present). The package-level
/// `FrozenUnit::Settings` is always present by the time config is frozen
/// (an absent `settings = {...}` in `.idp` still freezes an empty
/// default), so a missing one here just means an empty dict.
pub fn package_settings(project_context: &ProjectContext) -> SettingsDict {
    project_context
        .config_frozen
        .as_deref()
        .and_then(|units| {
            units.iter().find_map(|u| match u {
                ConfigFrozenUnit::Settings(d) => Some(d),
                _ => None,
            })
        })
        .cloned()
        .unwrap_or_default()
}

/// Package default, merged with one schema's own unnamed block.
pub fn effective_schema_settings(
    package_settings: &SettingsDict,
    schema: &SchemaSettings,
) -> SettingsDict {
    match &schema.unnamed {
        Some(overlay) => merge(package_settings, overlay),
        None => package_settings.clone(),
    }
}

/// [`effective_schema_settings`]'s origin-span counterpart - a fully
/// separate function rather than a return-type change on the one above, so
/// every existing caller (including the language server's settings-hover
/// feature) keeps its current signature untouched.
pub fn effective_schema_settings_spans(
    package_spans: &SettingsSpans,
    schema: &SchemaSettings,
) -> SettingsSpans {
    match &schema.unnamed_spans {
        Some(overlay) => merge_spans(package_spans, overlay),
        None => package_spans.clone(),
    }
}

/// The `@settings = Name` value off one declaration's already-frozen
/// annotation parameters (`FrozenUnit::Property` entries) - `None` if
/// there's no such annotation.
pub fn settings_annotation_of(parameters: &[SchemaFrozenUnit]) -> Option<&str> {
    parameters.iter().find_map(|p| match p {
        SchemaFrozenUnit::Property { name, expression } if name == "settings" => {
            expression.as_deref()
        }
        _ => None,
    })
}

/// `effective_schema_settings` merged with a named preset, if
/// `settings_name` names one this schema declares. A name that doesn't
/// resolve is a silent no-op by design for this slice - resolving or
/// validating the annotation's reference is enforcement work, out of
/// scope here (mirrors "parsed, frozen, inert").
pub fn effective_declaration_settings(
    effective_schema: &SettingsDict,
    schema: &SchemaSettings,
    settings_name: Option<&str>,
) -> SettingsDict {
    match settings_name.and_then(|n| schema.named.get(n)) {
        Some(overlay) => merge(effective_schema, overlay),
        None => effective_schema.clone(),
    }
}

/// [`effective_declaration_settings`]'s origin-span counterpart - see
/// [`effective_schema_settings_spans`] for why this is a separate function.
pub fn effective_declaration_settings_spans(
    effective_schema_spans: &SettingsSpans,
    schema: &SchemaSettings,
    settings_name: Option<&str>,
) -> SettingsSpans {
    match settings_name.and_then(|n| schema.named_spans.get(n)) {
        Some(overlay) => merge_spans(effective_schema_spans, overlay),
        None => effective_schema_spans.clone(),
    }
}

/// The single public entry point: every layer, composed.
/// `package_settings` is the already-extracted package-level dict (see
/// [`package_settings`] for a caller with a real `ProjectContext` - this
/// function takes the dict directly rather than `ProjectContext` itself,
/// since that's the only thing it was ever used for, and `ProjectContext`
/// can't be constructed/cached everywhere a caller might want this - e.g.
/// the language server, which only ever has the dict). `declaration_parameters`
/// is `None` for "just the schema's effective settings" (no particular
/// declaration in view), `Some(params)` for one declaration's frozen
/// `parameters` list (checked for `@settings = Name`).
pub fn effective_settings(
    package_settings: &SettingsDict,
    schema_frozen_units: &[SchemaFrozenUnit],
    declaration_parameters: Option<&[SchemaFrozenUnit]>,
) -> SettingsDict {
    let schema = schema_settings(schema_frozen_units);
    let schema_effective = effective_schema_settings(package_settings, &schema);

    match declaration_parameters {
        None => schema_effective,
        Some(params) => effective_declaration_settings(
            &schema_effective,
            &schema,
            settings_annotation_of(params),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::value::SettingsValue;

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

    fn spans(entries: &[(&str, (usize, usize))]) -> SettingsSpans {
        let mut s = SettingsSpans::new();
        for (k, v) in entries {
            s.0.insert(k.to_string(), *v);
        }
        s
    }

    #[test]
    fn schema_settings_splits_unnamed_and_named() {
        let units = vec![
            SchemaFrozenUnit::Settings {
                docstring: None,
                name: None,
                values: dict(&[("a", leaf(true))]),
                spans: spans(&[("a", (0, 1))]),
            },
            SchemaFrozenUnit::Settings {
                docstring: None,
                name: Some("Strict".to_string()),
                values: dict(&[("b", leaf(false))]),
                spans: spans(&[("b", (2, 3))]),
            },
        ];
        let s = schema_settings(&units);
        assert_eq!(s.unnamed.unwrap().get("a"), Some(&leaf(true)));
        assert_eq!(s.named.get("Strict").unwrap().get("b"), Some(&leaf(false)));
        assert_eq!(s.unnamed_spans.unwrap().get("a"), Some((0, 1)));
        assert_eq!(s.named_spans.get("Strict").unwrap().get("b"), Some((2, 3)));
    }

    #[test]
    fn effective_schema_settings_with_no_unnamed_block_returns_package_default() {
        let package = dict(&[("a", leaf(true))]);
        let schema = SchemaSettings::default();
        let result = effective_schema_settings(&package, &schema);
        assert_eq!(result.get("a"), Some(&leaf(true)));
    }

    #[test]
    fn effective_schema_settings_spans_merges_package_and_schema_origins() {
        let package_spans = spans(&[("a", (0, 1))]);
        let schema = SchemaSettings {
            unnamed_spans: Some(spans(&[("b", (10, 11))])),
            ..SchemaSettings::default()
        };
        let result = effective_schema_settings_spans(&package_spans, &schema);
        assert_eq!(result.get("a"), Some((0, 1)));
        assert_eq!(result.get("b"), Some((10, 11)));
    }

    #[test]
    fn effective_declaration_settings_spans_merges_in_the_named_presets_origins() {
        let effective_spans = spans(&[("a", (0, 1))]);
        let mut schema = SchemaSettings::default();
        schema.named_spans.insert("Strict".to_string(), spans(&[("a", (20, 21))]));
        let result = effective_declaration_settings_spans(&effective_spans, &schema, Some("Strict"));
        assert_eq!(result.get("a"), Some((20, 21)), "the preset's own span should win");
    }

    #[test]
    fn effective_declaration_settings_spans_with_unresolved_name_is_a_no_op() {
        let effective_spans = spans(&[("a", (0, 1))]);
        let schema = SchemaSettings::default();
        let result = effective_declaration_settings_spans(&effective_spans, &schema, Some("Missing"));
        assert_eq!(result.get("a"), Some((0, 1)));
    }

    #[test]
    fn effective_declaration_settings_with_no_annotation_is_a_no_op() {
        let effective = dict(&[("a", leaf(true))]);
        let schema = SchemaSettings::default();
        let result = effective_declaration_settings(&effective, &schema, None);
        assert_eq!(result.get("a"), Some(&leaf(true)));
    }

    #[test]
    fn effective_declaration_settings_with_unresolved_name_is_a_no_op() {
        let effective = dict(&[("a", leaf(true))]);
        let schema = SchemaSettings::default();
        let result = effective_declaration_settings(&effective, &schema, Some("Missing"));
        assert_eq!(result.get("a"), Some(&leaf(true)));
    }

    #[test]
    fn settings_annotation_of_finds_the_settings_property() {
        let params = vec![
            SchemaFrozenUnit::Property {
                name: "timeout_ms".to_string(),
                expression: Some("500".to_string()),
            },
            SchemaFrozenUnit::Property {
                name: "settings".to_string(),
                expression: Some("Strict".to_string()),
            },
        ];
        assert_eq!(settings_annotation_of(&params), Some("Strict"));
    }

    #[test]
    fn settings_annotation_of_none_when_absent() {
        let params = vec![SchemaFrozenUnit::Property {
            name: "timeout_ms".to_string(),
            expression: Some("500".to_string()),
        }];
        assert_eq!(settings_annotation_of(&params), None);
    }
}

//! Enforces effective settings against real declarations - the consumer
//! `core::settings::effective` was always missing. A declaration that
//! uses an annotation or validator its effective settings forbid is a
//! hard build error; a `@settings = Name` naming a preset the schema
//! doesn't declare is too. Lives in `settings::enforcement`, alongside
//! `settings::resolution` (dotted-key/`mode` validation) - same reasoning
//! as both that sibling and `import_resolver`/`alias_resolution`: this
//! needs more than one schema in isolation (a package-level dict), so it
//! doesn't fit `validation::validator`'s schema-only passes, and changing
//! that function's signature would touch every other caller for no
//! benefit.

use crate::diagnostics::Diagnostic;
use crate::schema::ir::frozen::unit::FrozenUnit;
use crate::settings::effective::{
    effective_declaration_settings, effective_schema_settings, schema_settings,
    settings_annotation_of, SchemaSettings,
};
use crate::settings::value::{SettingsDict, SettingsValue};

/// Walk every annotation-bearing declaration in `units` (`Struct`+its
/// `Field`s, `Error`+its `Field`s, `Protocol`+its `Function`s - the only
/// five kinds that can carry annotations at all) and check each one's
/// annotations/validators against its own effective settings. Collects
/// every violation rather than stopping at the first.
pub fn check_settings_enforcement(
    units: &[FrozenUnit],
    package_settings: &SettingsDict,
) -> Result<(), Vec<Diagnostic>> {
    let schema = schema_settings(units);
    let schema_effective = effective_schema_settings(package_settings, &schema);
    let mut errors = Vec::new();

    for unit in units {
        match unit {
            FrozenUnit::Struct { name, parameters, fields, span, .. } => {
                check_declaration(
                    &schema_effective, &schema, "struct", parameters,
                    &format!("struct '{name}'"), Some(*span), &mut errors,
                );
                for field in fields {
                    check_field(&schema_effective, &schema, "struct.field", field, &mut errors);
                }
            }
            FrozenUnit::Error { name, parameters, fields, .. } => {
                // `FrozenUnit::Error` carries no span - its own
                // violations are necessarily spanless (matches
                // `validation::validator`'s existing precedent for the
                // same gap, e.g. its `Validator` symbol-table entries).
                check_declaration(
                    &schema_effective, &schema, "error", parameters,
                    &format!("error '{name}'"), None, &mut errors,
                );
                for field in fields {
                    check_field(&schema_effective, &schema, "error.field", field, &mut errors);
                }
            }
            FrozenUnit::Protocol { name, parameters, functions, span, .. } => {
                check_declaration(
                    &schema_effective, &schema, "protocol", parameters,
                    &format!("protocol '{name}'"), Some(*span), &mut errors,
                );
                for function in functions {
                    if let FrozenUnit::Function { name: fname, parameters: fp, span: fs, .. } = function {
                        check_declaration(
                            &schema_effective, &schema, "protocol.function", fp,
                            &format!("protocol '{name}', function '{fname}'"), Some(*fs), &mut errors,
                        );
                    }
                }
            }
            _ => {}
        }
    }

    if errors.is_empty() { Ok(()) } else { Err(errors) }
}

fn check_field(
    schema_effective: &SettingsDict,
    schema: &SchemaSettings,
    decl_path: &str,
    field: &FrozenUnit,
    errors: &mut Vec<Diagnostic>,
) {
    if let FrozenUnit::Field { name, parameters, span, .. } = field {
        check_declaration(
            schema_effective, schema, decl_path, parameters,
            &format!("field '{name}'"), Some(*span), errors,
        );
    }
}

/// One declaration: resolve its own effective settings (package/schema
/// layers plus its own `@settings = Name` preset, if any - and if that
/// name doesn't resolve, that's its own error), then check every
/// annotation/validator it actually uses.
fn check_declaration(
    schema_effective: &SettingsDict,
    schema: &SchemaSettings,
    decl_path: &str,
    parameters: &[FrozenUnit],
    decl_label: &str,
    span: Option<(usize, usize)>,
    errors: &mut Vec<Diagnostic>,
) {
    let settings_name = settings_annotation_of(parameters);
    if let Some(name) = settings_name {
        if !schema.named.contains_key(name) {
            errors.push(
                Diagnostic::new(format!("settings preset '{name}' not found"))
                    .with_context(decl_label.to_string())
                    .with_help("check for a typo, or that it's declared as 'settings Name { ... }' in this schema")
                    .maybe_span(span),
            );
        }
    }
    let effective = effective_declaration_settings(schema_effective, schema, settings_name);

    for param in parameters {
        match param {
            // The preset selector itself, not a policy-governed feature.
            FrozenUnit::Property { name, .. } if name == "settings" => {}
            FrozenUnit::Property { name, .. } => {
                check_annotation(&effective, decl_path, name, decl_label, span, errors);
            }
            FrozenUnit::ValidatorRef { name, .. } => {
                check_validator_ref(&effective, decl_path, name, decl_label, span, errors);
            }
            _ => {}
        }
    }
}

/// A plain `@name`/`@name = value` annotation: decl-scoped key first,
/// global key second, allowed by default if neither exists.
fn check_annotation(
    effective: &SettingsDict,
    decl_path: &str,
    name: &str,
    decl_label: &str,
    span: Option<(usize, usize)>,
    errors: &mut Vec<Diagnostic>,
) {
    let candidates = [
        format!("{decl_path}.annotations.{name}.allowed"),
        format!("annotations.{name}.allowed"),
    ];
    verdict(effective, &candidates, decl_label, span, errors, |key| {
        (
            format!("annotation '@{name}' is forbidden here by '{key} = false'"),
            format!("remove '@{name}', or set '{key} = true' to allow it"),
        )
    });
}

/// A `@validators = [Name(...), ...]` entry: decl-scoping dominates,
/// specific-validator-name is the secondary tie-break within each level -
/// decl+name, decl+coarse, global+name, global+coarse, then allowed.
fn check_validator_ref(
    effective: &SettingsDict,
    decl_path: &str,
    name: &str,
    decl_label: &str,
    span: Option<(usize, usize)>,
    errors: &mut Vec<Diagnostic>,
) {
    let candidates = [
        format!("{decl_path}.validators.{name}.allowed"),
        format!("{decl_path}.validators.allowed"),
        format!("validators.{name}.allowed"),
        "validators.allowed".to_string(),
    ];
    verdict(effective, &candidates, decl_label, span, errors, |key| {
        (
            format!("validator '{name}' is forbidden here by '{key} = false'"),
            format!("remove '{name}(...)' from '@validators', or set '{key} = true' to allow it"),
        )
    });
}

/// Shared resolution: the first candidate key that exists at all wins -
/// `true` passes, `false` is a hard error built by `message`, anything
/// else (not a boolean) is its own "must be a boolean" error. No
/// candidate existing at any level means allowed, silently.
fn verdict(
    effective: &SettingsDict,
    candidates: &[String],
    decl_label: &str,
    span: Option<(usize, usize)>,
    errors: &mut Vec<Diagnostic>,
    message: impl Fn(&str) -> (String, String),
) {
    let Some((key, value)) = candidates
        .iter()
        .find_map(|k| effective.get_path(k).map(|v| (k.as_str(), v)))
    else {
        return;
    };
    match value {
        SettingsValue::Bool(true) => {}
        SettingsValue::Bool(false) => {
            let (msg, help) = message(key);
            errors.push(
                Diagnostic::new(msg).with_context(decl_label.to_string()).with_help(help).maybe_span(span),
            );
        }
        other => {
            errors.push(
                Diagnostic::new(format!("'{key}' must be a boolean, got {other:?}"))
                    .with_context(decl_label.to_string())
                    .with_help("a settings '.allowed' key only accepts 'true' or 'false'")
                    .maybe_span(span),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::ir::compiler::interpreted::kind_search::{KindValue, Primitive};

    fn b(v: bool) -> SettingsValue {
        SettingsValue::Bool(v)
    }

    fn nested(path: &[&str], leaf: SettingsValue) -> SettingsDict {
        let mut value = leaf;
        for segment in path.iter().rev() {
            let mut d = SettingsDict::new();
            d.0.insert(segment.to_string(), value);
            value = SettingsValue::Dict(d);
        }
        match value {
            SettingsValue::Dict(d) => d,
            _ => unreachable!("path is always non-empty in these tests"),
        }
    }

    fn package_with(path: &[&str], leaf: SettingsValue) -> SettingsDict {
        nested(path, leaf)
    }

    fn property(name: &str) -> FrozenUnit {
        FrozenUnit::Property { name: name.to_string(), expression: None }
    }

    fn validator_ref(name: &str) -> FrozenUnit {
        FrozenUnit::ValidatorRef { name: name.to_string(), args: vec![] }
    }

    fn a_struct(name: &str, parameters: Vec<FrozenUnit>, fields: Vec<FrozenUnit>) -> FrozenUnit {
        FrozenUnit::Struct {
            docstring: None, parameters, name: name.to_string(), fields, span: (0, 1),
        }
    }

    fn a_field(name: &str, parameters: Vec<FrozenUnit>) -> FrozenUnit {
        FrozenUnit::Field {
            docstring: None, parameters, optional: false, name: name.to_string(),
            kind_value: KindValue::Primitive(Primitive::String(None)), span: (0, 1),
        }
    }

    fn an_error(name: &str, parameters: Vec<FrozenUnit>, fields: Vec<FrozenUnit>) -> FrozenUnit {
        FrozenUnit::Error {
            docstring: None, parameters, ordinal: 0, imported_from: None,
            name: name.to_string(), message: "msg".to_string(), fields,
        }
    }

    fn a_protocol(name: &str, parameters: Vec<FrozenUnit>, functions: Vec<FrozenUnit>) -> FrozenUnit {
        FrozenUnit::Protocol {
            docstring: String::new(), parameters, name: name.to_string(), functions, span: (0, 1),
        }
    }

    fn a_function(name: &str, parameters: Vec<FrozenUnit>) -> FrozenUnit {
        FrozenUnit::Function {
            docstring: String::new(), parameters, name: name.to_string(),
            arguments: vec![], _return: None, throws: vec![], span: (0, 1),
        }
    }

    // --- resolution chain: plain annotations ---

    #[test]
    fn annotation_allowed_by_default_with_no_key() {
        let package = SettingsDict::new();
        let units = vec![a_struct("S", vec![property("framing")], vec![])];
        assert!(check_settings_enforcement(&units, &package).is_ok());
    }

    #[test]
    fn annotation_forbidden_by_global_key() {
        let package = package_with(&["annotations", "framing", "allowed"], b(false));
        let units = vec![a_struct("S", vec![property("framing")], vec![])];
        let errors = check_settings_enforcement(&units, &package).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("framing"));
        assert!(errors[0].message.contains("annotations.framing.allowed"));
    }

    #[test]
    fn decl_scoped_key_beats_global_key() {
        // Global forbids, but the struct-scoped key explicitly allows -
        // scoped must win.
        let mut package = package_with(&["annotations", "framing", "allowed"], b(false));
        let scoped = nested(&["struct", "annotations", "framing", "allowed"], b(true));
        package = crate::settings::merge::merge(&package, &scoped);
        let units = vec![a_struct("S", vec![property("framing")], vec![])];
        assert!(check_settings_enforcement(&units, &package).is_ok());
    }

    #[test]
    fn malformed_allowed_value_is_its_own_error() {
        let package = package_with(&["annotations", "framing", "allowed"], SettingsValue::Integer(5));
        let units = vec![a_struct("S", vec![property("framing")], vec![])];
        let errors = check_settings_enforcement(&units, &package).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("must be a boolean"));
    }

    #[test]
    fn the_settings_property_itself_is_never_checked() {
        let package = package_with(&["annotations", "settings", "allowed"], b(false));
        let units = vec![a_struct("S", vec![property("settings")], vec![])];
        assert!(check_settings_enforcement(&units, &package).is_ok());
    }

    // --- resolution chain: validator refs ---

    #[test]
    fn validator_allowed_by_default() {
        let package = SettingsDict::new();
        let units = vec![a_struct("S", vec![], vec![a_field("f", vec![validator_ref("StringBounds")])])];
        assert!(check_settings_enforcement(&units, &package).is_ok());
    }

    #[test]
    fn validator_forbidden_by_global_coarse() {
        let package = package_with(&["validators", "allowed"], b(false));
        let units = vec![a_struct("S", vec![], vec![a_field("f", vec![validator_ref("StringBounds")])])];
        let errors = check_settings_enforcement(&units, &package).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("validators.allowed"));
    }

    #[test]
    fn decl_scoped_coarse_beats_global_named() {
        // Global forbids this specific validator; decl-scoped coarse
        // (struct.field.validators.allowed = true) allows everything on
        // struct fields - decl-scoping dominates, so this must pass.
        let mut package = package_with(&["validators", "StringBounds", "allowed"], b(false));
        let scoped = nested(&["struct", "field", "validators", "allowed"], b(true));
        package = crate::settings::merge::merge(&package, &scoped);
        let units = vec![a_struct("S", vec![], vec![a_field("f", vec![validator_ref("StringBounds")])])];
        assert!(check_settings_enforcement(&units, &package).is_ok());
    }

    #[test]
    fn decl_scoped_named_is_the_most_specific() {
        let mut package = package_with(&["struct", "field", "validators", "allowed"], b(true));
        let specific = nested(&["struct", "field", "validators", "StringBounds", "allowed"], b(false));
        package = crate::settings::merge::merge(&package, &specific);
        let units = vec![a_struct("S", vec![], vec![a_field("f", vec![validator_ref("StringBounds")])])];
        let errors = check_settings_enforcement(&units, &package).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("struct.field.validators.StringBounds.allowed"));
    }

    // --- decl-path coverage, one per kind ---

    #[test]
    fn struct_field_uses_struct_field_path() {
        let package = package_with(&["struct", "field", "annotations", "x", "allowed"], b(false));
        let units = vec![a_struct("S", vec![], vec![a_field("f", vec![property("x")])])];
        let errors = check_settings_enforcement(&units, &package).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].span, Some((0, 1)));
    }

    #[test]
    fn error_field_uses_error_field_path() {
        let package = package_with(&["error", "field", "annotations", "x", "allowed"], b(false));
        let units = vec![an_error("E", vec![], vec![a_field("f", vec![property("x")])])];
        let errors = check_settings_enforcement(&units, &package).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].span, Some((0, 1)));
    }

    #[test]
    fn bare_error_violation_has_no_span() {
        let package = package_with(&["error", "annotations", "x", "allowed"], b(false));
        let units = vec![an_error("E", vec![property("x")], vec![])];
        let errors = check_settings_enforcement(&units, &package).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].span, None);
    }

    #[test]
    fn protocol_function_uses_protocol_function_path() {
        let package = package_with(&["protocol", "function", "annotations", "x", "allowed"], b(false));
        let units = vec![a_protocol("P", vec![], vec![a_function("f", vec![property("x")])])];
        let errors = check_settings_enforcement(&units, &package).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].span, Some((0, 1)));
    }

    #[test]
    fn protocol_itself_uses_protocol_path() {
        let package = package_with(&["protocol", "annotations", "framing", "allowed"], b(false));
        let units = vec![a_protocol("P", vec![property("framing")], vec![])];
        let errors = check_settings_enforcement(&units, &package).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].span, Some((0, 1)));
    }

    // --- unresolved @settings preset ---

    #[test]
    fn unresolved_settings_preset_is_its_own_error() {
        let package = SettingsDict::new();
        let units = vec![a_struct("S", vec![
            FrozenUnit::Property { name: "settings".to_string(), expression: Some("Missing".to_string()) },
        ], vec![])];
        let errors = check_settings_enforcement(&units, &package).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("Missing"));
        assert!(errors[0].message.contains("not found"));
    }

    #[test]
    fn resolved_settings_preset_is_not_an_error() {
        let package = SettingsDict::new();
        let units = vec![
            FrozenUnit::Settings { docstring: None, name: Some("Strict".to_string()), values: SettingsDict::new() },
            a_struct("S", vec![
                FrozenUnit::Property { name: "settings".to_string(), expression: Some("Strict".to_string()) },
            ], vec![]),
        ];
        assert!(check_settings_enforcement(&units, &package).is_ok());
    }
}

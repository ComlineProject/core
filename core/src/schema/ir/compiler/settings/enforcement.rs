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
    effective_declaration_settings, effective_declaration_settings_spans,
    effective_schema_settings, effective_schema_settings_spans, schema_settings,
    settings_annotation_of, SchemaSettings,
};
use crate::settings::value::{SettingsDict, SettingsSpans, SettingsValue};

/// Walk every annotation-bearing declaration in `units` (`Struct`+its
/// `Field`s, `Error`+its `Field`s, `Protocol`+its `Function`s - the only
/// five kinds that can carry annotations at all) and check each one's
/// annotations/validators against its own effective settings. Collects
/// every violation rather than stopping at the first.
pub fn check_settings_enforcement(
    units: &[FrozenUnit],
    package_settings: &SettingsDict,
    package_spans: &SettingsSpans,
) -> Result<(), Vec<Diagnostic>> {
    let schema = schema_settings(units);
    let schema_effective = effective_schema_settings(package_settings, &schema);
    let schema_effective_spans = effective_schema_settings_spans(package_spans, &schema);
    let mut errors = Vec::new();

    for unit in units {
        match unit {
            FrozenUnit::Struct { name, parameters, fields, span, .. } => {
                check_declaration(
                    &schema_effective, &schema_effective_spans, &schema, "struct", parameters,
                    &format!("struct '{name}'"), Some(*span), &mut errors,
                );
                for field in fields {
                    check_field(&schema_effective, &schema_effective_spans, &schema, "struct.field", field, &mut errors);
                }
            }
            FrozenUnit::Error { name, parameters, fields, .. } => {
                // `FrozenUnit::Error` carries no span - its own
                // violations are necessarily spanless (matches
                // `validation::validator`'s existing precedent for the
                // same gap, e.g. its `Validator` symbol-table entries).
                check_declaration(
                    &schema_effective, &schema_effective_spans, &schema, "error", parameters,
                    &format!("error '{name}'"), None, &mut errors,
                );
                for field in fields {
                    check_field(&schema_effective, &schema_effective_spans, &schema, "error.field", field, &mut errors);
                }
            }
            FrozenUnit::Protocol { name, parameters, functions, span, .. } => {
                check_declaration(
                    &schema_effective, &schema_effective_spans, &schema, "protocol", parameters,
                    &format!("protocol '{name}'"), Some(*span), &mut errors,
                );
                for function in functions {
                    if let FrozenUnit::Function { name: fname, parameters: fp, span: fs, .. } = function {
                        check_declaration(
                            &schema_effective, &schema_effective_spans, &schema, "protocol.function", fp,
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
    schema_effective_spans: &SettingsSpans,
    schema: &SchemaSettings,
    decl_path: &str,
    field: &FrozenUnit,
    errors: &mut Vec<Diagnostic>,
) {
    if let FrozenUnit::Field { name, parameters, span, .. } = field {
        check_declaration(
            schema_effective, schema_effective_spans, schema, decl_path, parameters,
            &format!("field '{name}'"), Some(*span), errors,
        );
    }
}

/// One declaration: resolve its own effective settings (package/schema
/// layers plus its own `@settings = Name` preset, if any - and if that
/// name doesn't resolve, that's its own error), then check every
/// annotation/validator it actually uses.
///
/// `schema_effective`/`schema_effective_spans` are two genuinely separate
/// lookup tables (values vs. their origin spans) threaded in parallel, not
/// one struct - bundling them would force every caller to build a wrapper
/// just to pass two things that are already computed together one level up.
#[allow(clippy::too_many_arguments)]
fn check_declaration(
    schema_effective: &SettingsDict,
    schema_effective_spans: &SettingsSpans,
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
    let effective_spans =
        effective_declaration_settings_spans(schema_effective_spans, schema, settings_name);

    for param in parameters {
        match param {
            // The preset selector itself, not a policy-governed feature.
            FrozenUnit::Property { name, .. } if name == "settings" => {}
            FrozenUnit::Property { name, .. } => {
                check_annotation(&effective, &effective_spans, decl_path, name, decl_label, span, errors);
            }
            FrozenUnit::ValidatorRef { name, .. } => {
                check_validator_ref(&effective, &effective_spans, decl_path, name, decl_label, span, errors);
            }
            _ => {}
        }
    }
}

/// A plain `@name`/`@name = value` annotation: decl-scoped key first,
/// global key second, allowed by default if neither exists.
fn check_annotation(
    effective: &SettingsDict,
    effective_spans: &SettingsSpans,
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
    verdict(effective, effective_spans, &candidates, decl_label, span, errors, |key| {
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
    effective_spans: &SettingsSpans,
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
    verdict(effective, effective_spans, &candidates, decl_label, span, errors, |key| {
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
///
/// The two error shapes point at different places, deliberately: "forbidden
/// by `= false`" is a real, correctly-written setting simply disallowing
/// this usage - `span` (the usage site) is the right place to show that. A
/// non-boolean value is a mistake in the settings block itself, so when its
/// origin span is known (`effective_spans`), that becomes the primary span,
/// with the usage site attached as a secondary pointer - when it isn't
/// known (hand-built settings in tests, or a package-level origin, not yet
/// span-tracked), this falls back to today's usage-site-only behavior.
fn verdict(
    effective: &SettingsDict,
    effective_spans: &SettingsSpans,
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
            let mut d = Diagnostic::new(format!("'{key}' must be a boolean, got {other:?}"))
                .with_help("a settings '.allowed' key only accepts 'true' or 'false'");
            d = match effective_spans.get(key) {
                Some(origin) => {
                    d = d.with_span(origin).with_context(format!("'{key}' set here"));
                    match span {
                        Some(usage) => d.with_secondary_span(usage, decl_label.to_string()),
                        None => d,
                    }
                }
                None => d.with_context(decl_label.to_string()).maybe_span(span),
            };
            errors.push(d);
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
        assert!(check_settings_enforcement(&units, &package, &SettingsSpans::new()).is_ok());
    }

    #[test]
    fn annotation_forbidden_by_global_key() {
        let package = package_with(&["annotations", "framing", "allowed"], b(false));
        let units = vec![a_struct("S", vec![property("framing")], vec![])];
        let errors = check_settings_enforcement(&units, &package, &SettingsSpans::new()).unwrap_err();
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
        assert!(check_settings_enforcement(&units, &package, &SettingsSpans::new()).is_ok());
    }

    #[test]
    fn malformed_allowed_value_is_its_own_error() {
        let package = package_with(&["annotations", "framing", "allowed"], SettingsValue::Integer(5));
        let units = vec![a_struct("S", vec![property("framing")], vec![])];
        let errors = check_settings_enforcement(&units, &package, &SettingsSpans::new()).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("must be a boolean"));
    }

    #[test]
    fn malformed_allowed_value_falls_back_to_the_usage_site_with_no_known_origin() {
        // No spans sidecar given (a hand-built `SettingsDict`, same as
        // every test in this module, or a not-yet-span-tracked
        // package-level origin) - graceful fallback to today's behavior,
        // not a regression.
        let package = package_with(&["annotations", "framing", "allowed"], SettingsValue::Integer(5));
        let units = vec![a_struct("S", vec![property("framing")], vec![])];
        let errors = check_settings_enforcement(&units, &package, &SettingsSpans::new()).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].span, Some((0, 1)), "falls back to the usage site");
        assert_eq!(errors[0].secondary, None);
    }

    #[test]
    fn malformed_allowed_value_points_at_its_origin_when_known() {
        let package = package_with(&["annotations", "framing", "allowed"], SettingsValue::Integer(5));
        let mut package_spans = SettingsSpans::new();
        package_spans.0.insert("annotations.framing.allowed".to_string(), (40, 41));
        let units = vec![a_struct("S", vec![property("framing")], vec![])];
        let errors = check_settings_enforcement(&units, &package, &package_spans).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].span, Some((40, 41)), "primary span is the settings entry's own origin");
        assert_eq!(
            errors[0].secondary.as_ref().map(|(span, _)| *span),
            Some((0, 1)),
            "secondary span is the usage site"
        );
    }

    #[test]
    fn forbidden_by_false_stays_usage_site_only_even_when_an_origin_span_is_known() {
        // The sibling case to the two tests above: a *correctly written*
        // `= false` forbidding this usage must keep pointing at the usage
        // site, never the settings entry - even when that entry's origin
        // span happens to be known.
        let package = package_with(&["annotations", "framing", "allowed"], b(false));
        let mut package_spans = SettingsSpans::new();
        package_spans.0.insert("annotations.framing.allowed".to_string(), (40, 41));
        let units = vec![a_struct("S", vec![property("framing")], vec![])];
        let errors = check_settings_enforcement(&units, &package, &package_spans).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].span, Some((0, 1)), "the forbidden case must keep pointing at the usage site");
        assert_eq!(errors[0].secondary, None);
    }

    #[test]
    fn the_settings_property_itself_is_never_checked() {
        let package = package_with(&["annotations", "settings", "allowed"], b(false));
        let units = vec![a_struct("S", vec![property("settings")], vec![])];
        assert!(check_settings_enforcement(&units, &package, &SettingsSpans::new()).is_ok());
    }

    // --- resolution chain: validator refs ---

    #[test]
    fn validator_allowed_by_default() {
        let package = SettingsDict::new();
        let units = vec![a_struct("S", vec![], vec![a_field("f", vec![validator_ref("StringBounds")])])];
        assert!(check_settings_enforcement(&units, &package, &SettingsSpans::new()).is_ok());
    }

    #[test]
    fn validator_forbidden_by_global_coarse() {
        let package = package_with(&["validators", "allowed"], b(false));
        let units = vec![a_struct("S", vec![], vec![a_field("f", vec![validator_ref("StringBounds")])])];
        let errors = check_settings_enforcement(&units, &package, &SettingsSpans::new()).unwrap_err();
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
        assert!(check_settings_enforcement(&units, &package, &SettingsSpans::new()).is_ok());
    }

    #[test]
    fn decl_scoped_named_is_the_most_specific() {
        let mut package = package_with(&["struct", "field", "validators", "allowed"], b(true));
        let specific = nested(&["struct", "field", "validators", "StringBounds", "allowed"], b(false));
        package = crate::settings::merge::merge(&package, &specific);
        let units = vec![a_struct("S", vec![], vec![a_field("f", vec![validator_ref("StringBounds")])])];
        let errors = check_settings_enforcement(&units, &package, &SettingsSpans::new()).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("struct.field.validators.StringBounds.allowed"));
    }

    // --- decl-path coverage, one per kind ---

    #[test]
    fn struct_field_uses_struct_field_path() {
        let package = package_with(&["struct", "field", "annotations", "x", "allowed"], b(false));
        let units = vec![a_struct("S", vec![], vec![a_field("f", vec![property("x")])])];
        let errors = check_settings_enforcement(&units, &package, &SettingsSpans::new()).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].span, Some((0, 1)));
    }

    #[test]
    fn error_field_uses_error_field_path() {
        let package = package_with(&["error", "field", "annotations", "x", "allowed"], b(false));
        let units = vec![an_error("E", vec![], vec![a_field("f", vec![property("x")])])];
        let errors = check_settings_enforcement(&units, &package, &SettingsSpans::new()).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].span, Some((0, 1)));
    }

    #[test]
    fn bare_error_violation_has_no_span() {
        let package = package_with(&["error", "annotations", "x", "allowed"], b(false));
        let units = vec![an_error("E", vec![property("x")], vec![])];
        let errors = check_settings_enforcement(&units, &package, &SettingsSpans::new()).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].span, None);
    }

    #[test]
    fn protocol_function_uses_protocol_function_path() {
        let package = package_with(&["protocol", "function", "annotations", "x", "allowed"], b(false));
        let units = vec![a_protocol("P", vec![], vec![a_function("f", vec![property("x")])])];
        let errors = check_settings_enforcement(&units, &package, &SettingsSpans::new()).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].span, Some((0, 1)));
    }

    #[test]
    fn protocol_itself_uses_protocol_path() {
        let package = package_with(&["protocol", "annotations", "framing", "allowed"], b(false));
        let units = vec![a_protocol("P", vec![property("framing")], vec![])];
        let errors = check_settings_enforcement(&units, &package, &SettingsSpans::new()).unwrap_err();
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
        let errors = check_settings_enforcement(&units, &package, &SettingsSpans::new()).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("Missing"));
        assert!(errors[0].message.contains("not found"));
    }

    // --- end-to-end: real parsed source, not hand-built fixtures ---

    #[test]
    fn malformed_settings_value_from_real_parsed_source_points_at_its_true_origin() {
        // Reproduces the exact reported scenario: a struct field uses a
        // validator governed by a settings key whose value, elsewhere in
        // the same file, is a typo'd non-boolean. Goes through the real
        // parser + freeze pipeline (`IncrementalInterpreter::from_source`),
        // not a hand-built `FrozenUnit`/`SettingsSpans` fixture - this is
        // the thing that actually proves span capture is wired correctly
        // end to end, not just that `verdict()`'s branching logic is right.
        use crate::schema::ir::compiler::interpreter::incremental::IncrementalInterpreter;
        use crate::schema::ir::compiler::Compile;

        let source = "\
struct Message {
    @validators = [StringBounds(min=3, max=10)]
    message: str
}

settings {
    validators.allowed = Tru
}
";
        let units = IncrementalInterpreter::from_source(source);
        let errors =
            check_settings_enforcement(&units, &SettingsDict::new(), &SettingsSpans::default())
                .unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("must be a boolean"));

        let origin = errors[0].span.expect("origin span should be known from real parsing");
        let origin_text = &source[origin.0..origin.1];
        assert!(
            origin_text.contains("Tru") && origin_text.contains("validators.allowed"),
            "primary span should point at the settings entry itself: {origin_text:?}"
        );

        let (usage_span, _) =
            errors[0].secondary.as_ref().expect("secondary span should point at the usage site");
        let usage_text = &source[usage_span.0..usage_span.1];
        assert!(
            usage_text.contains("@validators") && usage_text.contains("message"),
            "secondary span should point at the field that uses the setting: {usage_text:?}"
        );
    }

    #[test]
    fn resolved_settings_preset_is_not_an_error() {
        let package = SettingsDict::new();
        let units = vec![
            FrozenUnit::Settings {
                docstring: None, name: Some("Strict".to_string()),
                values: SettingsDict::new(), spans: SettingsSpans::new(),
            },
            a_struct("S", vec![
                FrozenUnit::Property { name: "settings".to_string(), expression: Some("Strict".to_string()) },
            ], vec![]),
        ];
        assert!(check_settings_enforcement(&units, &package, &SettingsSpans::new()).is_ok());
    }
}

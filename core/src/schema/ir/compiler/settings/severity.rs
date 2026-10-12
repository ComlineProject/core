//! Settings-driven diagnostic severity: `diagnostics."<code>".level`
//! overrides a diagnostic code's effective severity project-wide (merged
//! package + schema `settings` blocks - not decl-scoped, unlike
//! `enforcement`'s `.allowed` keys). A separate, independently-callable
//! pass chained after `validation::validate` (and `check_imports`/
//! `check_aliases`/`check_settings_enforcement`), same calling convention
//! as its siblings here.

use crate::diagnostics::{Diagnostic, Severity};
use crate::schema::ir::frozen::unit::FrozenUnit;
use crate::settings::effective::{
    effective_schema_settings, effective_schema_settings_spans, schema_settings,
};
use crate::config_value::{self, ValueForm};
use crate::settings::keys;
use crate::settings::value::{SettingsDict, SettingsSpans, SettingsValue};

/// Applies any `diagnostics."<code>".level` overrides to an
/// already-produced list of non-fatal diagnostics (the `Ok(warnings)`
/// `validation::validate` returns), splitting the result into what's
/// still non-fatal and what must now fail the build - **a partition, not
/// a pass/fail check**: escalating a code to `"error"` is the whole point
/// of this feature, so it has to actually move the diagnostic into the
/// caller's fatal-error path, not just relabel it while still treating it
/// as a warning. A malformed `level` value does the same (its own
/// `settings::invalid-diagnostic-level` error) - both land in the second
/// element, together, so one schema's unrelated-but-still-fine warnings
/// (first element) aren't silently dropped just because this schema also
/// has a different, broken or escalated one.
///
/// A code whose own intrinsic severity is `Error` is never touched -
/// settings can soften or silence a warning, not downgrade a hard error
/// (checked per-diagnostic, not against a central code registry - Phase 1
/// deliberately has none). A recognized level (`"allow"`, `"hint"`,
/// `"info"`, `"warn"`, `"error"`) either drops the diagnostic (`"allow"`)
/// or sets its [`Diagnostic::severity_override`]; the malformed-value
/// error points at the settings entry's own span when known (via
/// `effective_schema_settings_spans`, the same span-tracking machinery
/// `enforcement::verdict`'s malformed-value case already uses) and falls
/// back to the original diagnostic's own span otherwise.
pub fn resolve_diagnostic_overrides(
    units: &[FrozenUnit],
    diagnostics: Vec<Diagnostic>,
    package_settings: &SettingsDict,
    package_spans: &SettingsSpans,
) -> (Vec<Diagnostic>, Vec<Diagnostic>) {
    let schema = schema_settings(units);
    let effective = effective_schema_settings(package_settings, &schema);
    let effective_spans = effective_schema_settings_spans(package_spans, &schema);

    let mut warnings = Vec::new();
    let mut fatal = Vec::new();

    for diagnostic in diagnostics {
        if diagnostic.code.severity == Severity::Error {
            warnings.push(diagnostic);
            continue;
        }

        let shape = keys::diagnostics_level();
        let key = shape.key(None, Some(diagnostic.code.id));
        let Some(value) = effective.get_path(&key) else {
            warnings.push(diagnostic);
            continue;
        };

        // One check against the registry replaces both the "is it a
        // string" test and the open-coded membership match - the five
        // accepted spellings now live only in the registry.
        if config_value::check(&shape.value, value.form()).is_err() {
            fatal.push(malformed(&key, value, &effective_spans, &diagnostic));
            continue;
        }

        let ValueForm::Str(level) = value.form() else {
            fatal.push(malformed(&key, value, &effective_spans, &diagnostic));
            continue;
        };

        match level {
            "allow" => {}
            "hint" => warnings.push(diagnostic.with_severity_override(Severity::Hint)),
            "info" => warnings.push(diagnostic.with_severity_override(Severity::Info)),
            "warn" => warnings.push(diagnostic.with_severity_override(Severity::Warning)),
            "error" => fatal.push(diagnostic.with_severity_override(Severity::Error)),
            // Unreachable: `check` above accepted it, so it is one of the
            // five. Kept as a real arm rather than `unreachable!` so a new
            // registry spelling degrades to "malformed" instead of panicking.
            _ => fatal.push(malformed(&key, value, &effective_spans, &diagnostic)),
        }
    }

    (warnings, fatal)
}

fn malformed(
    key: &str,
    value: &SettingsValue,
    effective_spans: &SettingsSpans,
    diagnostic: &Diagnostic,
) -> Diagnostic {
    let shape = keys::diagnostics_level();
    let expected = config_value::describe(&shape.value);
    let d = Diagnostic::new(
        shape.malformed,
        format!("'{key}' must be {expected}, got {}", value.form()),
    )
    .with_help(format!("a diagnostics '{}' key only accepts {expected}", shape.leaf));

    match effective_spans.get(key) {
        Some(origin) => d.with_span(origin).with_context(format!("'{key}' set here")),
        None => d
            .maybe_span(diagnostic.span)
            .with_context(format!("while resolving '{}'", diagnostic.code.id)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::DiagnosticCode;

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

    const WARNING_CODE: DiagnosticCode = DiagnosticCode::warning("test::some-warning");
    const ERROR_CODE: DiagnosticCode = DiagnosticCode::error("test::some-error");

    #[test]
    fn no_override_leaves_the_diagnostic_unchanged() {
        let diagnostics = vec![Diagnostic::new(WARNING_CODE, "msg")];
        let (warnings, fatal) = resolve_diagnostic_overrides(
            &[], diagnostics.clone(), &SettingsDict::new(), &SettingsSpans::new(),
        );
        assert_eq!(warnings, diagnostics);
        assert!(fatal.is_empty());
    }

    #[test]
    fn allow_drops_the_diagnostic() {
        let package = nested(&["diagnostics", "test::some-warning", "level"], SettingsValue::Str("allow".to_string()));
        let diagnostics = vec![Diagnostic::new(WARNING_CODE, "msg")];
        let (warnings, fatal) = resolve_diagnostic_overrides(&[], diagnostics, &package, &SettingsSpans::new());
        assert_eq!(warnings, vec![]);
        assert!(fatal.is_empty());
    }

    #[test]
    fn error_moves_the_diagnostic_into_the_fatal_list() {
        // The whole point of escalating to "error": it must actually move
        // into the caller's fatal path, not just carry an Error-flavored
        // label while still being treated as a mere warning.
        let package = nested(&["diagnostics", "test::some-warning", "level"], SettingsValue::Str("error".to_string()));
        let diagnostics = vec![Diagnostic::new(WARNING_CODE, "msg")];
        let (warnings, fatal) = resolve_diagnostic_overrides(&[], diagnostics, &package, &SettingsSpans::new());
        assert!(warnings.is_empty());
        assert_eq!(fatal.len(), 1);
        assert_eq!(fatal[0].effective_severity(), Severity::Error);
        assert_eq!(fatal[0].code.severity, Severity::Warning, "the code's own severity is untouched");
    }

    #[test]
    fn hint_info_and_warn_remap_but_stay_non_fatal() {
        for (level, expect) in [("hint", Severity::Hint), ("info", Severity::Info), ("warn", Severity::Warning)] {
            let package = nested(&["diagnostics", "test::some-warning", "level"], SettingsValue::Str(level.to_string()));
            let diagnostics = vec![Diagnostic::new(WARNING_CODE, "msg")];
            let (warnings, fatal) = resolve_diagnostic_overrides(&[], diagnostics, &package, &SettingsSpans::new());
            assert!(fatal.is_empty(), "level={level}");
            assert_eq!(warnings[0].effective_severity(), expect, "level={level}");
        }
    }

    #[test]
    fn error_severity_codes_are_never_overridden() {
        let package = nested(&["diagnostics", "test::some-error", "level"], SettingsValue::Str("allow".to_string()));
        let diagnostics = vec![Diagnostic::new(ERROR_CODE, "msg")];
        let (warnings, fatal) = resolve_diagnostic_overrides(&[], diagnostics, &package, &SettingsSpans::new());
        assert!(fatal.is_empty(), "an Error-severity code's override is silently ignored, not applied");
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].effective_severity(), Severity::Error);
    }

    #[test]
    fn an_escalation_does_not_drop_an_unrelated_still_fine_warning() {
        let package = nested(&["diagnostics", "test::some-warning", "level"], SettingsValue::Str("error".to_string()));
        const OTHER_CODE: DiagnosticCode = DiagnosticCode::warning("test::unrelated");
        let diagnostics = vec![
            Diagnostic::new(WARNING_CODE, "escalated"),
            Diagnostic::new(OTHER_CODE, "still just a warning"),
        ];
        let (warnings, fatal) = resolve_diagnostic_overrides(&[], diagnostics, &package, &SettingsSpans::new());
        assert_eq!(fatal.len(), 1);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].message, "still just a warning");
    }

    #[test]
    fn malformed_value_is_its_own_fatal_error() {
        let package = nested(&["diagnostics", "test::some-warning", "level"], SettingsValue::Str("nope".to_string()));
        let diagnostics = vec![Diagnostic::new(WARNING_CODE, "msg").with_span((5, 6))];
        let (warnings, fatal) = resolve_diagnostic_overrides(&[], diagnostics, &package, &SettingsSpans::new());
        assert!(warnings.is_empty());
        assert_eq!(fatal.len(), 1);
        assert_eq!(fatal[0].code.id, "settings::invalid-diagnostic-level");
        assert!(fatal[0].message.contains("nope"));
    }

    #[test]
    fn malformed_value_falls_back_to_the_diagnostics_own_span_with_no_known_origin() {
        let package = nested(&["diagnostics", "test::some-warning", "level"], SettingsValue::Str("nope".to_string()));
        let diagnostics = vec![Diagnostic::new(WARNING_CODE, "msg").with_span((5, 6))];
        let (_, fatal) = resolve_diagnostic_overrides(&[], diagnostics, &package, &SettingsSpans::new());
        assert_eq!(fatal[0].span, Some((5, 6)));
    }

    #[test]
    fn malformed_value_points_at_its_origin_when_known() {
        let package = nested(&["diagnostics", "test::some-warning", "level"], SettingsValue::Str("nope".to_string()));
        let mut spans = SettingsSpans::new();
        spans.0.insert("diagnostics.test::some-warning.level".to_string(), (40, 44));
        let diagnostics = vec![Diagnostic::new(WARNING_CODE, "msg").with_span((5, 6))];
        let (_, fatal) = resolve_diagnostic_overrides(&[], diagnostics, &package, &spans);
        assert_eq!(fatal[0].span, Some((40, 44)));
    }

    #[test]
    fn non_string_value_is_also_malformed() {
        let package = nested(&["diagnostics", "test::some-warning", "level"], SettingsValue::Bool(true));
        let diagnostics = vec![Diagnostic::new(WARNING_CODE, "msg")];
        let (_, fatal) = resolve_diagnostic_overrides(&[], diagnostics, &package, &SettingsSpans::new());
        assert_eq!(fatal.len(), 1);
        assert_eq!(fatal[0].code.id, "settings::invalid-diagnostic-level");
    }
}

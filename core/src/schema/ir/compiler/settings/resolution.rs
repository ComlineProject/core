//! Pre-freeze validation for `settings` blocks - same spirit as
//! `alias_resolution`: a `settings` block's dotted keys and reserved
//! `mode` key need to make sense before freezing produces a
//! `FrozenUnit::Settings`, and neither check fits naturally into
//! `validation::validator`'s post-freeze `SymbolTable`-based passes (that
//! pass doesn't even look at `FrozenUnit::Settings` - see its module doc).

use crate::schema::idl::grammar::Declaration;
use crate::settings::desugar::desugar;
use crate::diagnostics::{Diagnostic, DiagnosticCode};
use super::conformance::check_settings_conformance;

const CONFLICT: DiagnosticCode = DiagnosticCode::error("settings::conflict");

/// Validate every `settings` block in `declarations`: its dotted keys must
/// describe one consistent nested shape (no leaf-vs-group conflict, no
/// exact duplicate), and once that holds, every key and value must match
/// the registry (see [`super::conformance`]). Both checks run against
/// every block found, collecting every error rather than stopping at the
/// first.
pub fn check_settings_conflicts(
    declarations: &[rust_sitter::Spanned<Declaration>],
) -> Result<(), Vec<Diagnostic>> {
    let mut errors = Vec::new();

    for decl in declarations {
        let Declaration::Settings(settings_def) = &decl.value else {
            continue;
        };
        let label = settings_def
            .name()
            .map(|n| format!("settings block '{n}'"))
            .unwrap_or_else(|| "the unnamed settings block".to_string());

        let entries: Vec<_> = settings_def
            .entries()
            .iter()
            .map(|e| e.value.to_path_value())
            .collect();

        match desugar(&entries) {
            Err(conflicts) => {
                for c in conflicts {
                    errors.push(Diagnostic {
                        code: CONFLICT,
                        help: Some(
                            "a dotted key can't be used as both a single value and a \
                             group of nested values"
                                .to_string(),
                        ),
                        message: format!(
                            "Conflicting settings keys: '{}' vs '{}'",
                            c.existing_path, c.conflicting_path
                        ),
                        context: format!("in {label}"),
                        span: Some(decl.span),
                        secondary: None,
                        severity_override: None,
                    });
                }
            }
            Ok(values) => {
                // Per-entry origin spans, so a conformance error points at
                // the offending line rather than the whole block. Built
                // the same way the freezing pass builds its own sidecar.
                let spans = crate::settings::SettingsSpans(
                    settings_def
                        .entries()
                        .iter()
                        .map(|entry| (entry.value.key_path().join("."), entry.span))
                        .collect(),
                );
                // Subsumes what `merge::validate_mode_keys` used to report
                // here: it walks every leaf, `mode` included, and checks
                // each against the registry - so an invalid `mode` is
                // reported once, by its own code, instead of twice.
                if let Err(conformance_errors) = check_settings_conformance(
                    &values,
                    &spans,
                    &label,
                    Some(decl.span),
                ) {
                    errors.extend(conformance_errors);
                }
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::idl::grammar;

    fn decls(source: &str) -> Vec<rust_sitter::Spanned<Declaration>> {
        grammar::parse(source).expect("should parse").0
    }

    #[test]
    fn a_consistent_settings_block_passes() {
        let declarations = decls("settings {\n    struct.field.validators.allowed = true\n}");
        assert!(check_settings_conflicts(&declarations).is_ok());
    }

    #[test]
    fn a_leaf_vs_group_conflict_is_rejected() {
        let declarations = decls("settings {\n    a.b = true\n    a.b.c = 1\n}");
        let errors = check_settings_conflicts(&declarations).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("a.b"));
    }

    #[test]
    fn mode_replace_passes() {
        let declarations = decls("settings {\n    mode = replace\n}");
        assert!(check_settings_conflicts(&declarations).is_ok());
    }

    #[test]
    fn an_invalid_mode_value_is_rejected() {
        let declarations = decls("settings {\n    mode = merge\n}");
        let errors = check_settings_conflicts(&declarations).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("mode"));
    }

    #[test]
    fn a_named_block_is_checked_too() {
        let declarations = decls("settings Strict {\n    mode = nope\n}");
        let errors = check_settings_conflicts(&declarations).unwrap_err();
        assert!(errors[0].context.contains("Strict"));
    }
}

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
const DUPLICATE_BLOCK: DiagnosticCode = DiagnosticCode::error("settings::duplicate-block");

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
    // A schema has one unnamed block and at most one block per name.
    // `effective::schema_settings` collects them into a map keyed by name
    // (and a single `unnamed` slot), so a second block with the same name
    // silently replaced the first and everything written in it was
    // discarded - no diagnostic anywhere.
    // Name -> where the first block of that name was written, so the second
    // can point back at it instead of describing a site the reader then has
    // to go and find.
    let mut seen: Vec<(Option<String>, (usize, usize))> = Vec::new();

    for decl in declarations {
        let Declaration::Settings(settings_def) = &decl.value else {
            continue;
        };
        let label = settings_def
            .name()
            .map(|n| format!("settings block '{n}'"))
            .unwrap_or_else(|| "the unnamed settings block".to_string());

        let name = settings_def.name();
        if let Some((_, first)) = seen.iter().find(|(seen_name, _)| *seen_name == name) {
            errors.push(
                Diagnostic::new(DUPLICATE_BLOCK, format!("Duplicate {label}"))
                    .with_context("declared twice in this schema".to_string())
                    .with_help("merge them - only the last one has any effect".to_string())
                    .with_span(decl.span)
                    .with_secondary_span(*first, format!("{label} declared here")),
            );
        }
        seen.push((name, decl.span));

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

#[cfg(test)]
mod duplicate_block_tests {
    use super::*;
    use crate::schema::idl::grammar;

    fn blocks(source: &str) -> Vec<Diagnostic> {
        let document = grammar::parse(source).expect("source should parse");
        check_settings_conflicts(&document.0).err().unwrap_or_default()
    }

    /// Like a duplicate declaration, a duplicate block is a two-site
    /// problem: the second one is the error, the first is what it shadows.
    #[test]
    fn a_duplicate_named_block_points_back_at_the_first() {
        let source = "settings Strict {\n    validators.allowed = true\n}\n\n\
                      settings Strict {\n    validators.allowed = false\n}\n";
        let errors = blocks(source);

        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code.id, "settings::duplicate-block");

        let (start, end) = errors[0].span.expect("spanned at the second block");
        assert!(source[start..end].contains("false"), "the primary is the second block");

        let (sec, label) = errors[0].secondary.as_ref().expect("the first block is named too");
        assert!(source[sec.0..sec.1].contains("true"), "the secondary is the first block");
        assert_eq!(label, "settings block 'Strict' declared here");
    }

    #[test]
    fn the_unnamed_block_gets_the_same_treatment() {
        let source = "settings {\n    validators.allowed = true\n}\n\n\
                      settings {\n    validators.allowed = false\n}\n";
        let errors = blocks(source);

        assert_eq!(errors.len(), 1);
        let (_, label) = errors[0].secondary.as_ref().expect("two sites");
        assert_eq!(label, "the unnamed settings block declared here");
    }

    /// Two blocks with *different* names are the normal case.
    #[test]
    fn differently_named_blocks_are_fine() {
        let source = "settings Strict {\n    validators.allowed = true\n}\n\n\
                      settings Loose {\n    validators.allowed = false\n}\n";
        assert!(blocks(source).is_empty());
    }
}

//! Pre-freeze validation for `settings` blocks - same spirit as
//! `alias_resolution`: a `settings` block's dotted keys and reserved
//! `mode` key need to make sense before freezing produces a
//! `FrozenUnit::Settings`, and neither check fits naturally into
//! `validation::validator`'s post-freeze `SymbolTable`-based passes (that
//! pass doesn't even look at `FrozenUnit::Settings` - see its module doc).

use crate::schema::idl::grammar::Declaration;
use crate::settings::desugar::desugar;
use crate::settings::merge::validate_mode_keys;
use crate::diagnostics::Diagnostic;

/// Validate every `settings` block in `declarations`: its dotted keys must
/// describe one consistent nested shape (no leaf-vs-group conflict, no
/// exact duplicate), and wherever a reserved `mode` key appears, its value
/// must be the bare keyword `replace`. Both checks run against every
/// block found, collecting every error rather than stopping at the first.
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
                    });
                }
            }
            Ok(values) => {
                if let Err(mode_errors) = validate_mode_keys(&values) {
                    for e in mode_errors {
                        let where_ = if e.path.is_empty() {
                            label.clone()
                        } else {
                            format!("{label}, at '{}'", e.path)
                        };
                        errors.push(Diagnostic {
                            help: Some(
                                "'mode' only accepts the bare keyword 'replace' - remove it \
                                 to merge (the default) instead"
                                    .to_string(),
                            ),
                            message: format!(
                                "Invalid 'mode' value: {:?} is not 'replace'",
                                e.found
                            ),
                            context: where_,
                            span: Some(decl.span),
                        });
                    }
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
        let declarations = decls("settings {\n    struct.field.validators.allowed = True\n}");
        assert!(check_settings_conflicts(&declarations).is_ok());
    }

    #[test]
    fn a_leaf_vs_group_conflict_is_rejected() {
        let declarations = decls("settings {\n    a.b = True\n    a.b.c = 1\n}");
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

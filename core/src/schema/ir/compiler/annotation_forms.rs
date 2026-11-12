//! How an annotation is written: `@key = value` or `@key(name = value, ...)`,
//! never both. The keyed form is the same as `= { name = value, ... }`, so an
//! annotation that writes both says two things and nothing a reader could
//! trust.
//!
//! Checked on the parsed schema, where it's still known which was written.

use crate::diagnostics::{Diagnostic, DiagnosticCode};
use crate::schema::idl::grammar::{Annotation, Declaration};

const BOTH_FORMS: DiagnosticCode = DiagnosticCode::error("annotations::both-forms");

/// Every annotation of `declarations` (declarations, fields, functions and
/// their arguments) that writes both `= value` and `(...)`.
pub fn check_annotation_forms(
    declarations: &[rust_sitter::Spanned<Declaration>],
) -> Result<(), Vec<Diagnostic>> {
    let mut errors = Vec::new();
    let mut check = |annotations: Vec<&Annotation>, span: (usize, usize)| {
        for annotation in annotations.into_iter().filter(|a| a.both_forms()) {
            let key = annotation.key();
            errors.push(
                Diagnostic::new(
                    BOTH_FORMS,
                    format!("`@{key}` is written with both `= value` and `(name = value, ...)`"),
                )
                .with_help(format!(
                    "write `@{key} = {{ name = value }}` or `@{key}(name = value)`, not both"
                ))
                .with_span(span),
            );
        }
    };
    for declaration in declarations {
        let span = declaration.span;
        match &declaration.value {
            Declaration::Struct(s) => {
                check(s.annotations(), span);
                for field in s.fields() {
                    check(field.value.annotations(), field.span);
                }
            }
            Declaration::Error(e) => {
                check(e.annotations(), span);
                for field in e.fields() {
                    check(field.value.annotations(), field.span);
                }
            }
            Declaration::TypeAlias(t) => check(t.annotations(), span),
            Declaration::Protocol(p) => {
                check(p.annotations(), span);
                for function in p.functions() {
                    check(function.value.annotations(), function.span);
                    if let Some(list) = function.value.args() {
                        check(list.first().annotations(), function.span);
                        for comma in list.rest() {
                            check(comma.arg_type().annotations(), function.span);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

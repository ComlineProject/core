// IR-compile/validation diagnostics rendering, using ariadne for rich,
// source-span-aware output. Parallel to `schema::idl::diagnostics`, which
// handles grammar/parse errors with `codespan_reporting` - this module
// handles the later IR-compile and validation stages, using the byte
// ranges now carried directly on `FrozenUnit`/`ValidationError`.

use std::ops::Range;

use ariadne::{Label, Report, ReportKind, Source};

use crate::schema::ir::context::SchemaContext;
use crate::schema::ir::validation::ValidationError;

type SpanId = (String, Range<usize>);

/// Render a `ValidationError` as a rich, source-span-aware diagnostic
/// string. Falls back to a plain message if the error carries no span (or
/// the schema's source isn't available), rather than failing.
pub fn render_validation_error(error: &ValidationError, schema_context: &SchemaContext) -> String {
    render(&error.message, &error.context, error.span, schema_context)
}

/// Render a plain compile-time message (e.g. from `CompileError`'s
/// `Display`) at the given span, the same way `render_validation_error`
/// does for validation errors.
pub fn render_compile_message(
    message: &str,
    label: &str,
    span: (usize, usize),
    schema_context: &SchemaContext,
) -> String {
    render(message, label, Some(span), schema_context)
}

fn render(
    message: &str,
    label: &str,
    span: Option<(usize, usize)>,
    schema_context: &SchemaContext,
) -> String {
    let Some(span) = span else {
        return format!("{}\n  {}", message, label);
    };

    let Some(file) = schema_context.codemap.files().first() else {
        return format!("{}\n  {}", message, label);
    };

    let filename = file.filename().to_string();
    let source = file.contents().to_string();
    let byte_range: Range<usize> = span.0..span.1;

    let report = Report::<SpanId>::build(ReportKind::Error, filename.clone(), span.0)
        .with_message(message)
        .with_label(Label::new((filename.clone(), byte_range)).with_message(label))
        .finish();

    let mut buf = Vec::new();
    match report.write((filename, Source::from(source)), &mut buf) {
        Ok(()) => String::from_utf8_lossy(&buf).into_owned(),
        Err(_) => format!("{}\n  {}", message, label),
    }
}
